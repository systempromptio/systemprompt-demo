//! Reading the governance spine back out for one caller.
//!
//! The admin site has rich views over these tables, but they live in a
//! different crate and are reached over an authenticated web session. A client
//! connected to this hub has neither, so the hub's read tools ask the same
//! tables directly:
//!
//! * `governance_decisions` — what was asked for, and whether policy allowed it
//! * `ai_requests` — what reached a provider, and what it cost
//! * `user_activity` — which tool calls actually ran
//! * `ai_safety_findings` — what the gateway's scanners caught before a
//!   provider was reached, which nothing else in this deployment can read
//!
//! Two functions at the bottom break the scoping rule below, each on purpose
//! and each documented at its definition: `list_safety_findings` reaches its
//! `user_id` through a join rather than a column, and `list_all_decisions` is
//! deliberately unscoped because it backs an admin-only tool.
//!
//! Every other query is scoped by **both** `user_id` and the session set, and
//! the two do different jobs. `user_id` is the security boundary: the tool
//! takes no arguments, so there is no selector a caller could widen to read
//! somebody else's rows. The session set is the honesty boundary: the tool is
//! presented as a readout of *this* conversation, and scoping by user alone
//! made it a lifetime total instead — a viewer comparing the numbers against
//! the handful of calls they just watched would find they matched nothing.
//!
//! That set is every session bound to the same conversation, not the one
//! session the caller happens to hold. A reload or resume mints a fresh
//! attested session, so an equality test reported only the slice written since
//! the last refresh and silently under-reported spend for the conversation the
//! caller is looking at. The bound session ids are the only path back to the
//! earlier rows. The caller's own id is unioned in so a session not yet bound
//! still reads its own activity.
//!
//! Tool fires come from `user_activity`, which is where `record_mcp_access`
//! writes them. `plugin_usage_events` only ever carries marketplace-webhook
//! rows, so reading it here reported "no tools ran" for every session that ever
//! ran a tool.

use sqlx::PgPool;
use systemprompt::identifiers::{SessionId, UserId};

pub(crate) const DECISION_LIMIT: i64 = 40;

#[derive(Debug)]
pub(crate) struct PolicyTally {
    pub(crate) policy: String,
    pub(crate) allowed: i64,
    pub(crate) denied: i64,
}

#[derive(Debug)]
pub(crate) struct DecisionRow {
    pub(crate) at: chrono::DateTime<chrono::Utc>,
    pub(crate) tool_name: String,
    pub(crate) decision: String,
    pub(crate) policy: String,
    pub(crate) reason: String,
    pub(crate) reverified: bool,
}

#[derive(Debug)]
pub(crate) struct SpendRow {
    pub(crate) requests: i64,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) cost_microdollars: i64,
}

#[derive(Debug)]
pub(crate) struct SafetyFindingRow {
    pub(crate) at: chrono::DateTime<chrono::Utc>,
    pub(crate) phase: String,
    pub(crate) severity: String,
    pub(crate) category: String,
    pub(crate) scanner: String,
    pub(crate) excerpt: String,
}

#[derive(Debug)]
pub(crate) struct GlobalDecisionRow {
    pub(crate) at: chrono::DateTime<chrono::Utc>,
    pub(crate) user_id: UserId,
    pub(crate) session_id: SessionId,
    pub(crate) tool_name: String,
    pub(crate) decision: String,
    pub(crate) policy: String,
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
           WHERE user_id = $1 AND session_id IN (
                     SELECT b.session_id FROM pi_conversation_sessions b
                      WHERE b.conversation_id IN (
                            SELECT conversation_id FROM pi_conversation_sessions
                             WHERE session_id = $2)
                     UNION ALL SELECT $2)
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
        // Why: one row per logical call — a nested enforcement point re-judging
        // a call appends its own row, and listing both reads as two calls.
        r#"SELECT g.created_at as "at!", g.tool_name as "tool_name!",
                  g.decision as "decision!",
                  COALESCE(NULLIF(g.policy, ''), 'default_included') as "policy!",
                  COALESCE(g.reason, '') as "reason!",
                  EXISTS (
                      SELECT 1 FROM governance_decisions r
                       WHERE r.user_id = g.user_id
                         AND r.session_id = g.session_id
                         AND r.evaluated_rules->>'call_id' = g.evaluated_rules->>'call_id'
                         AND r.evaluated_rules->>'origin' = 'reverified'
                  ) as "reverified!"
           FROM governance_decisions g
           WHERE g.user_id = $1 AND g.session_id IN (
                     SELECT b.session_id FROM pi_conversation_sessions b
                      WHERE b.conversation_id IN (
                            SELECT conversation_id FROM pi_conversation_sessions
                             WHERE session_id = $2)
                     UNION ALL SELECT $2)
             AND g.evaluated_rules->>'origin' IS DISTINCT FROM 'reverified'
           ORDER BY g.created_at DESC
           LIMIT $3"#,
        user_id.as_str(),
        session_id.as_str(),
        limit,
    )
    .fetch_all(pool)
    .await
}

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
                  COALESCE(SUM(cost_microdollars), 0)::BIGINT as "cost_microdollars!"
           FROM ai_requests
           WHERE user_id = $1 AND session_id IN (
                     SELECT b.session_id FROM pi_conversation_sessions b
                      WHERE b.conversation_id IN (
                            SELECT conversation_id FROM pi_conversation_sessions
                             WHERE session_id = $2)
                     UNION ALL SELECT $2)"#,
        user_id.as_str(),
        session_id.as_str(),
    )
    .fetch_one(pool)
    .await
}

pub(crate) async fn list_safety_findings(
    pool: &PgPool,
    user_id: &UserId,
    limit: i64,
) -> Result<Vec<SafetyFindingRow>, sqlx::Error> {
    sqlx::query_as!(
        SafetyFindingRow,
        r#"SELECT f.created_at as "at!", f.phase as "phase!",
                  f.severity as "severity!", f.category as "category!",
                  f.scanner as "scanner!",
                  COALESCE(f.excerpt, '') as "excerpt!"
           FROM ai_safety_findings f
           JOIN ai_requests r ON r.id = f.ai_request_id
           WHERE r.user_id = $1
           ORDER BY f.created_at DESC
           LIMIT $2"#,
        user_id.as_str(),
        limit,
    )
    .fetch_all(pool)
    .await
}

pub(crate) async fn list_all_decisions(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<GlobalDecisionRow>, sqlx::Error> {
    sqlx::query_as!(
        GlobalDecisionRow,
        r#"SELECT created_at as "at!", COALESCE(user_id, '') as "user_id!: UserId",
                  COALESCE(session_id, '') as "session_id!: SessionId",
                  tool_name as "tool_name!", decision as "decision!",
                  COALESCE(NULLIF(policy, ''), 'default_included') as "policy!"
           FROM governance_decisions
           ORDER BY created_at DESC
           LIMIT $1"#,
        limit,
    )
    .fetch_all(pool)
    .await
}
