//! Opening a conversation and watching it.
//!
//! The two endpoints that are not commands: one spawns the child, the other
//! attaches a viewer to its output.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use systemprompt::identifiers::{ContextId, SessionSource};

use super::auth::{authenticate, problem, unauthorized};
use super::gate::PiDeps;
use super::registry::{self, CreateRequest, PiRegistry};
use super::{MCP_CLIENT_SOURCE, SHIM_SOURCE, events, pump, transcript};
use crate::repositories::conversations;

#[derive(Debug, Deserialize)]
pub(super) struct CreateBody {
    token: String,
    #[serde(default)]
    resume: Option<ContextId>,
    // Why: must be on the configured allow-list; absent means the default
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Serialize)]
struct ModelCatalogue {
    models: Vec<super::models::GatewayModel>,
    default: String,
}

// Why: the gateway's own advertised catalogue, provider labels included, with
// the same exposure class as `pulse`, so deliberately unauthenticated
pub(super) async fn models(Extension(deps): Extension<Arc<PiDeps>>) -> Response {
    // lint-ok: http-error — infallible JSON body the widget renders directly
    Json(ModelCatalogue {
        models: super::models::catalogue(&deps.cfg),
        default: deps.cfg.model_name().to_owned(),
    })
    .into_response()
}

#[derive(Debug, Serialize)]
struct CreatedSession {
    conversation_id: ContextId,
    last_seq: i64,
    resumed: bool,
    manual_approval: bool,
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

    let Some(model) = super::models::resolve(&deps.cfg, body.model.as_deref()) else {
        return problem(StatusCode::BAD_REQUEST, "unknown model");
    };

    let attested = match mint_attested(&deps, &headers, &user_id).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    let Some(opening) = open_conversation(&pool, &user_id, body.resume.as_ref(), &attested).await
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
            model: &model,
        })
        .await;
    if result.is_err()
        && let Err(e) = deps.analytics.revoke_session(&attested).await
    {
        tracing::warn!(error = %e, "could not revoke the session of a refused pi conversation");
    }
    match result {
        Ok(parts) => started_response(
            &registry,
            &deps,
            parts,
            CreatedSession {
                conversation_id,
                last_seq: i64::try_from(start_seq).unwrap_or(i64::MAX),
                resumed,
                manual_approval: deps.cfg.require_approval(),
            },
        ),
        Err(e) => spawn_error_response(&e),
    }
}

async fn mint_attested(
    deps: &Arc<PiDeps>,
    headers: &HeaderMap,
    user_id: &systemprompt::identifiers::UserId,
) -> Result<systemprompt::identifiers::SessionId, Response> {
    let analytics_signals = systemprompt::traits::SessionAnalytics {
        user_agent: header_string(headers, axum::http::header::USER_AGENT),
        preferred_locale: header_string(headers, axum::http::header::ACCEPT_LANGUAGE),
        ..Default::default()
    };
    deps.session_service
        .create_authenticated_session(user_id, &analytics_signals, SessionSource::Api)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "could not mint a session for a pi conversation");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR, /* lint-ok: http-error — logged above; the
                                                    * client is told nothing about why */
                "could not mint a governed session",
            )
        })
}

fn started_response(
    registry: &PiRegistry,
    deps: &Arc<PiDeps>,
    parts: registry::SessionParts,
    created: CreatedSession,
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
        conversation_id: created.conversation_id.clone(),
    });
    (StatusCode::CREATED, Json(created)).into_response()
}

fn spawn_error_response(e: &registry::SpawnError) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    match e {
        registry::SpawnError::PerUserCap(_) => {
            problem(StatusCode::TOO_MANY_REQUESTS, "session limit reached")
        },
        // Why: a full server is not opaque — the body carries the caller's
        // place in line so the widget can render "#N in line" and keep the
        // spot warm by polling /capacity.
        registry::SpawnError::Waitlisted {
            position,
            queue_len,
        } => (
            StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, "5")],
            Json(serde_json::json!({
                "error": "server at capacity",
                "reason": "waitlisted",
                "position": position,
                "queue_len": queue_len,
            })),
        )
            .into_response(),
        registry::SpawnError::Credential(e) => {
            tracing::error!(error = %e, "could not mint a gateway credential for a pi conversation");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "could not start pi") // lint-ok: http-error — logged above; the client is told nothing about why
        },
        registry::SpawnError::Io(e) => {
            tracing::error!(error = %e, "could not spawn pi");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "could not start pi") // lint-ok: http-error — logged above; the client is told nothing about why
        },
        registry::SpawnError::Version(e) => {
            tracing::error!(error = %e, "pi version gate refused to spawn");
            // Why: unlike the opaque errors above, this one names its fix —
            // it can only be seen by the operator whose install drifted.
            problem(
                StatusCode::INTERNAL_SERVER_ERROR, /* lint-ok: http-error — deliberate
                                                    * operator-facing 500 */
                "pi version mismatch — align the install with services/config/pi.yaml",
            )
        },
    }
}

struct Opening {
    conversation_id: ContextId,
    start_seq: u64,
    transcript: Option<String>,
    resumed: bool,
}

async fn open_conversation(
    pool: &Arc<PgPool>,
    user_id: &systemprompt::identifiers::UserId,
    resume: Option<&ContextId>,
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
        let conversation_id = ContextId::generate();
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

fn header_string(headers: &HeaderMap, name: axum::http::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}
