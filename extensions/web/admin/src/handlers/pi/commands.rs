//! Everything a viewer can send *into* a running conversation.
//!
//! The security property this module exists to hold: the RPC command type is
//! picked by the route it arrived on, never read from the request. `bash` is an
//! RPC command that runs a shell with no `tool_call` hook firing at all, so a
//! passthrough here would hand every viewer a shell.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;
use sqlx::PgPool;

use super::auth::{authorize_session, problem, unauthorized};
use super::registry::PiRegistry;
use super::session::{self, Verdict};
use super::rpc;

#[derive(Debug, Deserialize)]
pub(super) struct PromptBody {
    token: String,
    conversation_id: String,
    message: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct AbortBody {
    token: String,
    conversation_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ApproveBody {
    token: String,
    conversation_id: String,
    approval_id: String,
    /// `"allow"` or anything else, which denies. Defaulting an unrecognised
    /// value to deny keeps a typo from becoming an approval.
    decision: String,
}

pub(super) async fn prompt(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<PromptBody>,
) -> Response { // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    say(&pool, &registry, body, Utterance::Prompt).await
}

/// Redirect the turn that is already running.
pub(super) async fn steer(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<PromptBody>,
) -> Response { // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    say(&pool, &registry, body, Utterance::Steer).await
}

/// Queue a message for after the current turn finishes.
pub(super) async fn follow_up(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<PromptBody>,
) -> Response { // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    say(&pool, &registry, body, Utterance::FollowUp).await
}

/// Which message-carrying command a route sends.
///
/// An enum chosen by the route rather than a string taken from the request: see
/// the module docs for why that distinction is load-bearing.
#[derive(Debug, Clone, Copy)]
enum Utterance {
    Prompt,
    Steer,
    FollowUp,
}

impl Utterance {
    const fn command(self, message: String) -> rpc::RpcCommand {
        match self {
            Self::Prompt => rpc::RpcCommand::Prompt { message },
            Self::Steer => rpc::RpcCommand::Steer { message },
            Self::FollowUp => rpc::RpcCommand::FollowUp { message },
        }
    }
}

/// The shared path for every message-carrying command.
async fn say(
    pool: &Arc<PgPool>,
    registry: &PiRegistry,
    body: PromptBody,
    utterance: Utterance,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(session) = authorize_session(pool, registry, &body.token, &body.conversation_id).await
    else {
        return unauthorized();
    };
    session.touch();
    send(&session, utterance.command(body.message)).await
}

pub(super) async fn abort(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<AbortBody>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(session) =
        authorize_session(&pool, &registry, &body.token, &body.conversation_id).await
    else {
        return unauthorized();
    };
    session.touch();
    send(&session, rpc::RpcCommand::Abort).await
}

pub(super) async fn approve(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<ApproveBody>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(session) =
        authorize_session(&pool, &registry, &body.token, &body.conversation_id).await
    else {
        return unauthorized();
    };
    session.touch();
    let verdict = if body.decision == "allow" {
        Verdict::Allow
    } else {
        Verdict::Deny
    };
    if session.resolve_approval(&body.approval_id, verdict) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        // Already answered, timed out, or never existed. The widget shows
        // "expired" rather than pretending the click landed.
        problem(StatusCode::CONFLICT, "approval is no longer pending")
    }
}

async fn send(session: &Arc<session::PiSession>, command: rpc::RpcCommand) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Ok(line) = command.to_line() else {
        return problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not encode command",
        );
    };
    match session.write_line(&line).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "pi stdin write failed");
            problem(StatusCode::GONE, "the pi session has ended")
        },
    }
}
