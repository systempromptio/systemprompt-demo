//! Opening a conversation and watching it.
//!
//! The two endpoints that are not commands: one spawns the child, the other
//! attaches a viewer to its output.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use systemprompt::identifiers::SessionSource;

use super::auth::{authenticate, problem, unauthorized};
use super::gate::PiDeps;
use super::registry::{self, PiRegistry};
use super::{SHIM_SOURCE, events, pump};

#[derive(Debug, Deserialize)]
pub(super) struct TokenQuery {
    token: String,
    /// Set by `EventSource` reconnects via `Last-Event-ID`; also accepted as a
    /// query param for clients that cannot read the header.
    #[serde(default)]
    since: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateBody {
    token: String,
}

#[derive(Debug, Serialize)]
struct CreatedSession {
    conversation_id: String,
}

pub(super) async fn create_session(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Extension(deps): Extension<Arc<PiDeps>>,
    headers: HeaderMap,
    Json(body): Json<CreateBody>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(user_id) = authenticate(&pool, &body.token).await else {
        return unauthorized();
    };

    // An attested session row, so provider spend in `ai_requests` and governance
    // rows in `governance_decisions` can be joined on one id the server issued.
    let analytics_signals = systemprompt::traits::SessionAnalytics {
        user_agent: header_string(&headers, axum::http::header::USER_AGENT),
        preferred_locale: header_string(&headers, axum::http::header::ACCEPT_LANGUAGE),
        ..Default::default()
    };
    let attested = match deps
        .session_service
        .create_authenticated_session(&user_id, &analytics_signals, SessionSource::Api)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "could not mint a session for a pi conversation");
            return problem(
                StatusCode::INTERNAL_SERVER_ERROR, // lint-ok: http-error — logged above; the client is told nothing about why
                "could not mint a governed session",
            );
        },
    };

    let conversation_id = uuid::Uuid::new_v4().to_string();
    match registry
        .create(conversation_id.clone(), user_id, attested, SHIM_SOURCE)
        .await
    {
        Ok(parts) => {
            let session = Arc::clone(&parts.session);
            pump::start(
                registry.clone(),
                Arc::clone(&deps),
                Arc::clone(&parts.session),
                parts.stdout,
                parts.stderr,
            );
            session.emit(events::PiEventBody::SessionReady {
                conversation_id: conversation_id.clone(),
            });
            (
                StatusCode::CREATED,
                Json(CreatedSession { conversation_id }),
            )
                .into_response()
        },
        Err(registry::SpawnError::PerUserCap(_) | registry::SpawnError::TotalCap(_)) => {
            problem(StatusCode::TOO_MANY_REQUESTS, "session limit reached")
        },
        Err(registry::SpawnError::Io(e)) => {
            tracing::error!(error = %e, "could not spawn pi");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "could not start pi") // lint-ok: http-error — logged above; the client is told nothing about why
        },
    }
}

pub(super) async fn stream(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Path(conversation_id): Path<String>,
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
        // Deliberately the same answer as "no such conversation" would give a
        // stranger: existence is not something to confirm.
        return problem(StatusCode::NOT_FOUND, "no such conversation");
    }

    let since = headers
        .get(axum::http::header::HeaderName::from_static("last-event-id"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .or(q.since)
        .unwrap_or(0);

    // Subscribe before replaying, so a frame emitted between the two arrives
    // twice rather than not at all. The widget dedupes on `seq`.
    let mut rx = session.subscribe();
    let backlog = session.replay_since(since);

    let stream = async_stream::stream! {
        for event in backlog {
            yield Ok(sse_event(&event));
        }
        loop {
            match rx.recv().await {
                Ok(event) => yield Ok(sse_event(&event)),
                // Lagged: this viewer fell behind the broadcast buffer. Keep the
                // stream open — reconnecting with Last-Event-ID repairs the gap.
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

/// The `seq` doubles as the SSE id, which is what makes `Last-Event-ID` resume
/// exactly where the viewer left off.
fn sse_event(event: &events::PiEvent) -> Event {
    let data = serde_json::to_string(event)
        .unwrap_or_else(|_| "{\"type\":\"error\",\"message\":\"unserialisable\"}".to_owned());
    Event::default().id(event.seq.to_string()).data(data)
}

fn header_string(headers: &HeaderMap, name: axum::http::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}
