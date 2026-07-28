//! The snapshot cache, and the push that replaced the stats poll.
//!
//! One collection feeds two consumers: the `GET stats/{id}` fallback and the
//! ephemeral `stats` frame on the conversation's SSE stream. Sharing the
//! cached body is what keeps a polled number and a pushed number from ever
//! disagreeing about the same conversation.

use std::sync::Arc;

use sqlx::PgPool;
use systemprompt::identifiers::ContextId;

use super::super::events::PiEventBody;
use super::super::session::PiSession;

// Why: an uncached collection is half a dozen queries. A 2s TTL means one
// collection per conversation per burst no matter how many tabs watch it.
// The ownership check still runs per request — only the collection is shared.
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(2);
const CACHE_SWEEP_AT: usize = 64;
const CACHE_STALE: std::time::Duration = std::time::Duration::from_secs(60);

static CACHE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, String)>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub(super) fn cached_body(conversation_id: &ContextId) -> Option<String> {
    let cache = CACHE.lock().ok()?;
    cache
        .get(conversation_id.as_str())
        .filter(|(at, _)| at.elapsed() < CACHE_TTL)
        .map(|(_, body)| body.clone())
}

pub(super) fn store_body(conversation_id: &ContextId, body: String) {
    if let Ok(mut cache) = CACHE.lock() {
        if cache.len() > CACHE_SWEEP_AT {
            cache.retain(|_, (at, _)| at.elapsed() < CACHE_STALE);
        }
        cache.insert(
            conversation_id.as_str().to_owned(),
            (std::time::Instant::now(), body),
        );
    }
}

fn invalidate(conversation_id: &ContextId) {
    if let Ok(mut cache) = CACHE.lock() {
        cache.remove(conversation_id.as_str());
    }
}

// Why: settle time for the request row and audit rows a turn just produced —
// pushing on the very first trigger would snapshot before the numbers land,
// and a turn's tool calls would each schedule their own collection.
const PUSH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(750);

pub(in crate::handlers::pi) async fn snapshot(
    pool: &PgPool,
    session: &PiSession,
) -> Option<serde_json::Value> {
    if let Some(body) = cached_body(&session.conversation_id) {
        return serde_json::from_str(&body).ok();
    }
    match super::collect(pool, &session.conversation_id, &session.user_id).await {
        Ok(stats) => {
            let value = serde_json::to_value(&stats).ok()?;
            if let Ok(body) = serde_json::to_string(&stats) {
                store_body(&session.conversation_id, body);
            }
            Some(value)
        },
        Err(e) => {
            tracing::warn!(error = %e, "could not collect pi stats for push");
            None
        },
    }
}

// Why: this replaced the browser's 3s stats poll — debounced per session so a
// burst of triggers costs one collection, not one per tool call
pub(in crate::handlers::pi) fn push_soon(pool: Arc<PgPool>, session: Arc<PiSession>) {
    if !session.stats_push_begin() {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(PUSH_DEBOUNCE).await;
        session.stats_push_done();
        if session.is_closed() || !session.has_viewers() {
            return;
        }
        invalidate(&session.conversation_id);
        if let Some(stats) = snapshot(&pool, &session).await {
            session.emit_ephemeral(PiEventBody::Stats { stats });
        }
    });
}
