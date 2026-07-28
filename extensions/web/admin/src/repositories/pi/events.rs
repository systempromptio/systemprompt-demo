//! The stored frames of a conversation.
//!
//! Separate from [`super::conversations`] because the two have different write
//! rhythms: a conversation row is touched a handful of times over its life,
//! while frames arrive in batches for as long as a child is producing output.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use systemprompt::identifiers::ContextId;

#[derive(Debug, Clone)]
pub struct PiStoredEvent {
    pub seq: i64,
    pub kind: String,
    // JSON: the stored frame itself — a JSONB column replayed verbatim
    pub body: serde_json::Value,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct TranscriptMessage {
    pub kind: String,
    pub text: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewPiEvent {
    pub seq: i64,
    pub kind: String,
    // JSON: the frame to store — a JSONB column written verbatim
    pub body: serde_json::Value,
}

/// Stored frames after `after_seq`, oldest first — the order a transcript
/// replays in.
pub async fn list_conversation_events(
    pool: &PgPool,
    id: &ContextId,
    after_seq: i64,
    limit: i64,
) -> Result<Vec<PiStoredEvent>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT seq, kind, body, at
        FROM pi_conversation_events
        WHERE conversation_id = $1 AND seq > $2
        ORDER BY seq
        LIMIT $3
        "#,
        id.as_str(),
        after_seq,
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| PiStoredEvent {
            seq: r.seq,
            kind: r.kind,
            body: r.body,
            at: r.at,
        })
        .collect())
}

/// The prose lanes of a transcript: what the user typed (`user_message`) and
/// the coalesced text the model streamed back (`text_delta`), in replay order.
pub async fn list_transcript_messages(
    pool: &PgPool,
    id: &ContextId,
    limit: i64,
) -> Result<Vec<TranscriptMessage>, sqlx::Error> {
    sqlx::query_as!(
        TranscriptMessage,
        r#"
        SELECT kind as "kind!", body->>'text' as "text!", at as "at!"
        FROM pi_conversation_events
        WHERE conversation_id = $1
          AND kind IN ('user_message', 'text_delta')
          AND body->>'text' IS NOT NULL
        ORDER BY seq
        LIMIT $2
        "#,
        id.as_str(),
        limit
    )
    .fetch_all(pool)
    .await
}

/// Write a batch of frames and advance the conversation's watermark.
///
/// One statement per column via `UNNEST` rather than a loop of inserts: the
/// writer batches a turn's worth of frames, and a round trip each would put
/// Postgres latency in the path of a streaming transcript.
///
/// `ON CONFLICT DO NOTHING` because a retried batch after a partial failure
/// must not abort on frames that already landed — `seq` is monotonic per
/// conversation, so a duplicate is always the same frame.
pub async fn insert_conversation_events(
    pool: &PgPool,
    id: &ContextId,
    events: &[NewPiEvent],
) -> Result<(), sqlx::Error> {
    if events.is_empty() {
        return Ok(());
    }
    let seqs: Vec<i64> = events.iter().map(|e| e.seq).collect();
    let kinds: Vec<String> = events.iter().map(|e| e.kind.clone()).collect();
    let bodies: Vec<serde_json::Value> = events.iter().map(|e| e.body.clone()).collect();
    let high = seqs.iter().copied().max().unwrap_or(0);

    let mut tx = pool.begin().await?;
    sqlx::query!(
        r#"
        INSERT INTO pi_conversation_events (conversation_id, seq, kind, body)
        SELECT $1, s, k, b
        FROM UNNEST($2::bigint[], $3::text[], $4::jsonb[]) AS t(s, k, b)
        ON CONFLICT (conversation_id, seq) DO NOTHING
        "#,
        id.as_str(),
        &seqs,
        &kinds,
        &bodies
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"
        UPDATE pi_conversations
        SET last_seq = GREATEST(last_seq, $2), updated_at = NOW()
        WHERE id = $1
        "#,
        id.as_str(),
        high
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
