//! Watching a running conversation: the SSE stream and the command catalogue.
//!
//! Split from [`super::api`] so opening a conversation and observing one stay
//! separate seams — the viewer path holds no spawn logic and vice versa.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use futures::stream::Stream;
use serde::Deserialize;
use sqlx::PgPool;
use systemprompt::identifiers::ContextId;

use super::auth::{authenticate, problem, unauthorized};
use super::events;
use super::registry::PiRegistry;

#[derive(Debug, Deserialize)]
pub(super) struct TokenQuery {
    token: String,
    #[serde(default)]
    since: Option<u64>,
}

impl TokenQuery {
    pub(super) fn token(&self) -> &str {
        &self.token
    }
}

pub(super) async fn commands(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Path(conversation_id): Path<ContextId>,
    Query(q): Query<TokenQuery>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    if super::auth::authorize_session(&pool, &registry, &q.token, &conversation_id)
        .await
        .is_none()
    {
        return unauthorized();
    }
    Json(super::skills::catalogue().await).into_response()
}

pub(super) async fn stream(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Path(conversation_id): Path<ContextId>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(user_id) = authenticate(&pool, &q.token).await else {
        return unauthorized();
    };
    let Some(session) = registry.get(&conversation_id) else {
        return problem(StatusCode::NOT_FOUND, "no such conversation");
    };
    if session.user_id != user_id {
        return problem(StatusCode::NOT_FOUND, "no such conversation");
    }

    let since = headers
        .get(axum::http::header::HeaderName::from_static("last-event-id"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .or(q.since)
        .unwrap_or(0);

    let mut rx = session.subscribe();
    let backlog = session.replay_since(since);

    let stream = async_stream::stream! {
        for event in backlog {
            yield Ok(sse_event(&event));
        }
        // Why: stats frames never enter the replay buffer, so without this a
        // fresh viewer would show stale meters until the next turn
        if let Some(stats) = super::stats::snapshot(&pool, &session).await {
            yield Ok(sse_event(&events::PiEvent::ephemeral(
                events::PiEventBody::Stats { stats },
            )));
        }
        loop {
            match rx.recv().await {
                Ok(event) => yield Ok(sse_event(&event)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!(skipped = n, "pi viewer lagged");
                },
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    let stream: Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> = Box::pin(stream);
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

fn sse_event(event: &events::PiEvent) -> Event {
    let data = serde_json::to_string(event)
        .unwrap_or_else(|_| "{\"type\":\"error\",\"message\":\"unserialisable\"}".to_owned());
    let sse = Event::default().data(data);
    // Why: an ephemeral frame must not become the browser's Last-Event-ID —
    // a reconnect would resume from a seq the transcript never had
    match event.seq() {
        Some(seq) => sse.id(seq.to_string()),
        None => sse,
    }
}
