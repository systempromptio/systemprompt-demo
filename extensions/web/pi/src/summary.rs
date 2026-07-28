//! What this user has been doing across every conversation.
//!
//! One page of per-conversation tallies plus an all-time rollup and the
//! user's most-used tools — the Activity tab's whole payload. Scoped to the
//! embed token's user; nothing here can read across accounts.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use systemprompt::identifiers::ContextId;

use super::auth::{authenticate, problem, unauthorized};
use super::format;
use systemprompt_web_governance::repositories::analytics::user_summary;

const CONVERSATION_LIMIT: i64 = 20;
const TOOL_LIMIT: i64 = 8;

#[derive(Debug, Deserialize)]
pub(super) struct SummaryQuery {
    token: String,
}

#[derive(Debug, Serialize)]
struct ConversationView {
    id: ContextId,
    title: Option<String>,
    requests: i64,
    errors: i64,
    denied: i64,
    tool_calls: i64,
    cost_microdollars: i64,
    cost_display: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
struct Totals {
    conversations: i64,
    requests: i64,
    errors: i64,
    denied: i64,
    tool_calls: i64,
    cost_display: String,
}

#[derive(Debug, Serialize)]
struct ToolView {
    tool: String,
    calls: i64,
}

#[derive(Debug, Serialize)]
struct SummaryBody {
    totals: Totals,
    conversations: Vec<ConversationView>,
    top_tools: Vec<ToolView>,
}

pub(super) async fn me_summary(
    State(pool): State<Arc<PgPool>>,
    Query(q): Query<SummaryQuery>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(user_id) = authenticate(&pool, &q.token).await else {
        return unauthorized();
    };
    let kpis = user_summary::list_user_conversation_kpis(&pool, &user_id, CONVERSATION_LIMIT).await;
    let tools = user_summary::list_user_tool_usage(&pool, &user_id, TOOL_LIMIT).await;
    let (Ok(kpis), Ok(tools)) = (kpis, tools) else {
        tracing::error!("could not read a pi user activity summary");
        return problem(
            StatusCode::INTERNAL_SERVER_ERROR, /* lint-ok: http-error — logged above; the
                                                * client is told nothing about why */
            "could not read summary",
        );
    };

    let mut totals = Totals {
        conversations: i64::try_from(kpis.len()).unwrap_or(i64::MAX),
        requests: 0,
        errors: 0,
        denied: 0,
        tool_calls: 0,
        cost_display: String::new(),
    };
    let mut cost = 0i64;
    let conversations: Vec<ConversationView> = kpis
        .into_iter()
        .map(|k| {
            totals.requests += k.requests;
            totals.errors += k.errors;
            totals.denied += k.denied;
            totals.tool_calls += k.tool_calls;
            cost += k.cost_microdollars;
            ConversationView {
                id: k.id,
                title: k.title,
                requests: k.requests,
                errors: k.errors,
                denied: k.denied,
                tool_calls: k.tool_calls,
                cost_microdollars: k.cost_microdollars,
                cost_display: format::cost(k.cost_microdollars),
                created_at: k.created_at,
                updated_at: k.updated_at,
            }
        })
        .collect();
    totals.cost_display = format::cost(cost);

    Json(SummaryBody {
        totals,
        conversations,
        top_tools: tools
            .into_iter()
            .map(|t| ToolView {
                tool: t.tool,
                calls: t.calls,
            })
            .collect(),
    })
    .into_response()
}
