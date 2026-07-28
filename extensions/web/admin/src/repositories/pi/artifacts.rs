//! The artifacts a conversation's tool calls left behind.
//!
//! Rows in `mcp_artifacts` are written by the hub's tool executor, not by this
//! extension — this module only reads them, and always through the caller's
//! own `user_id`, so a leaked artifact id on its own opens nothing.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use systemprompt::identifiers::{ArtifactId, ContextId, SessionId, UserId};

#[derive(Debug, Clone)]
pub struct McpArtifactRow {
    pub artifact_id: ArtifactId,
    pub artifact_type: String,
    pub title: Option<String>,
    pub server_name: String,
    pub context_id: Option<ContextId>,
    // JSON: the typed artifact payload, shape owned by the tool that made it
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct McpArtifactSummary {
    pub artifact_id: ArtifactId,
    pub artifact_type: String,
    pub title: Option<String>,
    pub server_name: String,
    pub created_at: DateTime<Utc>,
}

/// Every artifact this attested session's tool calls produced, newest first.
///
/// The session id lives in the execution metadata the hub stamped, not in a
/// column — the `->>'session_id'` filter is the price of not owning the table.
pub async fn list_artifacts_for_session(
    pool: &PgPool,
    session_id: &SessionId,
    user_id: &UserId,
    limit: i64,
) -> Result<Vec<McpArtifactSummary>, sqlx::Error> {
    sqlx::query_as!(
        McpArtifactSummary,
        r#"
        SELECT artifact_id AS "artifact_id: ArtifactId",
               artifact_type,
               title,
               server_name,
               created_at
        FROM mcp_artifacts
        WHERE metadata->>'session_id' = $1
          AND user_id = $2
          AND (expires_at IS NULL OR expires_at > NOW())
        ORDER BY created_at DESC
        LIMIT $3
        "#,
        session_id.as_str(),
        user_id.as_str(),
        limit,
    )
    .fetch_all(pool)
    .await
}

pub async fn find_artifact_for_user(
    pool: &PgPool,
    artifact_id: &ArtifactId,
    user_id: &UserId,
) -> Result<Option<McpArtifactRow>, sqlx::Error> {
    sqlx::query_as!(
        McpArtifactRow,
        r#"
        SELECT artifact_id AS "artifact_id: ArtifactId",
               artifact_type,
               title,
               server_name,
               context_id AS "context_id: ContextId",
               data,
               created_at
        FROM mcp_artifacts
        WHERE artifact_id = $1
          AND user_id = $2
          AND (expires_at IS NULL OR expires_at > NOW())
        "#,
        artifact_id.as_str(),
        user_id.as_str(),
    )
    .fetch_optional(pool)
    .await
}
