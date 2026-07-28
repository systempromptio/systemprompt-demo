//! Conversation-detail repository.
//!
//! A conversation may span many attested sessions — every resume binds a fresh
//! one — so each query here joins through `pi_conversation_sessions` rather
//! than keying on a single session id. That join is what makes the numbers
//! survive a reload: the rows written before a resume keep their old session
//! id, and the binding history is the only path back to them.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use systemprompt::identifiers::{ContextId, TraceId};

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
    /// Absent when the request was refused before routing resolved a model —
    /// the `rejected` status. That row is the whole point of a governance
    /// pane, so it must survive the read rather than be asserted away.
    pub model: Option<String>,
    /// What the client asked for. Differs from `model` whenever a gateway route
    /// rewrote it, which is the moment worth showing. Outlives `model`: a
    /// rejected request still records what was asked for.
    pub requested_model: Option<String>,
    /// Absent for the same reason as `model`.
    pub provider: Option<String>,
    pub route_match: Option<String>,
    pub status: String,
    pub latency_ms: Option<i32>,
    pub cost_microdollars: i64,
    pub cache_hit: bool,
    /// Populated on `failed` rows; the drilldown the error count summarizes.
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// One time bucket of request activity — enough to draw latency and error
/// rate over the conversation's life without shipping every row.
#[derive(Debug, Clone, Copy)]
pub struct RequestBucket {
    pub at: DateTime<Utc>,
    pub requests: i64,
    pub errors: i64,
    pub latency_p50_ms: Option<f64>,
    pub latency_p95_ms: Option<f64>,
    pub cost_microdollars: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

pub async fn get_conversation_kpis(
    pool: &PgPool,
    conversation_id: &ContextId,
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
        WHERE session_id IN (
            SELECT session_id FROM pi_conversation_sessions WHERE conversation_id = $1
        )
        "#,
        conversation_id.as_str()
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

pub async fn list_conversation_requests(
    pool: &PgPool,
    conversation_id: &ContextId,
) -> Result<Vec<SessionRequestRow>, sqlx::Error> {
    sqlx::query_as!(
        SessionRequestRow,
        r#"
        SELECT
            id                                  AS "id!",
            context_id                          AS "context_id?: ContextId",
            trace_id                            AS "trace_id?: TraceId",
            model                               AS "model?",
            requested_model                     AS "requested_model?",
            provider                            AS "provider?",
            route_match                         AS "route_match?",
            status                              AS "status!",
            latency_ms                          AS "latency_ms?",
            cost_microdollars                   AS "cost_microdollars!",
            cache_hit                           AS "cache_hit!",
            NULLIF(error_message, '')           AS "error_message?",
            created_at                          AS "created_at!"
        FROM ai_requests
        WHERE session_id IN (
            SELECT session_id FROM pi_conversation_sessions WHERE conversation_id = $1
        )
        ORDER BY created_at DESC
        LIMIT 200
        "#,
        conversation_id.as_str()
    )
    .fetch_all(pool)
    .await
}

/// Requests folded into fixed-width time buckets, oldest first.
///
/// `bucket_secs` is clamped by the caller; percentiles come from
/// `percentile_cont` so a bucket with one row reports that row rather than an
/// interpolation artifact.
pub async fn list_conversation_request_buckets(
    pool: &PgPool,
    conversation_id: &ContextId,
    bucket_secs: i64,
) -> Result<Vec<RequestBucket>, sqlx::Error> {
    sqlx::query_as!(
        RequestBucket,
        r#"
        SELECT
            date_bin(make_interval(secs => $2::double precision),
                     created_at, TIMESTAMPTZ '2001-01-01') AS "at!",
            COUNT(*)::bigint                                  AS "requests!",
            COUNT(*) FILTER (WHERE status = 'failed')::bigint AS "errors!",
            percentile_cont(0.5) WITHIN GROUP (ORDER BY latency_ms)  AS "latency_p50_ms?",
            percentile_cont(0.95) WITHIN GROUP (ORDER BY latency_ms) AS "latency_p95_ms?",
            COALESCE(SUM(cost_microdollars), 0)::bigint       AS "cost_microdollars!",
            COALESCE(SUM(input_tokens), 0)::bigint            AS "input_tokens!",
            COALESCE(SUM(output_tokens), 0)::bigint           AS "output_tokens!"
        FROM ai_requests
        WHERE session_id IN (
            SELECT session_id FROM pi_conversation_sessions WHERE conversation_id = $1
        )
        GROUP BY 1
        ORDER BY 1
        LIMIT 200
        "#,
        conversation_id.as_str(),
        bucket_secs as f64
    )
    .fetch_all(pool)
    .await
}
