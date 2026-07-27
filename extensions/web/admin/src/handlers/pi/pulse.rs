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
//!   [`crate::repositories::analytics::pulse`]), the member tier rounds those
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
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use systemprompt::identifiers::{AgentId, UserId};

use super::auth::problem;
use super::tier::{Tier, resolve};
use super::{format, normalize};
use crate::repositories::analytics::pulse as repo;
use crate::repositories::{analytics, dashboard};
use crate::types::{
    ActivityStats, HourlyActivity, RealtimePulse, SkillCount, ToolSuccessRate, TopUser, TrafficData,
};

/// How long a member or anonymous snapshot is served before recomputation.
const CACHE_TTL: Duration = Duration::from_secs(60);

/// The admin snapshot runs an order of magnitude more work — traffic KPIs alone
/// scan `user_sessions` and `engagement_events` over two periods — so it is
/// held longer. An operator watching a demo does not need sub-two-minute
/// resolution on a weekly rollup.
const ADMIN_CACHE_TTL: Duration = Duration::from_secs(120);

/// The rolling window the headline figures cover.
const WINDOW_HOURS: i64 = 24;

/// Models listed in the mix. Beyond a handful the pane has no room and the tail
/// is noise.
const MODEL_LIMIT: i64 = 5;

/// An admin has room for the tail, and the tail is where a misrouted model
/// shows up.
const ADMIN_MODEL_LIMIT: i64 = 12;

/// Refused tools listed. The member tier sees the single worst offender as
/// colour; an admin sees the distribution, which is the actionable form.
const ADMIN_BLOCKED_LIMIT: i64 = 10;

/// The range string `get_traffic_data` understands for the admin block.
const ADMIN_TRAFFIC_RANGE: &str = "30d";

/// The per-tier snapshot cache, and when each was taken.
///
/// In-process rather than in Redis for the same reason the rate-limit stage is:
/// this deployment is one node, and a stale-by-a-minute counter is not worth a
/// second piece of infrastructure. Keyed by tier because the tiers carry
/// genuinely different payloads — one shared slot would hand a member the admin
/// block whenever an admin polled first.
type Snapshot = Option<(Instant, PulseResponse)>;

static CACHE: LazyLock<Mutex<[Snapshot; Tier::COUNT]>> =
    LazyLock::new(|| Mutex::new([const { None }; Tier::COUNT]));

/// The pulse's own query type rather than [`super::api::TokenQuery`], whose
/// `token` is mandatory. Here its absence is the anonymous tier, not a 400.
#[derive(Debug, Deserialize)]
pub(super) struct PulseQuery {
    #[serde(default)]
    token: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct PulseResponse {
    /// Seconds since this snapshot was computed. The pane can say "as of a
    /// moment ago" honestly instead of implying the number is live.
    age_seconds: u64,
    window_hours: i64,
    /// Absent for an anonymous caller, and for a member whose window holds too
    /// few people to aggregate without identifying them.
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<PulseWindowOut>,
    all_time: PulseTotalsOut,
    /// The operator block. Present only for [`Tier::Admin`].
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<Box<AdminDetailOut>>,
}

/// Counts are strings because the member tier rounds them and the admin tier
/// does not, and one shape for both means one render path in the pane. Rates
/// and latencies stay numeric: a percentage is already an aggregate and reveals
/// nothing a bucket would hide.
#[derive(Debug, Clone, Serialize)]
struct PulseWindowOut {
    people: String,
    sessions: String,
    requests: String,
    tool_calls: String,
    allowed: String,
    denied: String,
    /// Whole percent of governed calls that policy let through. `None` when
    /// nothing was decided — 100% of nothing is not a reassuring claim, it is
    /// a meaningless one.
    allow_rate_percent: Option<i64>,
    latency_p50_ms: Option<i32>,
    input_tokens: String,
    output_tokens: String,
    cost_display: String,
    model_mix: Vec<ModelShareOut>,
    /// The most-refused tools, where anything was refused. One entry below the
    /// admin tier.
    blocked_tools: Vec<BlockedToolOut>,
}

#[derive(Debug, Clone, Serialize)]
struct ModelShareOut {
    model: String,
    requests: String,
    percent: i64,
}

#[derive(Debug, Clone, Serialize)]
struct BlockedToolOut {
    tool_name: String,
    denials: String,
}

#[derive(Debug, Clone, Serialize)]
struct PulseTotalsOut {
    sessions: String,
    requests: String,
    tool_calls: String,
    secrets_caught: String,
}

/// Everything an operator gets that a member does not.
///
/// Boxed in [`PulseResponse`] because it is large and absent for two of the
/// three tiers; inlining it would make every member response carry the
/// footprint of a block it never uses.
#[derive(Debug, Clone, Serialize)]
struct AdminDetailOut {
    /// Site traffic over [`ADMIN_TRAFFIC_RANGE`]: KPIs with period-over-period
    /// comparison, timeseries, sources, geo, devices, top pages.
    traffic: Arc<TrafficData>,
    /// This hour and today, for the top of the block.
    realtime: RealtimePulse,
    /// Lifetime-ish event counters across the plugin spine.
    activity: ActivityStats,
    active_users_24h: i64,
    /// Who is actually using the demo. The one place an identity appears in any
    /// pulse payload, and it appears only here.
    top_users: Vec<TopUserOut>,
    /// What they are running.
    popular_skills: Vec<SkillCount>,
    /// When they run it, by hour of day.
    hourly_activity: Vec<HourlyActivity>,
    /// Which tools work.
    tool_success: Vec<ToolSuccessRate>,
    /// Per-tool and per-agent rollups over the last seven days.
    tools: Vec<ToolRollupOut>,
    agents: Vec<AgentRollupOut>,
}

/// [`TopUser`] without the email address.
///
/// An admin can read the email from the CLI, and this payload reaches a browser
/// on the public homepage. The display name identifies the account for the
/// purpose the block serves — seeing who is exercising the demo — and the
/// address adds nothing to that while being the field worth leaking.
#[derive(Debug, Clone, Serialize)]
struct TopUserOut {
    user_id: UserId,
    display_name: String,
    logins: i64,
    edits: i64,
    mcp_calls: i64,
    last_active: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
struct ToolRollupOut {
    tool_name: String,
    calls: i64,
    errors: i64,
    sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
struct AgentRollupOut {
    agent_id: AgentId,
    calls: i64,
    errors: i64,
    sessions: i64,
}

/// `GET /api/public/pi/pulse?token=…`
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
            // A stale snapshot beats an error card: the section is context, and
            // minute-old context is still true enough to make its point.
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

/// How long this tier's snapshot stays good for.
const fn ttl(tier: Tier) -> Duration {
    match tier {
        Tier::Admin => ADMIN_CACHE_TTL,
        _ => CACHE_TTL,
    }
}

/// The cached snapshot if it is still inside its TTL.
fn fresh_snapshot(tier: Tier) -> Option<PulseResponse> {
    let guard = CACHE.lock().ok()?;
    let (at, snapshot) = guard[tier.index()].as_ref()?;
    let age = at.elapsed();
    (age < ttl(tier)).then(|| snapshot.clone().aged(age.as_secs()))
}

/// The cached snapshot regardless of age, for the error path.
fn stale_snapshot(tier: Tier) -> Option<PulseResponse> {
    let guard = CACHE.lock().ok()?;
    let (at, snapshot) = guard[tier.index()].as_ref()?;
    Some(snapshot.clone().aged(at.elapsed().as_secs()))
}

impl PulseResponse {
    fn aged(mut self, seconds: u64) -> Self {
        self.age_seconds = seconds;
        self
    }
}

async fn collect(pool: &PgPool, tier: Tier) -> Result<PulseResponse, sqlx::Error> {
    let all_time = repo::get_pulse_all_time(pool).await?;

    // The anonymous tier stops here. Not an optimisation — there is no window
    // it is entitled to, so computing one would be work done to throw away.
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

/// The 24h window, or `None` when it is too sparse for this tier to see.
async fn collect_window(pool: &PgPool, tier: Tier) -> Result<Option<PulseWindowOut>, sqlx::Error> {
    let since = Utc::now() - chrono::Duration::hours(WINDOW_HOURS);
    let exact = tier == Tier::Admin;

    let window = repo::get_pulse_window(pool, since).await?;

    // Suppression before any further query: a window this empty is not going to
    // be rendered, so the model mix and blocklist are work for nothing.
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

/// The operator block, assembled from the repositories that outlived the admin
/// pages. Nothing here is new SQL — these are the same queries the retired
/// dashboard ran and the CLI still runs.
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

impl From<TopUser> for TopUserOut {
    fn from(u: TopUser) -> Self {
        Self {
            user_id: u.user_id,
            display_name: u.display_name,
            logins: u.logins,
            edits: u.edits,
            mcp_calls: u.mcp_calls,
            last_active: u.last_active,
        }
    }
}

/// A count rendered for the caller's tier.
fn count(n: i64, exact: bool) -> String {
    if exact {
        n.to_string()
    } else {
        normalize::bucket(n)
    }
}

/// A token count rendered for the caller's tier, on its own larger scale.
fn tokens(n: i64, exact: bool) -> String {
    if exact {
        n.to_string()
    } else {
        normalize::bucket_tokens(n)
    }
}
