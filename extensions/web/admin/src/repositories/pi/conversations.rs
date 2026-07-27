//! Durable pi conversations: the row that outlives the process.
//!
//! Two things are durable here. The first is the transcript. The second matters
//! more and is easier to miss: the mapping from a conversation id to the
//! attested session its spend and governance rows key on. Without that mapping
//! on disk, `stats.rs` could only answer questions about a conversation whose
//! child process was still alive — every number it shows is a database query,
//! but the key unlocking those queries would come out of a `HashMap` a reload
//! empties.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use systemprompt::identifiers::{SessionId, UserId};

#[derive(Debug, Clone)]
pub struct PiConversationRow {
    pub id: String,
    pub user_id: UserId,
    pub attested_session_id: SessionId,
    pub title: Option<String>,
    pub last_seq: i64,
    pub closed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct PiConversationSummary {
    pub id: String,
    pub title: Option<String>,
    pub last_seq: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// True while a child process is still attached. Advisory only — the list
    /// is read from Postgres and a server restart leaves no live child behind.
    pub live: bool,
}

pub async fn insert_conversation(
    pool: &PgPool,
    id: &str,
    user_id: &UserId,
    attested: &SessionId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO pi_conversations (id, user_id, attested_session_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (id) DO NOTHING
        "#,
        id,
        user_id.as_str(),
        attested.as_str()
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The one row a caller is allowed to see, or nothing.
///
/// Scoped to the owner in the `WHERE` clause rather than checked afterwards, so
/// there is no shape of this function that can return someone else's
/// conversation. A soft-deleted row is invisible for the same reason a missing
/// one is.
pub async fn find_conversation(
    pool: &PgPool,
    id: &str,
    user_id: &UserId,
) -> Result<Option<PiConversationRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, user_id, attested_session_id, title, last_seq, closed_at
        FROM pi_conversations
        WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
        "#,
        id,
        user_id.as_str()
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| PiConversationRow {
        id: r.id,
        user_id: UserId::new(r.user_id),
        attested_session_id: SessionId::new(r.attested_session_id),
        title: r.title,
        last_seq: r.last_seq,
        closed_at: r.closed_at,
    }))
}

/// The picker's contents, most recently touched first — which is also the
/// order that makes the head of the list the one to restore by default.
pub async fn list_conversations(
    pool: &PgPool,
    user_id: &UserId,
    limit: i64,
) -> Result<Vec<PiConversationSummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, title, last_seq, created_at, updated_at, closed_at
        FROM pi_conversations
        WHERE user_id = $1 AND deleted_at IS NULL
        ORDER BY updated_at DESC
        LIMIT $2
        "#,
        user_id.as_str(),
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| PiConversationSummary {
            id: r.id,
            title: r.title,
            last_seq: r.last_seq,
            created_at: r.created_at,
            updated_at: r.updated_at,
            live: r.closed_at.is_none(),
        })
        .collect())
}

/// Name a conversation.
///
/// The row count distinguishes a title that was set from a conversation the
/// caller does not own, which the `WHERE` clause conflates with one that does
/// not exist.
pub async fn update_conversation_title(
    pool: &PgPool,
    id: &str,
    user_id: &UserId,
    title: &str,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET title = $3, updated_at = NOW()
        WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
        "#,
        id,
        user_id.as_str(),
        title
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

/// Auto-title from the first user message, and only then.
///
/// Separate from [`update_conversation_title`] so the automatic path can never
/// overwrite a name someone chose: the `title IS NULL` predicate is in the
/// statement, not in a read-then-write the writer task would race itself on.
pub async fn update_conversation_title_if_unset(
    pool: &PgPool,
    id: &str,
    title: &str,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET title = $2
        WHERE id = $1 AND title IS NULL
        "#,
        id,
        title
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

/// Point a restored conversation at its new attested session.
///
/// Also clears `closed_at`: the row is live again, and a picker that still
/// showed it as ended would be lying about the child now attached to it.
pub async fn update_conversation_session(
    pool: &PgPool,
    id: &str,
    attested: &SessionId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET attested_session_id = $2, closed_at = NULL, updated_at = NOW()
        WHERE id = $1
        "#,
        id,
        attested.as_str()
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_conversation_closed(pool: &PgPool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET closed_at = NOW()
        WHERE id = $1 AND closed_at IS NULL
        "#,
        id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Soft-delete. The transcript stays on disk because the governance rows it
/// explains do — a conversation a user hid is not one the audit trail forgot.
pub async fn delete_conversation(
    pool: &PgPool,
    id: &str,
    user_id: &UserId,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET deleted_at = NOW()
        WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
        "#,
        id,
        user_id.as_str()
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}
