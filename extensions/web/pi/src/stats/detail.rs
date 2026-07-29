//! The drilldowns behind the headline stats: every request, and the same
//! requests folded into time buckets a chart can draw.
//!
//! Same authority model as the rollup — the embed token resolves to one user,
//! the conversation must be theirs, and every refusal is the same opaque 404.
//!
//! `scope=all` (the default) widens the read past the conversation in the path
//! to every conversation the caller owns, matching the pane's default view.
//! The path conversation still carries the authority either way.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use systemprompt::identifiers::ContextId;

use super::super::auth::{authorize_conversation, problem};
use super::super::format;
use systemprompt::identifiers::UserId;
use systemprompt_web_governance::repositories::analytics::session_detail;
use systemprompt_web_governance::repositories::scope::StatsScope;

const MIN_BUCKET_SECS: i64 = 10;
const MAX_BUCKET_SECS: i64 = 3600;
const DEFAULT_BUCKET_SECS: i64 = 60;

fn scoped<'a>(
    wanted: Option<&str>,
    user_id: &'a UserId,
    conversation_id: &'a ContextId,
) -> StatsScope<'a> {
    if wanted == Some("current") {
        StatsScope::conversation(user_id, conversation_id)
    } else {
        StatsScope::all(user_id)
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RequestsQuery {
    token: String,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Serialize)]
struct RequestView {
    id: String,
    trace_id: Option<String>,
    model: Option<String>,
    requested_model: Option<String>,
    provider: Option<String>,
    route_match: Option<String>,
    status: String,
    latency_ms: Option<i32>,
    cost_display: String,
    cache_hit: bool,
    error_message: Option<String>,
    at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
struct RequestsBody {
    conversation_id: ContextId,
    requests: Vec<RequestView>,
}

pub(crate) async fn requests(
    State(pool): State<Arc<PgPool>>,
    Path(conversation_id): Path<ContextId>,
    Query(q): Query<RequestsQuery>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(row) = authorize_conversation(&pool, &q.token, &conversation_id).await else {
        return problem(StatusCode::NOT_FOUND, "no such conversation");
    };
    let scope = scoped(q.scope.as_deref(), &row.user_id, &conversation_id);
    match session_detail::list_scoped_requests(&pool, scope).await {
        Ok(rows) => Json(RequestsBody {
            conversation_id,
            requests: rows
                .into_iter()
                .map(|r| RequestView {
                    id: r.id,
                    trace_id: r.trace_id.map(|t| t.to_string()),
                    model: r.model,
                    requested_model: r.requested_model,
                    provider: r.provider,
                    route_match: r.route_match,
                    status: r.status,
                    latency_ms: r.latency_ms,
                    cost_display: format::cost(r.cost_microdollars),
                    cache_hit: r.cache_hit,
                    error_message: r.error_message,
                    at: r.created_at,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "could not list pi conversation requests");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR, /* lint-ok: http-error — logged above; the
                                                    * client is told nothing about why */
                "could not read requests",
            )
        },
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct TimeseriesQuery {
    token: String,
    #[serde(default)]
    bucket: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Serialize)]
struct BucketView {
    at: chrono::DateTime<chrono::Utc>,
    requests: i64,
    errors: i64,
    latency_p50_ms: Option<i64>,
    latency_p95_ms: Option<i64>,
    cost_microdollars: i64,
    input_tokens: i64,
    output_tokens: i64,
}

#[derive(Debug, Serialize)]
struct TimeseriesBody {
    conversation_id: ContextId,
    bucket_secs: i64,
    buckets: Vec<BucketView>,
}

pub(crate) async fn timeseries(
    State(pool): State<Arc<PgPool>>,
    Path(conversation_id): Path<ContextId>,
    Query(q): Query<TimeseriesQuery>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(row) = authorize_conversation(&pool, &q.token, &conversation_id).await else {
        return problem(StatusCode::NOT_FOUND, "no such conversation");
    };
    let bucket_secs = q
        .bucket
        .unwrap_or(DEFAULT_BUCKET_SECS)
        .clamp(MIN_BUCKET_SECS, MAX_BUCKET_SECS);
    let scope = scoped(q.scope.as_deref(), &row.user_id, &conversation_id);
    match session_detail::list_scoped_request_buckets(&pool, scope, bucket_secs).await {
        Ok(rows) => Json(TimeseriesBody {
            conversation_id,
            bucket_secs,
            buckets: rows
                .into_iter()
                .map(|b| BucketView {
                    at: b.at,
                    requests: b.requests,
                    errors: b.errors,
                    latency_p50_ms: b.latency_p50_ms.map(|v| v.round() as i64),
                    latency_p95_ms: b.latency_p95_ms.map(|v| v.round() as i64),
                    cost_microdollars: b.cost_microdollars,
                    input_tokens: b.input_tokens,
                    output_tokens: b.output_tokens,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "could not bucket pi conversation requests");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR, /* lint-ok: http-error — logged above; the
                                                    * client is told nothing about why */
                "could not read timeseries",
            )
        },
    }
}
