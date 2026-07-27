//! The demo trace: one ordered story per agent session.
//!
//! Three tables record what a governed coding agent did, and each answers a
//! different question:
//!
//! * `governance_decisions` — what was asked for, and whether policy allowed it
//! * `ai_requests` — what actually reached a provider, and what it cost
//! * `user_activity` — which tool calls ran to completion
//!
//! Read separately they are three lists. Read as one time-ordered union they
//! are the demo: a prompt denied by `secret_scan` sits immediately above the
//! `ai_requests` row that never happened, which is the whole point.

use sqlx::PgPool;
use systemprompt::identifiers::SessionId;

#[derive(Debug, Clone)]
pub struct DemoTraceRow {
    pub id: String,
    pub at: chrono::DateTime<chrono::Utc>,
    pub kind: String,
    pub subject: String,
    pub outcome: String,
    pub policy: String,
    pub detail: String,
    // JSON: governance audit payload; each policy stage writes its own shape.
    pub evaluated_rules: Option<serde_json::Value>,
}

/// The merged, time-ordered trace for one session.
///
/// A `policy` of `authz_rule_based` is the gateway deciding whether the caller
/// may reach a model route at all, which is a different gate from the tool
/// checks the plugin hook runs; it gets its own `route` kind so the timeline
/// does not label a model id as a tool.
pub async fn list_demo_trace(
    pool: &PgPool,
    session_id: &SessionId,
    limit: i64,
) -> Result<Vec<DemoTraceRow>, sqlx::Error> {
    sqlx::query_as!(
        DemoTraceRow,
        r#"SELECT id as "id!", created_at as "at!", kind as "kind!", subject as "subject!",
                  outcome as "outcome!", policy as "policy!", detail as "detail!",
                  evaluated_rules as "evaluated_rules?"
           FROM (
             SELECT id,
                    created_at,
                    CASE WHEN tool_name = 'user_prompt' THEN 'prompt'
                         WHEN policy = 'authz_rule_based' THEN 'route'
                         ELSE 'tool' END as kind,
                    tool_name as subject,
                    decision as outcome,
                    policy,
                    reason as detail,
                    evaluated_rules
             FROM governance_decisions
             WHERE session_id = $1 AND session_id <> ''
             UNION ALL
             SELECT id,
                    created_at,
                    'request' as kind,
                    COALESCE(requested_model, model) as subject,
                    status as outcome,
                    '' as policy,
                    COALESCE(NULLIF(error_message, ''),
                             COALESCE(input_tokens, 0)::text || ' in / '
                               || COALESCE(output_tokens, 0)::text || ' out tokens · '
                               || COALESCE(latency_ms, 0)::text || 'ms · $'
                               || ROUND(cost_microdollars / 1000000.0, 4)::text) as detail,
                    NULL::jsonb as evaluated_rules
             FROM ai_requests
             WHERE session_id = $1 AND session_id <> ''
             UNION ALL
             -- Tool fires live in `user_activity`, where `record_mcp_access`
             -- writes them, with the session stamped into `metadata`.
             -- `plugin_usage_events` carries only marketplace-webhook rows, so
             -- reading it here showed an empty fire lane for every session.
             SELECT id,
                    created_at,
                    'fire' as kind,
                    COALESCE(NULLIF(entity_name, ''), 'unknown') as subject,
                    'ok' as outcome,
                    '' as policy,
                    description as detail,
                    NULL::jsonb as evaluated_rules
             FROM user_activity
             WHERE metadata->>'session_id' = $1 AND $1 <> ''
               AND category = 'mcp_access' AND action = 'used'
           ) trace
           ORDER BY created_at ASC
           LIMIT $2"#,
        session_id.as_str(),
        limit,
    )
    .fetch_all(pool)
    .await
}
