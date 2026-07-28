//! Per-user activity rollups for the dashboard's Activity tab.
//!
//! Everything here is scoped to one user in the `WHERE` clause and joins
//! through `pi_conversation_sessions`, so each conversation's tallies cover
//! every attested session it was ever bound to — including the ones a resume
//! left behind.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use systemprompt::identifiers::{ContextId, UserId};

#[derive(Debug, Clone)]
pub struct ConversationKpiRow {
    pub id: ContextId,
    pub title: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub requests: i64,
    pub errors: i64,
    pub cost_microdollars: i64,
    pub denied: i64,
    pub tool_calls: i64,
}

#[derive(Debug, Clone)]
pub struct ToolUsageRow {
    pub tool: String,
    pub calls: i64,
}

pub async fn list_user_conversation_kpis(
    pool: &PgPool,
    user_id: &UserId,
    limit: i64,
) -> Result<Vec<ConversationKpiRow>, sqlx::Error> {
    sqlx::query_as!(
        ConversationKpiRow,
        r#"
        SELECT c.id AS "id!: ContextId",
               c.title,
               c.created_at AS "created_at!",
               c.updated_at AS "updated_at!",
               COALESCE(r.requests, 0)::bigint  AS "requests!",
               COALESCE(r.errors, 0)::bigint    AS "errors!",
               COALESCE(r.cost, 0)::bigint      AS "cost_microdollars!",
               COALESCE(g.denied, 0)::bigint    AS "denied!",
               COALESCE(t.tool_calls, 0)::bigint AS "tool_calls!"
        FROM pi_conversations c
        LEFT JOIN LATERAL (
            SELECT COUNT(*) AS requests,
                   COUNT(*) FILTER (WHERE status = 'failed') AS errors,
                   SUM(cost_microdollars) AS cost
            FROM ai_requests
            WHERE session_id IN (SELECT session_id FROM pi_conversation_sessions
                                 WHERE conversation_id = c.id)
        ) r ON TRUE
        LEFT JOIN LATERAL (
            SELECT COUNT(*) FILTER (WHERE decision = 'deny') AS denied
            FROM governance_decisions
            WHERE session_id IN (SELECT session_id FROM pi_conversation_sessions
                                 WHERE conversation_id = c.id)
        ) g ON TRUE
        LEFT JOIN LATERAL (
            SELECT COUNT(*) AS tool_calls
            FROM user_activity
            WHERE category = 'mcp_access' AND action = 'used'
              AND metadata->>'session_id' IN (SELECT session_id FROM pi_conversation_sessions
                                              WHERE conversation_id = c.id)
        ) t ON TRUE
        WHERE c.user_id = $1 AND c.deleted_at IS NULL
        ORDER BY c.updated_at DESC
        LIMIT $2
        "#,
        user_id.as_str(),
        limit
    )
    .fetch_all(pool)
    .await
}

pub async fn list_user_tool_usage(
    pool: &PgPool,
    user_id: &UserId,
    limit: i64,
) -> Result<Vec<ToolUsageRow>, sqlx::Error> {
    sqlx::query_as!(
        ToolUsageRow,
        r#"
        SELECT COALESCE(NULLIF(entity_name, ''), 'unknown') AS "tool!",
               COUNT(*)::bigint AS "calls!"
        FROM user_activity
        WHERE user_id = $1 AND category = 'mcp_access' AND action = 'used'
        GROUP BY 1
        ORDER BY 2 DESC
        LIMIT $2
        "#,
        user_id.as_str(),
        limit
    )
    .fetch_all(pool)
    .await
}
