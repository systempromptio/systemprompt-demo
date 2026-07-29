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
//!
//! The union is windowed, and the window must never decide whether a denial is
//! visible — the feed's "Blocked" filter reads exactly what these functions
//! return, so a block truncated away here becomes a block the pane counts but
//! cannot show. Hence [`list_trace_with_denials`]: the recency window is taken
//! newest-first, and every denial is fetched separately and merged back in.
//! Only `governance_decisions` can carry a `deny` — an `ai_requests` row
//! records a status and a `user_activity` row records a fire that already
//! happened — so that second read is one table.

use sqlx::PgPool;

use crate::repositories::scope::StatsScope;

// Why: far above any plausible denial count, so the bound exists only to stop a
// pathological account returning an unbounded feed.
const DENIAL_LIMIT: i64 = 500;

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

/// The newest `limit` events, plus every denial in scope regardless of age.
pub async fn list_trace_with_denials(
    pool: &PgPool,
    scope: StatsScope<'_>,
    limit: i64,
) -> Result<Vec<DemoTraceRow>, sqlx::Error> {
    let mut rows = list_demo_trace(pool, scope, limit).await?;
    let denials = list_scoped_denials(pool, scope, DENIAL_LIMIT).await?;
    let seen: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    let missing: Vec<DemoTraceRow> = denials
        .into_iter()
        .filter(|d| !seen.contains(d.id.as_str()))
        .collect();
    rows.extend(missing);
    rows.sort_by_key(|r| r.at);
    Ok(rows)
}

async fn list_scoped_denials(
    pool: &PgPool,
    scope: StatsScope<'_>,
    limit: i64,
) -> Result<Vec<DemoTraceRow>, sqlx::Error> {
    sqlx::query_as!(
        DemoTraceRow,
        r#"SELECT id as "id!", created_at as "at!",
                  CASE WHEN tool_name = 'user_prompt' THEN 'prompt'
                       WHEN policy = 'authz_rule_based' THEN 'route'
                       ELSE 'tool' END as "kind!",
                  tool_name as "subject!", decision as "outcome!", policy as "policy!",
                  reason as "detail!", evaluated_rules as "evaluated_rules?"
           FROM governance_decisions
           WHERE decision = 'deny'
             AND user_id = $1
             AND ($2::text IS NULL OR session_id IN (
                   SELECT session_id FROM pi_conversation_sessions
                   WHERE conversation_id = $2))
           ORDER BY created_at DESC
           LIMIT $3"#,
        scope.user(),
        scope.conversation_filter(),
        limit,
    )
    .fetch_all(pool)
    .await
}

// Why: newest-first so the limit bites at the newest end; an account-wide union
// truncated ascending would pin the feed to the oldest decisions ever recorded.
// The caller re-sorts.
async fn list_demo_trace(
    pool: &PgPool,
    scope: StatsScope<'_>,
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
             WHERE user_id = $1
               AND ($2::text IS NULL OR session_id IN (
                     SELECT session_id FROM pi_conversation_sessions
                     WHERE conversation_id = $2))
             UNION ALL
             SELECT id,
                    created_at,
                    'request' as kind,
                    -- A request refused before routing carries neither model,
                    -- and `subject` is asserted non-null by the outer select.
                    -- The row is precisely the one this feed exists to show, so
                    -- it gets a label rather than being dropped.
                    COALESCE(requested_model, model, 'unrouted') as subject,
                    status as outcome,
                    '' as policy,
                    COALESCE(NULLIF(error_message, ''),
                             COALESCE(input_tokens, 0)::text || ' in / '
                               || COALESCE(output_tokens, 0)::text || ' out tokens · '
                               || COALESCE(latency_ms, 0)::text || 'ms · $'
                               || ROUND(cost_microdollars / 1000000.0, 4)::text) as detail,
                    NULL::jsonb as evaluated_rules
             FROM ai_requests
             WHERE user_id = $1
               AND ($2::text IS NULL OR session_id IN (
                     SELECT session_id FROM pi_conversation_sessions
                     WHERE conversation_id = $2))
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
             WHERE user_id = $1
               AND category = 'mcp_access' AND action = 'used'
               AND ($2::text IS NULL OR metadata->>'session_id' IN (
                     SELECT session_id FROM pi_conversation_sessions
                     WHERE conversation_id = $2))
           ) trace
           ORDER BY created_at DESC
           LIMIT $3"#,
        scope.user(),
        scope.conversation_filter(),
        limit,
    )
    .fetch_all(pool)
    .await
}
