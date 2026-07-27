//! Reading the governance spine back out for one caller.
//!
//! The admin site has rich views over these tables, but they live in a
//! different crate and are reached over an authenticated web session. A client
//! connected to this hub has neither, so `governance_stats` asks the same three
//! tables directly:
//!
//! * `governance_decisions` — what was asked for, and whether policy allowed it
//! * `ai_requests` — what reached a provider, and what it cost
//! * `user_activity` — which tool calls actually ran
//!
//! Every query is scoped by **both** `user_id` and `session_id`, and the two do
//! different jobs. `user_id` is the security boundary: the tool takes no
//! arguments, so there is no selector a caller could widen to read somebody
//! else's rows. `session_id` is the honesty boundary: the tool is presented as
//! a readout of *this* session, and scoping by user alone made it a lifetime
//! total instead — a viewer comparing the numbers against the handful of calls
//! they just watched would find they matched nothing.
//!
//! Tool fires come from `user_activity`, which is where `record_mcp_access`
//! writes them. `plugin_usage_events` only ever carries marketplace-webhook
//! rows, so reading it here reported "no tools ran" for every session that ever
//! ran a tool.

use sqlx::PgPool;
use systemprompt::identifiers::{SessionId, UserId};

/// How many recent decisions the tool returns. A demo session produces a
/// handful; a long-lived account produces thousands, and a tool result is read
/// by a model with a context window.
pub(crate) const DECISION_LIMIT: i64 = 40;

/// One policy's tally over the caller's history.
#[derive(Debug)]
pub(crate) struct PolicyTally {
    pub(crate) policy: String,
    pub(crate) allowed: i64,
    pub(crate) denied: i64,
}

/// One decision, newest first.
#[derive(Debug)]
pub(crate) struct DecisionRow {
    pub(crate) at: chrono::DateTime<chrono::Utc>,
    pub(crate) tool_name: String,
    pub(crate) decision: String,
    pub(crate) policy: String,
    pub(crate) reason: String,
}

/// Provider spend for the caller.
#[derive(Debug)]
pub(crate) struct SpendRow {
    pub(crate) requests: i64,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) cost_microdollars: i64,
    /// Mean latency in milliseconds, absent when no request has completed.
    pub(crate) mean_latency_ms: Option<f64>,
    /// Newest model actually reached.
    pub(crate) model: Option<String>,
}

/// One tool that ran to completion, with how often.
#[derive(Debug)]
pub(crate) struct ToolFireRow {
    pub(crate) tool_name: String,
    pub(crate) fires: i64,
}

pub(crate) async fn list_policy_tallies(
    pool: &PgPool,
    user_id: &UserId,
    session_id: &SessionId,
) -> Result<Vec<PolicyTally>, sqlx::Error> {
    sqlx::query_as!(
        PolicyTally,
        r#"SELECT COALESCE(NULLIF(policy, ''), 'default_included') as "policy!",
                  COUNT(*) FILTER (WHERE decision = 'allow') as "allowed!",
                  COUNT(*) FILTER (WHERE decision = 'deny')  as "denied!"
           FROM governance_decisions
           WHERE user_id = $1 AND session_id = $2
           GROUP BY 1
           ORDER BY 3 DESC, 2 DESC"#,
        user_id.as_str(),
        session_id.as_str(),
    )
    .fetch_all(pool)
    .await
}

pub(crate) async fn list_recent_decisions(
    pool: &PgPool,
    user_id: &UserId,
    session_id: &SessionId,
    limit: i64,
) -> Result<Vec<DecisionRow>, sqlx::Error> {
    sqlx::query_as!(
        DecisionRow,
        r#"SELECT created_at as "at!", tool_name as "tool_name!",
                  decision as "decision!",
                  COALESCE(NULLIF(policy, ''), 'default_included') as "policy!",
                  COALESCE(reason, '') as "reason!"
           FROM governance_decisions
           WHERE user_id = $1 AND session_id = $2
           ORDER BY created_at DESC
           LIMIT $3"#,
        user_id.as_str(),
        session_id.as_str(),
        limit,
    )
    .fetch_all(pool)
    .await
}

/// Spend is always one row: the aggregate collapses an empty history to zeros
/// rather than to no row, so the caller never has to distinguish "no requests"
/// from "query returned nothing".
pub(crate) async fn get_spend(
    pool: &PgPool,
    user_id: &UserId,
    session_id: &SessionId,
) -> Result<SpendRow, sqlx::Error> {
    sqlx::query_as!(
        SpendRow,
        r#"SELECT COUNT(*) as "requests!",
                  COALESCE(SUM(input_tokens), 0)::BIGINT as "input_tokens!",
                  COALESCE(SUM(output_tokens), 0)::BIGINT as "output_tokens!",
                  COALESCE(SUM(cost_microdollars), 0)::BIGINT as "cost_microdollars!",
                  AVG(latency_ms)::FLOAT8 as "mean_latency_ms?",
                  (SELECT COALESCE(r.requested_model, r.model) FROM ai_requests r
                   WHERE r.user_id = $1 AND r.session_id = $2
                   ORDER BY r.created_at DESC LIMIT 1) as "model?"
           FROM ai_requests
           WHERE user_id = $1 AND session_id = $2"#,
        user_id.as_str(),
        session_id.as_str(),
    )
    .fetch_one(pool)
    .await
}

/// Tool calls that actually executed, from `user_activity`.
///
/// `record_mcp_access` stamps the session into `metadata` rather than a column:
/// `user_activity` is core-owned, and an index is a safer thing for this repo to
/// add to it than a column. Rows written before that stamp existed have no
/// `session_id` and correctly fall outside every session's scope.
pub(crate) async fn list_tool_fires(
    pool: &PgPool,
    user_id: &UserId,
    session_id: &SessionId,
    limit: i64,
) -> Result<Vec<ToolFireRow>, sqlx::Error> {
    sqlx::query_as!(
        ToolFireRow,
        r#"SELECT COALESCE(NULLIF(entity_name, ''), 'unknown') as "tool_name!",
                  COUNT(*) as "fires!"
           FROM user_activity
           WHERE user_id = $1
             AND category = 'mcp_access'
             AND action = 'used'
             AND metadata->>'session_id' = $2
           GROUP BY 1
           ORDER BY 2 DESC
           LIMIT $3"#,
        user_id.as_str(),
        session_id.as_str(),
        limit,
    )
    .fetch_all(pool)
    .await
}
