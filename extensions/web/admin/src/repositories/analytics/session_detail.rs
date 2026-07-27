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
    /// Prompt tokens the provider served from its own cache rather than
    /// re-reading. Billed at a fraction of a fresh input token, so a pane that
    /// shows spend without showing this cannot explain why the two disagree.
    pub total_cache_read_tokens: i64,
    pub total_cache_creation_tokens: i64,
    pub cache_hit_count: i64,
}

#[derive(Debug, Clone)]
pub struct SessionRequestRow {
    pub id: String,
    pub context_id: Option<ContextId>,
    pub trace_id: Option<TraceId>,
    pub model: String,
    /// What the client asked for. Differs from `model` whenever a gateway route
    /// rewrote it, which is the moment worth showing.
    pub requested_model: Option<String>,
    pub provider: String,
    pub route_match: Option<String>,
    pub status: String,
    pub latency_ms: Option<i32>,
    pub cost_microdollars: i64,
    pub cache_hit: bool,
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
            COALESCE(SUM(cost_microdollars), 0)::bigint        AS "total_cost_microdollars!",
            COALESCE(SUM(cache_read_tokens), 0)::bigint        AS "total_cache_read_tokens!",
            COALESCE(SUM(cache_creation_tokens), 0)::bigint    AS "total_cache_creation_tokens!",
            COUNT(*) FILTER (WHERE cache_hit)::bigint          AS "cache_hit_count!"
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
        total_cache_read_tokens: row.total_cache_read_tokens,
        total_cache_creation_tokens: row.total_cache_creation_tokens,
        cache_hit_count: row.cache_hit_count,
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
            requested_model                     AS "requested_model?",
            provider                            AS "provider!",
            route_match                         AS "route_match?",
            status                              AS "status!",
            latency_ms                          AS "latency_ms?",
            cost_microdollars                   AS "cost_microdollars!",
            cache_hit                           AS "cache_hit!",
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
