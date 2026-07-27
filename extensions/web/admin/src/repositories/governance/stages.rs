//! Per-stage tallies for the four-policy pipeline.
//!
//! The `policy` column cannot answer "how many times did `scope_check` pass?".
//! [`super::audit::record_decision`] writes `default_allow` on every allow and,
//! on a deny, only the *first* policy that failed — so a column read reports
//! three of the four stages as having never run. The full chain is in
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
use systemprompt::identifiers::SessionId;

use super::{GovernanceCounts, PerPolicyCounts};

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

/// Pass/fail counts per policy for one session, newest activity noted.
///
/// Only stages that actually ran come back; callers fold these onto [`STAGES`]
/// to fill in the zeros.
pub async fn list_session_policy_stages(
    pool: &PgPool,
    session_id: &SessionId,
) -> Result<Vec<PerPolicyCounts>, sqlx::Error> {
    sqlx::query_as!(
        PerPolicyCounts,
        r#"SELECT entry->>'policy_id'                                      AS "policy!",
                  COUNT(*) FILTER (WHERE entry->>'result' = 'pass')::bigint AS "allowed!",
                  COUNT(*) FILTER (WHERE entry->>'result' = 'fail')::bigint AS "denied!",
                  MAX(created_at)                                          AS "last_at?"
           FROM governance_decisions,
                LATERAL jsonb_array_elements(evaluated_rules->'chain') entry
           WHERE session_id = $1
             AND session_id <> ''
             AND jsonb_typeof(evaluated_rules->'chain') = 'array'
             AND entry->>'policy_id' IS NOT NULL
           GROUP BY entry->>'policy_id'"#,
        session_id.as_str(),
    )
    .fetch_all(pool)
    .await
}

/// The session's headline verdict counts.
///
/// `secret_breaches` counts denials attributed to `secret_scan` — the stage
/// whose trips are worth calling out on their own, because a caught credential
/// is the demo's sharpest single moment.
pub async fn get_session_governance_counts(
    pool: &PgPool,
    session_id: &SessionId,
) -> Result<GovernanceCounts, sqlx::Error> {
    let row = sqlx::query!(
        r#"SELECT COUNT(*)::bigint                                          AS "total!",
                  COUNT(*) FILTER (WHERE decision = 'allow')::bigint        AS "allowed!",
                  COUNT(*) FILTER (WHERE decision = 'deny')::bigint         AS "denied!",
                  COUNT(*) FILTER (WHERE decision = 'deny'
                                     AND policy = 'secret_scan')::bigint    AS "secret_breaches!"
           FROM governance_decisions
           WHERE session_id = $1 AND session_id <> ''"#,
        session_id.as_str(),
    )
    .fetch_one(pool)
    .await?;

    Ok(GovernanceCounts {
        total: row.total,
        allowed: row.allowed,
        denied: row.denied,
        secret_breaches: row.secret_breaches,
    })
}
