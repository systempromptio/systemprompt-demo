//! What the agent just cost, and what policy let through.
//!
//! The homepage pane shows a user their own governance spine while the terminal
//! beside it is still running. That data already exists — `ai_requests` for
//! spend and latency, `governance_decisions` for verdicts, `user_activity`
//! for tool fires — so this endpoint composes the same repository functions the
//! retired admin pages used rather than adding queries.
//!
//! It is deliberately *not* an admin route. The interactive site has no admin
//! concept: authority here comes from the embed token, which resolves to
//! exactly one user, and the conversation must belong to that user. A caller
//! therefore reads their own session and nobody else's, with the same opaque
//! 404 for "not yours" as for "does not exist".
//!
//! The ownership check reads `pi_conversations`, not the live-session registry.
//! Every number below is a database query, and so is the attested session id
//! they key on — which is what makes these stats survive a reload and a server
//! restart. Resolving that id from memory instead would make a conversation
//! uncostable the moment its child exited, with every row explaining it still
//! sitting in Postgres.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use sqlx::PgPool;
use systemprompt::identifiers::{ContextId, SessionId};

mod facets;
mod push;

use facets::{Facets, credit_position, facets, model_mix, policy_stages, trace_counts};
pub(super) use push::{push_soon, snapshot};
use push::{cached_body, store_body};

use super::auth::{authorize_conversation, problem};
use super::format;
use super::watch::TokenQuery;
use crate::repositories::analytics::session_detail;
use crate::repositories::governance::{demo_trace, stages};

const TRACE_LIMIT: i64 = 120;

fn json_body(body: String) -> Response {
    // lint-ok: http-error — a widget-facing endpoint answers in its own shape
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

#[derive(Debug, Serialize)]
struct PiStats {
    conversation_id: ContextId,
    model: Option<String>,
    requested_model: Option<String>,
    provider: Option<String>,
    route_match: Option<String>,
    requests: i64,
    errors: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    cache_hit_percent: i64,
    cost_microdollars: i64,
    cost_display: String,
    cost_per_request_display: String,
    latency_p50_ms: Option<i32>,
    latency_p95_ms: Option<i32>,
    latency_last_ms: Option<i32>,
    allowed: i64,
    denied: i64,
    prompts_blocked: i64,
    tools_blocked: i64,
    tool_calls: i64,
    secrets_caught: i64,
    policy_stages: Vec<PiPolicyStage>,
    model_mix: Vec<PiModelShare>,
    credit: PiCredit,
    events: Vec<PiStatEvent>,
}

#[derive(Debug, Serialize)]
struct PiPolicyStage {
    id: String,
    label: String,
    passed: i64,
    failed: i64,
    active: bool,
}

#[derive(Debug, Serialize)]
struct PiModelShare {
    model: String,
    requests: i64,
    percent: i64,
}

#[derive(Debug, Serialize)]
struct PiCredit {
    granted_microdollars: i64,
    spent_microdollars: i64,
    remaining_microdollars: i64,
    granted_display: String,
    spent_display: String,
    remaining_display: String,
    remaining_percent: i64,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    approver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    approver_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    approver_at: Option<String>,
}

fn approver_field(rules: Option<&serde_json::Value>, key: &str) -> Option<String> {
    rules?
        .get("approver")?
        .get(key)?
        .as_str()
        .map(str::to_owned)
}

pub(super) async fn stats(
    State(pool): State<Arc<PgPool>>,
    Path(conversation_id): Path<ContextId>,
    Query(q): Query<TokenQuery>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(row) = authorize_conversation(&pool, q.token(), &conversation_id).await else {
        return problem(StatusCode::NOT_FOUND, "no such conversation");
    };

    if let Some(body) = cached_body(&conversation_id) {
        return json_body(body);
    }

    match collect(
        &pool,
        &conversation_id,
        &row.attested_session_id,
        &row.user_id,
    )
    .await
    {
        Ok(stats) => serde_json::to_string(&stats).map_or_else(
            |_| Json(&stats).into_response(),
            |body| {
                store_body(&conversation_id, body.clone());
                json_body(body)
            },
        ),
        Err(e) => {
            tracing::error!(error = %e, "could not read pi session stats");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "could not read stats") // lint-ok: http-error — logged above; the client is told nothing about why
        },
    }
}

async fn collect(
    pool: &PgPool,
    conversation_id: &ContextId,
    attested: &SessionId,
    user_id: &systemprompt::identifiers::UserId,
) -> Result<PiStats, sqlx::Error> {
    let credit = credit_position(pool, user_id).await;
    let kpis = session_detail::get_session_kpis(pool, attested).await?;
    let requests = session_detail::list_session_requests(pool, attested).await?;
    let trace = demo_trace::list_demo_trace(pool, attested, TRACE_LIMIT).await?;
    let counts = stages::get_session_governance_counts(pool, attested).await?;
    let stage_rows = stages::list_session_policy_stages(pool, attested).await?;

    let Facets {
        latency_last_ms,
        latency_p50_ms,
        latency_p95_ms,
        model,
        provider,
        route_match,
        requested_model,
    } = facets(&requests);

    let cache_hit_percent = if kpis.request_count > 0 {
        kpis.cache_hit_count.saturating_mul(100) / kpis.request_count
    } else {
        0
    };
    let cost_per_request_display = if kpis.request_count > 0 {
        format::cost(kpis.total_cost_microdollars / kpis.request_count)
    } else {
        format::cost(0)
    };

    let counts_from_trace = trace_counts(&trace);

    Ok(PiStats {
        conversation_id: conversation_id.clone(),
        model,
        requested_model,
        provider,
        route_match,
        requests: kpis.request_count,
        errors: kpis.error_count,
        input_tokens: kpis.total_input_tokens,
        output_tokens: kpis.total_output_tokens,
        cache_read_tokens: kpis.total_cache_read_tokens,
        cache_creation_tokens: kpis.total_cache_creation_tokens,
        cache_hit_percent,
        cost_microdollars: kpis.total_cost_microdollars,
        cost_display: format::cost(kpis.total_cost_microdollars),
        cost_per_request_display,
        latency_p50_ms,
        latency_p95_ms,
        latency_last_ms,
        secrets_caught: counts.secret_breaches,
        policy_stages: policy_stages(&stage_rows),
        model_mix: model_mix(&requests),
        allowed: counts_from_trace.allowed,
        denied: counts_from_trace.denied,
        prompts_blocked: counts_from_trace.prompts_blocked,
        tools_blocked: counts_from_trace.tools_blocked,
        tool_calls: counts_from_trace.tool_calls,
        credit,
        events: trace
            .into_iter()
            .map(|r| PiStatEvent {
                approver: approver_field(r.evaluated_rules.as_ref(), "username"),
                approver_action: approver_field(r.evaluated_rules.as_ref(), "action"),
                approver_at: approver_field(r.evaluated_rules.as_ref(), "decided_at"),
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
