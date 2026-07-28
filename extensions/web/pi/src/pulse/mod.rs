//! `GET /api/public/pi/pulse` — the deployment, counted across everybody.
//!
//! The pane beside the terminal proves we record a visitor's own session. It
//! cannot prove the machinery is not a diorama built for them. This endpoint
//! does: the same tables, the same policies, aggregated over every account.
//!
//! # One endpoint, three audiences
//!
//! Who is asking decides how much comes back, and the decision is made here
//! rather than in the browser (see [`super::tier`]):
//!
//! - **Anonymous** — lifetime totals only. There is no window, because a window
//!   is where the identifiable numbers live.
//! - **Member** — the 24h window, bucketed by [`super::normalize`] and withheld
//!   entirely when too few people are in it to aggregate safely.
//! - **Admin** — the window unrounded, plus a `detail` block: traffic, per-tool
//!   and per-agent rollups, the busiest accounts, and the activity shape of the
//!   last week.
//!
//! Nothing about the tier is echoed to the caller. A client that knew its own
//! rank would eventually be asked to enforce it, which is the same reason
//! `GET /admin/auth/me` omits `is_admin`.
//!
//! # Two properties this must keep
//!
//! - **Nothing identifying leaves below the admin tier.** The repository layer
//!   returns only counts for the window (see
//!   [`systemprompt_web_governance::repositories::analytics::pulse`]), the member tier rounds those
//!   counts, and a window with fewer than [`super::normalize::MIN_PEOPLE`]
//!   accounts in it is not sent at all. The admin tier is the sole exception,
//!   and it is the tier that could already read every one of these rows from
//!   the CLI.
//! - **It cannot be polled into a table scan.** The pane refreshes the pulse
//!   once a minute and the admin block is far more expensive than the member
//!   one, so each tier's answer is computed at most once per its own TTL and
//!   every caller in that interval is served the same snapshot.
//!
//! The token is optional, which is what makes the anonymous tier reachable at
//! all: `/embed-token` reads the session cookie, so a visitor who has not
//! signed in has no token to present. Requiring one would have left the tier
//! the homepage most wants to reach as the only one that could never load.

use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde::Deserialize;
use sqlx::PgPool;

mod wire;

use wire::{
    AdminDetailOut, AgentRollupOut, BlockedToolOut, ModelShareOut, PulseResponse, PulseTotalsOut,
    PulseWindowOut, ToolRollupOut, TopUserOut, count, tokens,
};

use super::auth::problem;
use super::tier::{Tier, resolve};
use super::{format, normalize};
use systemprompt_web_governance::repositories::analytics::pulse as repo;
use systemprompt_web_governance::repositories::{analytics, dashboard};

const CACHE_TTL: Duration = Duration::from_secs(60);

const ADMIN_CACHE_TTL: Duration = Duration::from_secs(120);

const WINDOW_HOURS: i64 = 24;

const MODEL_LIMIT: i64 = 5;

const ADMIN_MODEL_LIMIT: i64 = 12;

const ADMIN_BLOCKED_LIMIT: i64 = 10;

const ADMIN_TRAFFIC_RANGE: &str = "30d";

type Snapshot = Option<(Instant, PulseResponse)>;

static CACHE: LazyLock<Mutex<[Snapshot; Tier::COUNT]>> =
    LazyLock::new(|| Mutex::new([const { None }; Tier::COUNT]));

#[derive(Debug, Deserialize)]
pub(super) struct PulseQuery {
    #[serde(default)]
    token: String,
}

pub(super) async fn pulse(
    State(pool): State<Arc<PgPool>>,
    Query(q): Query<PulseQuery>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let tier = resolve(&pool, &q.token).await;

    if let Some(cached) = fresh_snapshot(tier) {
        return Json(cached).into_response();
    }

    match collect(&pool, tier).await {
        Ok(snapshot) => {
            if let Ok(mut guard) = CACHE.lock() {
                guard[tier.index()] = Some((Instant::now(), snapshot.clone()));
            }
            Json(snapshot).into_response()
        },
        Err(e) => {
            tracing::error!(error = %e, ?tier, "could not read the platform pulse");
            stale_snapshot(tier).map_or_else(
                // lint-ok: http-error — logged above
                || {
                    problem(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "could not read the pulse",
                    )
                },
                |stale| Json(stale).into_response(),
            )
        },
    }
}

const fn ttl(tier: Tier) -> Duration {
    match tier {
        Tier::Admin => ADMIN_CACHE_TTL,
        _ => CACHE_TTL,
    }
}

fn snapshot_at(tier: Tier) -> Option<(Duration, PulseResponse)> {
    let guard = CACHE
        .lock()
        .inspect_err(|_| tracing::warn!("pulse cache mutex poisoned; recomputing snapshot"))
        .ok()?;
    let taken = guard[tier.index()]
        .as_ref()
        .map(|(at, snapshot)| (at.elapsed(), snapshot.clone()));
    drop(guard);
    taken
}

fn fresh_snapshot(tier: Tier) -> Option<PulseResponse> {
    let (age, snapshot) = snapshot_at(tier)?;
    (age < ttl(tier)).then(|| snapshot.aged(age.as_secs()))
}

fn stale_snapshot(tier: Tier) -> Option<PulseResponse> {
    let (age, snapshot) = snapshot_at(tier)?;
    Some(snapshot.aged(age.as_secs()))
}

impl PulseResponse {
    const fn aged(mut self, seconds: u64) -> Self {
        self.age_seconds = seconds;
        self
    }
}

async fn collect(pool: &PgPool, tier: Tier) -> Result<PulseResponse, sqlx::Error> {
    let all_time = repo::get_pulse_all_time(pool).await?;

    let window = match tier {
        Tier::Anonymous => None,
        Tier::Member | Tier::Admin => collect_window(pool, tier).await?,
    };

    let detail = match tier {
        Tier::Admin => Some(Box::new(collect_detail(pool).await?)),
        Tier::Anonymous | Tier::Member => None,
    };

    let exact = tier == Tier::Admin;
    Ok(PulseResponse {
        age_seconds: 0,
        window_hours: WINDOW_HOURS,
        window,
        all_time: PulseTotalsOut {
            sessions: count(all_time.sessions, exact),
            requests: count(all_time.requests, exact),
            tool_calls: count(all_time.tool_calls, exact),
            secrets_caught: count(all_time.secrets_caught, exact),
        },
        detail,
    })
}

async fn collect_window(pool: &PgPool, tier: Tier) -> Result<Option<PulseWindowOut>, sqlx::Error> {
    let since = Utc::now() - chrono::Duration::hours(WINDOW_HOURS);
    let exact = tier == Tier::Admin;

    let window = repo::get_pulse_window(pool, since).await?;

    if !exact && !normalize::window_is_publishable(window.people) {
        return Ok(None);
    }

    let model_limit = if exact {
        ADMIN_MODEL_LIMIT
    } else {
        MODEL_LIMIT
    };
    let blocked_limit = if exact { ADMIN_BLOCKED_LIMIT } else { 1 };
    let models = repo::list_pulse_model_mix(pool, since, model_limit).await?;
    let blocked = repo::list_pulse_blocked_tools(pool, since, blocked_limit).await?;

    let decided = window.allowed + window.denied;
    let allow_rate_percent = (decided > 0).then(|| window.allowed.saturating_mul(100) / decided);

    let model_total: i64 = models.iter().map(|m| m.requests).sum();
    let model_mix = models
        .into_iter()
        .map(|m| ModelShareOut {
            percent: if model_total > 0 {
                m.requests.saturating_mul(100) / model_total
            } else {
                0
            },
            requests: count(m.requests, exact),
            model: m.model,
        })
        .collect();

    Ok(Some(PulseWindowOut {
        people: count(window.people, exact),
        sessions: count(window.sessions, exact),
        requests: count(window.requests, exact),
        tool_calls: count(window.tool_calls, exact),
        allowed: count(window.allowed, exact),
        denied: count(window.denied, exact),
        allow_rate_percent,
        latency_p50_ms: window.latency_p50_ms,
        input_tokens: tokens(window.input_tokens, exact),
        output_tokens: tokens(window.output_tokens, exact),
        cost_display: format::cost(window.cost_microdollars),
        model_mix,
        blocked_tools: blocked
            .into_iter()
            .map(|b| BlockedToolOut {
                tool_name: b.tool_name,
                denials: count(b.denials, exact),
            })
            .collect(),
    }))
}

async fn collect_detail(pool: &PgPool) -> Result<AdminDetailOut, sqlx::Error> {
    let traffic = dashboard::traffic::get_traffic_data(pool, ADMIN_TRAFFIC_RANGE).await?;
    let realtime = dashboard::traffic::get_realtime_pulse(pool).await?;
    let activity = dashboard::aggregates::get_activity_stats(pool).await?;
    let active_users_24h = dashboard::aggregates::get_active_users_24h(pool).await?;
    let top_users = dashboard::queries::list_top_users(pool).await?;
    let popular_skills = dashboard::queries::list_popular_skills(pool).await?;
    let hourly_activity = dashboard::queries::list_hourly_activity(pool).await?;
    let tool_success = dashboard::queries::list_tool_success_rates(pool).await?;
    let tools = analytics::list_tools(pool).await?;
    let agents = analytics::list_agents(pool).await?;

    Ok(AdminDetailOut {
        traffic: Arc::new(traffic),
        realtime,
        activity,
        active_users_24h,
        top_users: top_users.into_iter().map(TopUserOut::from).collect(),
        popular_skills,
        hourly_activity,
        tool_success,
        tools: tools
            .into_iter()
            .map(|t| ToolRollupOut {
                tool_name: t.tool_name,
                calls: t.calls,
                errors: t.errors,
                sessions: t.sessions,
            })
            .collect(),
        agents: agents
            .into_iter()
            .map(|a| AgentRollupOut {
                agent_id: a.agent_id,
                calls: a.calls,
                errors: a.errors,
                sessions: a.sessions,
            })
            .collect(),
    })
}
