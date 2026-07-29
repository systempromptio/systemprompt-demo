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
use super::{events, rpc};

#[derive(Debug, Deserialize)]
pub(super) struct PromptBody {
    token: String,
    conversation_id: systemprompt::identifiers::ContextId,
    message: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct AbortBody {
    token: String,
    conversation_id: systemprompt::identifiers::ContextId,
}

#[derive(Debug, Deserialize)]
pub(super) struct ApproveBody {
    token: String,
    conversation_id: systemprompt::identifiers::ContextId,
    approval_id: String,
    decision: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ApprovalModeBody {
    token: String,
    conversation_id: systemprompt::identifiers::ContextId,
    manual: bool,
}

pub(super) async fn prompt(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<PromptBody>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    say(&pool, &registry, body, Utterance::Prompt).await
}

pub(super) async fn steer(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<PromptBody>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    say(&pool, &registry, body, Utterance::Steer).await
}

pub(super) async fn follow_up(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<PromptBody>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    say(&pool, &registry, body, Utterance::FollowUp).await
}

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

    const fn label(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Steer => "steer",
            Self::FollowUp => "follow_up",
        }
    }
}

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
    session.emit(events::PiEventBody::UserMessage {
        text: body.message.clone(),
        via: utterance.label(),
    });
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
    // Why: the click instant, not the audit-write instant — the stamp the
    // trail shows.
    let decided_at = chrono::Utc::now();
    let user_id = session.user_id.clone();
    let username =
        systemprompt_web_governance::repositories::user_access::find_display_name(&pool, &user_id)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| user_id.to_string());
    let attribution = session::Attribution {
        user_id,
        username,
        decided_at,
    };
    // Why: fail closed — only the two spellings of yes are allowed, anything
    // else (including a typo or an unknown future verb) denies.
    let always = body.decision == "allow_always";
    let verdict = if always || body.decision == "allow" {
        Verdict::Allow(attribution)
    } else {
        Verdict::Deny(attribution)
    };
    if session
        .approvals
        .resolve(&body.approval_id, verdict, always)
    {
        StatusCode::NO_CONTENT.into_response()
    } else {
        problem(StatusCode::CONFLICT, "approval is no longer pending")
    }
}

// Why: the mode a call is judged under is read at the call, so a flip that
// lands mid-turn applies to the next tool call rather than the next session —
// and pending approvals stay pending, because a person was already asked.
pub(super) async fn approval_mode(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<ApprovalModeBody>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(session) =
        authorize_session(&pool, &registry, &body.token, &body.conversation_id).await
    else {
        return unauthorized();
    };
    session.touch();
    session.approvals.set_manual(body.manual);
    tracing::info!(
        conversation_id = %body.conversation_id,
        user_id = %session.user_id,
        manual = body.manual,
        "pi approval mode changed"
    );
    StatusCode::NO_CONTENT.into_response()
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
