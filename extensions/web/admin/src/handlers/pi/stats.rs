//! What the agent just cost, and what policy let through.
//!
//! The homepage pane shows a user their own governance spine while the terminal
//! beside it is still running. That data already exists — `ai_requests` for
//! spend and latency, `governance_decisions` for verdicts, `plugin_usage_events`
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
    events: Vec<PiStatEvent>,
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
        // The same answer a stranger gets for a conversation that never
        // existed: whose session this is, is not something to confirm.
        return problem(StatusCode::NOT_FOUND, "no such conversation");
    };

    match collect(&pool, &conversation_id, &session.attested_session).await {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "could not read pi session stats");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "could not read stats") // lint-ok: http-error — logged above; the client is told nothing about why
        },
    }
}

async fn collect(
    pool: &PgPool,
    conversation_id: &str,
    attested: &SessionId,
) -> Result<PiStats, sqlx::Error> {
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
