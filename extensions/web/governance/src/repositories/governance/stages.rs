//! Per-stage tallies for the four-policy pipeline.
//!
//! The `policy` column cannot answer "how many times did `scope_check` pass?".
//! The canonical writer behind [`super::audit`] records `default_allow` on
//! every allow and, on a deny, only the *first* policy that failed — so a
//! column read reports three of the four stages as having never run. The full
//! chain is in
//! `evaluated_rules`, where `DecisionAudit` serializes one entry per stage:
//!
//! ```json
//! { "chain": [ { "policy_id": "scope_check", "result": "pass", "detail": "" } ] }
//! ```
//!
//! `result` is lowercase on the wire — `ChainEntryResult` carries
//! `#[serde(tag = "result", rename_all = "lowercase")]` — so the predicates
//! below match `'pass'` and `'fail'`, not the Rust variant names.

use sqlx::PgPool;

use super::{GovernanceCounts, PerPolicyCounts};
use crate::repositories::scope::StatsScope;

/// The pipeline's stages, in the order they actually run — the sequence in
/// `services/governance/config.yaml`, which `default_configs()` in
/// `handlers::webhook::governance::policy` mirrors.
///
/// Hard-coded rather than discovered from the rows so a session that has not
/// yet triggered a decision still renders four stages at zero. "The pipeline
/// exists and nothing has tripped it" and "there is no pipeline" are different
/// facts, and an empty query result cannot tell them apart.
///
/// A deployment that reorders or disables a stage in YAML will disagree with
/// this list. That is a display-order question only — the tallies come from the
/// rows — and the alternative, deriving the order from whichever decisions
/// happen to exist, reintroduces the growing-list problem this exists to avoid.
pub const STAGES: [(&str, &str); 4] = [
    ("secret_scan", "Secret scan"),
    ("scope_check", "Scope check"),
    ("tool_blocklist", "Tool blocklist"),
    ("rate_limit", "Rate limit"),
];

/// Pass/fail counts per policy across every attested session in scope, newest
/// activity noted.
///
/// Only stages that actually ran come back; callers fold these onto [`STAGES`]
/// to fill in the zeros.
pub async fn list_scoped_policy_stages(
    pool: &PgPool,
    scope: StatsScope<'_>,
) -> Result<Vec<PerPolicyCounts>, sqlx::Error> {
    sqlx::query_as!(
        PerPolicyCounts,
        r#"SELECT entry->>'policy_id'                                      AS "policy!",
                  COUNT(*) FILTER (WHERE entry->>'result' = 'pass')::bigint AS "allowed!",
                  COUNT(*) FILTER (WHERE entry->>'result' = 'fail')::bigint AS "denied!",
                  MAX(created_at)                                          AS "last_at?"
           FROM governance_decisions,
                LATERAL jsonb_array_elements(evaluated_rules->'chain') entry
           WHERE user_id = $1
             AND ($2::text IS NULL OR session_id IN (
                   SELECT session_id FROM pi_conversation_sessions
                   WHERE conversation_id = $2))
             AND jsonb_typeof(evaluated_rules->'chain') = 'array'
             AND entry->>'policy_id' IS NOT NULL
           GROUP BY entry->>'policy_id'"#,
        scope.user(),
        scope.conversation_filter(),
    )
    .fetch_all(pool)
    .await
}

/// The scope's headline verdict counts.
///
/// The only source for any figure the pane displays. Counting the trace instead
/// reports the newest N rather than the truth, because the trace is a bounded
/// window and this is not.
///
/// `secret_breaches` counts denials attributed to `secret_scan` — the stage
/// whose trips are worth calling out on their own, because a caught credential
/// is the demo's sharpest single moment.
pub async fn get_scoped_governance_counts(
    pool: &PgPool,
    scope: StatsScope<'_>,
) -> Result<GovernanceCounts, sqlx::Error> {
    let row = sqlx::query!(
        r#"SELECT COUNT(*)::bigint                                          AS "total!",
                  COUNT(*) FILTER (WHERE decision = 'allow')::bigint        AS "allowed!",
                  COUNT(*) FILTER (WHERE decision = 'deny')::bigint         AS "denied!",
                  COUNT(*) FILTER (WHERE decision = 'deny'
                                     AND policy = 'secret_scan')::bigint    AS "secret_breaches!",
                  COUNT(*) FILTER (WHERE decision = 'deny'
                                     AND tool_name = 'user_prompt')::bigint AS "prompts_blocked!",
                  COUNT(*) FILTER (WHERE decision = 'deny'
                                     AND tool_name <> 'user_prompt')::bigint AS "tools_blocked!"
           FROM governance_decisions
           WHERE user_id = $1
             AND ($2::text IS NULL OR session_id IN (
                   SELECT session_id FROM pi_conversation_sessions
                   WHERE conversation_id = $2))"#,
        scope.user(),
        scope.conversation_filter(),
    )
    .fetch_one(pool)
    .await?;

    Ok(GovernanceCounts {
        total: row.total,
        allowed: row.allowed,
        denied: row.denied,
        secret_breaches: row.secret_breaches,
        prompts_blocked: row.prompts_blocked,
        tools_blocked: row.tools_blocked,
        tool_calls: count_scoped_tool_calls(pool, scope).await?,
    })
}

// Why: `user_activity` has no `session_id` column — the id is stamped into
// `metadata` — so narrowing to one conversation reads the JSON, while the
// account-wide count uses the indexed `user_id` directly.
async fn count_scoped_tool_calls(pool: &PgPool, scope: StatsScope<'_>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT COUNT(*)::bigint AS "calls!"
           FROM user_activity
           WHERE user_id = $1
             AND category = 'mcp_access' AND action = 'used'
             AND ($2::text IS NULL OR metadata->>'session_id' IN (
                   SELECT session_id FROM pi_conversation_sessions
                   WHERE conversation_id = $2))"#,
        scope.user(),
        scope.conversation_filter(),
    )
    .fetch_one(pool)
    .await
}
