//! Session-detail repository.
//!
//! A session groups every AI request produced by a single interactive run.
//! This module serves the KPI rollup and the request list for one session.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use systemprompt::identifiers::{ContextId, SessionId, TraceId};

#[derive(Debug, Clone, Copy)]
pub struct SessionKpis {
    pub request_count: i64,
    pub context_count: i64,
    pub trace_count: i64,
    pub error_count: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cost_microdollars: i64,
}

#[derive(Debug, Clone)]
pub struct SessionRequestRow {
    pub id: String,
    pub context_id: Option<ContextId>,
    pub trace_id: Option<TraceId>,
    pub model: String,
    pub status: String,
    pub latency_ms: Option<i32>,
    pub cost_microdollars: i64,
    pub created_at: DateTime<Utc>,
}

pub async fn get_session_kpis(
    pool: &PgPool,
    session_id: &SessionId,
) -> Result<SessionKpis, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT
            COUNT(*)::bigint                                   AS "request_count!",
            COUNT(DISTINCT context_id)::bigint                 AS "context_count!",
            COUNT(DISTINCT trace_id)::bigint                   AS "trace_count!",
            COUNT(*) FILTER (WHERE status = 'failed')::bigint  AS "error_count!",
            COALESCE(SUM(input_tokens), 0)::bigint             AS "total_input_tokens!",
            COALESCE(SUM(output_tokens), 0)::bigint            AS "total_output_tokens!",
            COALESCE(SUM(cost_microdollars), 0)::bigint        AS "total_cost_microdollars!"
        FROM ai_requests
        WHERE session_id = $1
        "#,
        session_id.as_str()
    )
    .fetch_one(pool)
    .await?;
    Ok(SessionKpis {
        request_count: row.request_count,
        context_count: row.context_count,
        trace_count: row.trace_count,
        error_count: row.error_count,
        total_input_tokens: row.total_input_tokens,
        total_output_tokens: row.total_output_tokens,
        total_cost_microdollars: row.total_cost_microdollars,
    })
}

pub async fn list_session_requests(
    pool: &PgPool,
    session_id: &SessionId,
) -> Result<Vec<SessionRequestRow>, sqlx::Error> {
    sqlx::query_as!(
        SessionRequestRow,
        r#"
        SELECT
            id                                  AS "id!",
            context_id                          AS "context_id?: ContextId",
            trace_id                            AS "trace_id?: TraceId",
            model                               AS "model!",
            status                              AS "status!",
            latency_ms                          AS "latency_ms?",
            cost_microdollars                   AS "cost_microdollars!",
            created_at                          AS "created_at!"
        FROM ai_requests
        WHERE session_id = $1
        ORDER BY created_at DESC
        LIMIT 200
        "#,
        session_id.as_str()
    )
    .fetch_all(pool)
    .await
}
