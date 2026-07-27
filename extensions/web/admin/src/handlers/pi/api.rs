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
use super::registry::{self, CreateRequest, PiRegistry};
use super::{MCP_CLIENT_SOURCE, SHIM_SOURCE, events, pump, transcript};
use crate::repositories::pi::conversations;

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

#[derive(Debug, Deserialize)]
pub(super) struct CreateBody {
    token: String,
    #[serde(default)]
    resume: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreatedSession {
    conversation_id: String,
    last_seq: i64,
    resumed: bool,
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
                StatusCode::INTERNAL_SERVER_ERROR, /* lint-ok: http-error — logged above; the
                                                    * client is told nothing about why */
                "could not mint a governed session",
            );
        },
    };

    let Some(opening) = open_conversation(&pool, &user_id, body.resume.as_deref(), &attested).await
    else {
        return problem(
            // lint-ok: http-error — logged inside; the client is told nothing about why
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not start a conversation",
        );
    };
    let Opening {
        conversation_id,
        start_seq,
        transcript,
        resumed,
    } = opening;

    let result = registry
        .create(CreateRequest {
            conversation_id: conversation_id.clone(),
            user_id,
            attested_session: attested.clone(),
            shim_source: SHIM_SOURCE,
            mcp_client_source: MCP_CLIENT_SOURCE,
            mcp_token: &body.token,
            transcript: transcript.as_deref(),
            start_seq,
        })
        .await;
    if result.is_err() {
        if let Err(e) = deps.analytics.revoke_session(&attested).await {
            tracing::warn!(error = %e, "could not revoke the session of a refused pi conversation");
        }
    }
    match result {
        Ok(parts) => started_response(&registry, &deps, parts, conversation_id, start_seq, resumed),
        Err(e) => spawn_error_response(&e),
    }
}

fn started_response(
    registry: &PiRegistry,
    deps: &Arc<PiDeps>,
    parts: registry::SessionParts,
    conversation_id: String,
    start_seq: u64,
    resumed: bool,
) -> Response {
    // lint-ok: http-error — renders the 201 success body, not an error
    let session = Arc::clone(&parts.session);
    pump::start(
        registry.clone(),
        Arc::clone(deps),
        Arc::clone(&parts.session),
        parts.stdout,
        parts.stderr,
    );
    session.emit(events::PiEventBody::SessionReady {
        conversation_id: conversation_id.clone(),
    });
    (
        StatusCode::CREATED,
        Json(CreatedSession {
            conversation_id,
            last_seq: i64::try_from(start_seq).unwrap_or(i64::MAX),
            resumed,
        }),
    )
        .into_response()
}

fn spawn_error_response(e: &registry::SpawnError) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    match e {
        registry::SpawnError::PerUserCap(_) | registry::SpawnError::TotalCap(_) => {
            problem(StatusCode::TOO_MANY_REQUESTS, "session limit reached")
        },
        registry::SpawnError::Credential(e) => {
            tracing::error!(error = %e, "could not mint a gateway credential for a pi conversation");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "could not start pi") // lint-ok: http-error — logged above; the client is told nothing about why
        },
        registry::SpawnError::Io(e) => {
            tracing::error!(error = %e, "could not spawn pi");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "could not start pi") // lint-ok: http-error — logged above; the client is told nothing about why
        },
    }
}

struct Opening {
    conversation_id: String,
    start_seq: u64,
    transcript: Option<String>,
    resumed: bool,
}

async fn open_conversation(
    pool: &Arc<PgPool>,
    user_id: &systemprompt::identifiers::UserId,
    resume: Option<&str>,
    attested: &systemprompt::identifiers::SessionId,
) -> Option<Opening> {
    let existing = match resume {
        Some(id) => conversations::find_conversation(pool, id, user_id)
            .await
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "could not read a pi conversation to resume");
                None
            }),
        None => None,
    };

    let Some(row) = existing else {
        let conversation_id = uuid::Uuid::new_v4().to_string();
        if let Err(e) =
            conversations::insert_conversation(pool, &conversation_id, user_id, attested).await
        {
            tracing::error!(error = %e, "could not record a pi conversation");
            return None;
        }
        return Some(Opening {
            conversation_id,
            start_seq: 0,
            transcript: None,
            resumed: false,
        });
    };

    if let Err(e) = conversations::update_conversation_session(pool, &row.id, attested).await {
        tracing::error!(error = %e, "could not re-point a resumed pi conversation");
        return None;
    }
    Some(Opening {
        start_seq: u64::try_from(row.last_seq).unwrap_or(0),
        transcript: transcript::render(pool, &row.id).await,
        conversation_id: row.id,
        resumed: true,
    })
}

pub(super) async fn commands(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Path(conversation_id): Path<String>,
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
    Event::default().id(event.seq().to_string()).data(data)
}

fn header_string(headers: &HeaderMap, name: axum::http::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}
