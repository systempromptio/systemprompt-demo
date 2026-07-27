//! The platform pulse: the same spine, counted across everybody.
//!
//! A visitor watching their own session learns what we record. What they cannot
//! learn from it is that the machinery is not staged for them — one session's
//! numbers look the same whether the deployment governs one agent or a
//! thousand. These aggregates answer that, and they are the only place in the
//! public API that reads rows belonging to other people.
//!
//! So the contract here is narrower than anywhere else: every function returns
//! counts and rates only. No identifier — no user, session, trace, or email —
//! reaches a return type, which is what makes the shape safe to serve to any
//! signed-in caller. Tool *names* are the one exception, and they are a
//! property of the deployment's policy, not of whoever tripped it.
//!
//! Anonymous and replay traffic is excluded from the people count by the same
//! predicate [`super::super::dashboard::aggregates::get_active_users_24h`]
//! uses, so the two never disagree about how many humans are here.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Everything that happened in one rolling window.
#[derive(Debug, Clone, Copy, Default)]
pub struct PulseWindow {
    pub people: i64,
    pub sessions: i64,
    pub requests: i64,
    pub tool_calls: i64,
    pub allowed: i64,
    pub denied: i64,
    pub latency_p50_ms: Option<i32>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_microdollars: i64,
}

/// Cumulative totals since the deployment's first row.
#[derive(Debug, Clone, Copy, Default)]
pub struct PulseTotals {
    pub sessions: i64,
    pub requests: i64,
    pub tool_calls: i64,
    pub secrets_caught: i64,
}

/// One model's share of a window.
#[derive(Debug, Clone)]
pub struct PulseModelShare {
    pub model: String,
    pub requests: i64,
}

/// A tool and how often policy refused it.
#[derive(Debug, Clone)]
pub struct PulseBlockedTool {
    pub tool_name: String,
    pub denials: i64,
}

/// Inference and governance counts since `since`.
///
/// Two subqueries rather than a join: `ai_requests` and `governance_decisions`
/// have no row-level correspondence — one turn produces one request and any
/// number of decisions — so joining them would multiply the token sums by the
/// tool-call count.
pub async fn get_pulse_window(
    pool: &PgPool,
    since: DateTime<Utc>,
) -> Result<PulseWindow, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        WITH reqs AS (
            SELECT r.user_id, r.session_id, r.latency_ms, r.input_tokens,
                   r.output_tokens, r.cost_microdollars
            FROM ai_requests r
            JOIN users u ON u.id = r.user_id
            WHERE r.created_at >= $1
              AND NOT ('anonymous' = ANY(u.roles))
              AND u.email NOT LIKE '%@anonymous.local'
        ),
        decisions AS (
            SELECT d.decision
            FROM governance_decisions d
            JOIN users u ON u.id = d.user_id
            WHERE d.created_at >= $1
              AND NOT ('anonymous' = ANY(u.roles))
              AND u.email NOT LIKE '%@anonymous.local'
        )
        SELECT
            (SELECT COUNT(DISTINCT user_id) FROM reqs)::bigint            AS "people!",
            -- `session_id` is ON DELETE SET NULL, so a null is a session whose
            -- row is gone, not a request that never had one. Excluded, or the
            -- denominator counts them all as a single extra session.
            (SELECT COUNT(DISTINCT session_id) FROM reqs
              WHERE session_id IS NOT NULL AND session_id <> '')::bigint  AS "sessions!",
            (SELECT COUNT(*) FROM reqs)::bigint                           AS "requests!",
            (SELECT COALESCE(SUM(input_tokens), 0) FROM reqs)::bigint     AS "input_tokens!",
            (SELECT COALESCE(SUM(output_tokens), 0) FROM reqs)::bigint    AS "output_tokens!",
            (SELECT COALESCE(SUM(cost_microdollars), 0) FROM reqs)::bigint
                                                                          AS "cost_microdollars!",
            (SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY latency_ms)
               FROM reqs WHERE latency_ms IS NOT NULL)                    AS "latency_p50?: f64",
            (SELECT COUNT(*) FROM decisions WHERE decision = 'allow')::bigint
                                                                          AS "allowed!",
            (SELECT COUNT(*) FROM decisions WHERE decision = 'deny')::bigint
                                                                          AS "denied!",
            (SELECT COUNT(*) FROM user_activity ua
               JOIN users u2 ON u2.id = ua.user_id
              WHERE ua.created_at >= $1
                AND ua.category = 'mcp_access' AND ua.action = 'used'
                AND NOT ('anonymous' = ANY(u2.roles))
                AND u2.email NOT LIKE '%@anonymous.local')::bigint        AS "tool_calls!"
        "#,
        since,
    )
    .fetch_one(pool)
    .await?;

    Ok(PulseWindow {
        people: row.people,
        sessions: row.sessions,
        requests: row.requests,
        tool_calls: row.tool_calls,
        allowed: row.allowed,
        denied: row.denied,
        // A latency in milliseconds that overflows an i32 is 24 days; the
        // column it came from is already i32, so the cast cannot lose anything
        // a real row could carry.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "source column is i32; a p50 cannot exceed its own inputs"
        )]
        latency_p50_ms: row.latency_p50.map(|v| v.round() as i32),
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        cost_microdollars: row.cost_microdollars,
    })
}

/// Which models served a window, busiest first.
pub async fn list_pulse_model_mix(
    pool: &PgPool,
    since: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<PulseModelShare>, sqlx::Error> {
    sqlx::query_as!(
        PulseModelShare,
        r#"SELECT r.model AS "model!", COUNT(*)::bigint AS "requests!"
           FROM ai_requests r
           JOIN users u ON u.id = r.user_id
           WHERE r.created_at >= $1
             AND NOT ('anonymous' = ANY(u.roles))
             AND u.email NOT LIKE '%@anonymous.local'
           GROUP BY r.model
           ORDER BY COUNT(*) DESC
           LIMIT $2"#,
        since,
        limit,
    )
    .fetch_all(pool)
    .await
}

/// The tools policy refused most often in a window.
pub async fn list_pulse_blocked_tools(
    pool: &PgPool,
    since: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<PulseBlockedTool>, sqlx::Error> {
    sqlx::query_as!(
        PulseBlockedTool,
        r#"SELECT d.tool_name AS "tool_name!", COUNT(*)::bigint AS "denials!"
           FROM governance_decisions d
           WHERE d.created_at >= $1 AND d.decision = 'deny' AND d.tool_name <> ''
           GROUP BY d.tool_name
           ORDER BY COUNT(*) DESC
           LIMIT $2"#,
        since,
        limit,
    )
    .fetch_all(pool)
    .await
}

/// Lifetime totals.
///
/// Unfiltered by identity on purpose: this is the "how much has this thing
/// governed, ever" line, and excluding replay traffic from it would make it
/// disagree with the audit tables an operator reads from the CLI.
pub async fn get_pulse_all_time(pool: &PgPool) -> Result<PulseTotals, sqlx::Error> {
    let row = sqlx::query!(
        r#"SELECT
            (SELECT COUNT(DISTINCT session_id) FROM ai_requests
              WHERE session_id IS NOT NULL AND session_id <> '')::bigint AS "sessions!",
            (SELECT COUNT(*) FROM ai_requests)::bigint                   AS "requests!",
            (SELECT COUNT(*) FROM user_activity
              WHERE category = 'mcp_access' AND action = 'used')::bigint AS "tool_calls!",
            (SELECT COUNT(*) FROM governance_decisions
              WHERE decision = 'deny' AND policy = 'secret_scan')::bigint
                                                                         AS "secrets_caught!""#,
    )
    .fetch_one(pool)
    .await?;

    Ok(PulseTotals {
        sessions: row.sessions,
        requests: row.requests,
        tool_calls: row.tool_calls,
        secrets_caught: row.secrets_caught,
    })
}
