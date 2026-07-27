//! What the agent just cost, and what policy let through.
//!
//! The homepage pane shows a user their own governance spine while the terminal
//! beside it is still running. That data already exists — `ai_requests` for
//! spend and latency, `governance_decisions` for verdicts, `user_activity`
//! for tool fires — so this endpoint composes the same repository functions the
//! retired admin pages used rather than adding queries.
//!
//! It is deliberately *not* an admin route. The interactive site has no admin
//! concept: authority here comes from the embed token, which resolves to exactly
//! one user, and the conversation must belong to that user. A caller therefore
//! reads their own session and nobody else's, with the same opaque 404 for "not
//! yours" as for "does not exist".

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Serialize;
use sqlx::PgPool;
use systemprompt::identifiers::SessionId;

use super::api::TokenQuery;
use super::auth::{authorize_session, problem};
use super::format;
use super::registry::PiRegistry;
use crate::repositories::analytics::session_detail;
use crate::repositories::governance::demo_trace;

/// Trace rows returned to the pane. Enough to render a feed, capped so a long
/// session cannot turn a 3-second poll into a large response.
const TRACE_LIMIT: i64 = 120;

#[derive(Debug, Serialize)]
struct PiStats {
    conversation_id: String,
    /// Newest model the session actually reached, absent until the first call.
    model: Option<String>,
    requests: i64,
    errors: i64,
    input_tokens: i64,
    output_tokens: i64,
    cost_microdollars: i64,
    cost_display: String,
    latency_p50_ms: Option<i32>,
    latency_last_ms: Option<i32>,
    allowed: i64,
    denied: i64,
    prompts_blocked: i64,
    tools_blocked: i64,
    tool_calls: i64,
    /// What this identity has left to spend, and of how much.
    credit: PiCredit,
    events: Vec<PiStatEvent>,
}

/// The visitor's credit position.
///
/// Lifetime, not per-session: `cost_*` above is what this conversation spent,
/// and the two answer different questions. "This turn cost $0.004" is
/// interesting; "you have $4.83 of your $5 left" is the one that tells someone
/// whether to keep going, and it is the number the signup promise is about.
#[derive(Debug, Serialize)]
struct PiCredit {
    granted_microdollars: i64,
    spent_microdollars: i64,
    /// Granted minus spent. Can go negative: a request's cost lands after the
    /// guard has already refused the next one, so the last call overshoots.
    remaining_microdollars: i64,
    granted_display: String,
    spent_display: String,
    remaining_display: String,
    /// Whole percent of the grant still unspent, clamped to 0–100 so a meter
    /// can bind to it directly and an overshoot renders as empty, not inverted.
    remaining_percent: i64,
    /// True once the balance is gone. The gateway refuses the next request at
    /// this point, so the pane says so before the terminal has to.
    exhausted: bool,
}

#[derive(Debug, Serialize)]
struct PiStatEvent {
    id: String,
    at: chrono::DateTime<chrono::Utc>,
    kind: String,
    subject: String,
    outcome: String,
    policy: String,
    detail: String,
}

/// `GET /api/public/pi/stats/{conversation_id}?token=…`
pub(super) async fn stats(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Path(conversation_id): Path<String>,
    Query(q): Query<TokenQuery>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(session) = authorize_session(&pool, &registry, q.token(), &conversation_id).await
    else {
        return problem(StatusCode::NOT_FOUND, "no such conversation");
    };

    match collect(
        &pool,
        &conversation_id,
        &session.attested_session,
        &session.user_id,
    )
    .await
    {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "could not read pi session stats");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "could not read stats") // lint-ok: http-error — logged above; the client is told nothing about why
        },
    }
}

/// Read the credit position, or fall back to a zeroed one.
///
/// A credit-ledger failure must not take the whole pane down: the governance
/// feed beside it is the thing a visitor came to see, and it is still correct
/// when the balance is not.
async fn credit_position(pool: &PgPool, user_id: &systemprompt::identifiers::UserId) -> PiCredit {
    let balance = systemprompt_credits_extension::get_balance(pool, user_id.as_str())
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "could not read a credit balance for the pi stats pane");
            systemprompt_credits_extension::CreditBalance {
                balance_microdollars: 0,
                granted_microdollars: 0,
                spent_microdollars: 0,
            }
        });

    let remaining_percent = if balance.granted_microdollars > 0 {
        let pct = balance.balance_microdollars.saturating_mul(100) / balance.granted_microdollars;
        pct.clamp(0, 100)
    } else {
        0
    };

    PiCredit {
        granted_microdollars: balance.granted_microdollars,
        spent_microdollars: balance.spent_microdollars,
        remaining_microdollars: balance.balance_microdollars,
        granted_display: format::cost_round(balance.granted_microdollars),
        spent_display: format::cost(balance.spent_microdollars),
        // A negative remainder reads as debt the visitor owes, which is not
        // what an overshot demo grant means. Floor the display at zero and let
        // `exhausted` carry the fact.
        remaining_display: format::cost(balance.balance_microdollars.max(0)),
        remaining_percent,
        exhausted: balance.granted_microdollars > 0 && balance.balance_microdollars <= 0,
    }
}

async fn collect(
    pool: &PgPool,
    conversation_id: &str,
    attested: &SessionId,
    user_id: &systemprompt::identifiers::UserId,
) -> Result<PiStats, sqlx::Error> {
    let credit = credit_position(pool, user_id).await;
    let kpis = session_detail::get_session_kpis(pool, attested).await?;
    let requests = session_detail::list_session_requests(pool, attested).await?;
    let trace = demo_trace::list_demo_trace(pool, attested, TRACE_LIMIT).await?;

    // `list_session_requests` is newest-first, so the head is the latest turn.
    let latency_last_ms = requests.first().and_then(|r| r.latency_ms);
    let latency_p50_ms = format::median(requests.iter().filter_map(|r| r.latency_ms).collect());
    let model = requests.first().map(|r| r.model.clone());

    let counted = |kind: &str, outcome: &str| {
        trace
            .iter()
            .filter(|r| r.kind == kind && r.outcome == outcome)
            .count() as i64
    };

    Ok(PiStats {
        conversation_id: conversation_id.to_owned(),
        model,
        requests: kpis.request_count,
        errors: kpis.error_count,
        input_tokens: kpis.total_input_tokens,
        output_tokens: kpis.total_output_tokens,
        cost_microdollars: kpis.total_cost_microdollars,
        cost_display: format::cost(kpis.total_cost_microdollars),
        latency_p50_ms,
        latency_last_ms,
        allowed: trace.iter().filter(|r| r.outcome == "allow").count() as i64,
        denied: trace.iter().filter(|r| r.outcome == "deny").count() as i64,
        prompts_blocked: counted("prompt", "deny"),
        tools_blocked: counted("tool", "deny"),
        tool_calls: trace.iter().filter(|r| r.kind == "fire").count() as i64,
        credit,
        events: trace
            .into_iter()
            .map(|r| PiStatEvent {
                id: r.id,
                at: r.at,
                kind: r.kind,
                subject: r.subject,
                outcome: r.outcome,
                policy: r.policy,
                detail: r.detail,
            })
            .collect(),
    })
}
