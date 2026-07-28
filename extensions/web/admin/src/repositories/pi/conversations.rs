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
use systemprompt::identifiers::{ContextId, SessionId, UserId};

#[derive(Debug, Clone)]
pub struct PiConversationRow {
    pub id: ContextId,
    pub user_id: UserId,
    pub attested_session_id: SessionId,
    pub title: Option<String>,
    pub last_seq: i64,
    pub closed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct PiConversationSummary {
    pub id: ContextId,
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
    id: &ContextId,
    user_id: &UserId,
    attested: &SessionId,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        r#"
        INSERT INTO pi_conversations (id, user_id, attested_session_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (id) DO NOTHING
        "#,
        id.as_str(),
        user_id.as_str(),
        attested.as_str()
    )
    .execute(&mut *tx)
    .await?;
    insert_session_binding(&mut tx, id, attested).await?;
    tx.commit().await?;
    Ok(())
}

// Why: the stat queries join through this binding history; without the row,
// everything written before a resume keys on a session id no query can reach.
async fn insert_session_binding(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: &ContextId,
    attested: &SessionId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO pi_conversation_sessions (conversation_id, session_id)
        VALUES ($1, $2)
        ON CONFLICT DO NOTHING
        "#,
        id.as_str(),
        attested.as_str()
    )
    .execute(&mut **tx)
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
    id: &ContextId,
    user_id: &UserId,
) -> Result<Option<PiConversationRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id AS "id: ContextId", user_id, attested_session_id, title, last_seq, closed_at
        FROM pi_conversations
        WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
        "#,
        id.as_str(),
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

/// The audit-trail lookup: one conversation by id alone, plus its owner's
/// display name.
///
/// Deliberately not owner-scoped — the trace page is a shareable report, and
/// the conversation id in the URL is the unguessable capability that
/// authorizes the read. A soft-deleted row stays invisible: deleting the
/// conversation revokes the link.
pub async fn find_conversation_with_owner(
    pool: &PgPool,
    id: &ContextId,
) -> Result<Option<(PiConversationRow, String)>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT c.id AS "id: ContextId", c.user_id, c.attested_session_id,
               c.title, c.last_seq, c.closed_at,
               u.name AS owner_name
        FROM pi_conversations c
        JOIN users u ON u.id = c.user_id
        WHERE c.id = $1 AND c.deleted_at IS NULL
        "#,
        id.as_str()
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
        (
            PiConversationRow {
                id: r.id,
                user_id: UserId::new(r.user_id),
                attested_session_id: SessionId::new(r.attested_session_id),
                title: r.title,
                last_seq: r.last_seq,
                closed_at: r.closed_at,
            },
            r.owner_name,
        )
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
        SELECT id AS "id: ContextId", title, last_seq, created_at, updated_at, closed_at
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

/// The row count distinguishes a title that was set from a conversation the
/// caller does not own, which the `WHERE` clause conflates with one that does
/// not exist.
pub async fn update_conversation_title(
    pool: &PgPool,
    id: &ContextId,
    user_id: &UserId,
    title: &str,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET title = $3, updated_at = NOW()
        WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
        "#,
        id.as_str(),
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
    id: &ContextId,
    title: &str,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET title = $2
        WHERE id = $1 AND title IS NULL
        "#,
        id.as_str(),
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
    id: &ContextId,
    attested: &SessionId,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET attested_session_id = $2, closed_at = NULL, updated_at = NOW()
        WHERE id = $1
        "#,
        id.as_str(),
        attested.as_str()
    )
    .execute(&mut *tx)
    .await?;
    insert_session_binding(&mut tx, id, attested).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn update_conversation_closed(pool: &PgPool, id: &ContextId) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET closed_at = NOW()
        WHERE id = $1 AND closed_at IS NULL
        "#,
        id.as_str()
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Soft-delete. The transcript stays on disk because the governance rows it
/// explains do — a conversation a user hid is not one the audit trail forgot.
pub async fn delete_conversation(
    pool: &PgPool,
    id: &ContextId,
    user_id: &UserId,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET deleted_at = NOW()
        WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
        "#,
        id.as_str(),
        user_id.as_str()
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}
