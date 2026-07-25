//! The governed pi web terminal.
//!
//! One `pi --mode rpc` child per conversation, driven from a browser widget.
//! Output streams down over SSE; prompts and approvals come up over POST. The
//! child's stdin is held by the registry, not by a request, so the transport can
//! be ordinary request/response while the process lives for minutes.
//!
//! # Where enforcement actually happens
//!
//! pi runs its own tools in-process. Watching the event stream from outside is
//! too late — `tool_execution_start` is emitted *before* the gate resolves, and
//! the only external lever is `abort`, which kills a whole turn rather than
//! denying one call. So the enforcement point is inside pi: a shim extension
//! whose `tool_call` handler calls `ctx.ui.confirm`, which suspends the call and
//! emits an `extension_ui_request`. The shim decides nothing; this module
//! decides everything, and answers on the same stream.
//!
//! # Two hard-won constraints
//!
//! - **The RPC command surface is ungoverned.** `{"type":"bash"}` executes a
//!   shell command with no `tool_call` hook firing at all. Only [`RpcCommand`]'s
//!   variants are ever constructed here, and no client string reaches pi as a
//!   command type — relaying raw RPC would hand every viewer a shell.
//! - **pi has no sandbox.** Tools run with this process's permissions, so the
//!   default tool set is read-only (`--tools read`, enforced by pi itself) and
//!   the child gets a scratch workspace, a cleared environment, and its own
//!   `HOME`. Enabling `bash` needs a container per session, which V1 does not
//!   have.

mod config;
mod events;
mod gate;
mod pump;
mod registry;
mod rpc;
mod session;
mod spawn;
mod token;

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use systemprompt::config::SecretsBootstrap;
use systemprompt::identifiers::{SessionSource, UserId};

pub(crate) use config::PiConfig;

use crate::error::{AdminError, AdminResult};
use crate::repositories;
use crate::types::UserContext;

pub(crate) use registry::PiRegistry;

use gate::PiDeps;
use session::Verdict;

/// The shim pi loads. Compiled in rather than read from disk so a deployment
/// cannot drift into running a stale or edited enforcement point.
const SHIM_SOURCE: &str = include_str!("shim/governance-shim.ts");

/// Public routes for the widget.
///
/// Public on purpose: the site auth gate 302-redirects unauthenticated hits on
/// protected prefixes, and an `EventSource` reports a redirect to HTML as an
/// opaque error. The embed token is the only credential here, checked by hand.
pub(crate) fn pi_router(
    pool: Arc<PgPool>,
    registry: PiRegistry,
    session_service: Arc<systemprompt::oauth::SessionCreationService>,
    analytics: Arc<dyn systemprompt::traits::AnalyticsProvider>,
) -> Router {
    let deps = Arc::new(PiDeps {
        pool: Arc::clone(&pool),
        analytics,
        session_service,
        cfg: registry.config().clone(),
    });
    Router::new()
        .route("/api/public/pi/session", post(create_session))
        .route("/api/public/pi/stream/{conversation_id}", get(stream))
        .route("/api/public/pi/prompt", post(prompt))
        .route("/api/public/pi/steer", post(steer))
        .route("/api/public/pi/follow-up", post(follow_up))
        .route("/api/public/pi/abort", post(abort))
        .route("/api/public/pi/approve", post(approve))
        .layer(Extension(registry))
        .layer(Extension(deps))
        .with_state(pool)
}

/// Admin-only issuance of an embed token, mounted on the authenticated admin
/// router rather than here.
pub(crate) async fn issue_embed_token_handler(
    Extension(user_ctx): Extension<UserContext>,
    State(pool): State<Arc<PgPool>>,
    Path(target_user_id): Path<String>,
) -> AdminResult<Response> {
    if !user_ctx.is_admin {
        return Err(AdminError::Forbidden("Admin access required".to_owned()));
    }
    let target_user_id = UserId::new(target_user_id);
    let secret = SecretsBootstrap::manifest_signing_secret_seed().map_err(AdminError::internal)?;
    let version = repositories::users::find_share_token_version(&pool, &target_user_id)
        .await
        .map_err(AdminError::internal)?
        .ok_or_else(|| AdminError::NotFound("user not found".to_owned()))?;
    let exp = now_secs() + token::TTL_SECS;
    Ok(Json(IssuedToken {
        token: token::sign(&secret, &target_user_id, version, exp),
        expires_at: exp,
    })
    .into_response())
}

// ── request shapes ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenQuery {
    token: String,
    /// Set by `EventSource` reconnects via `Last-Event-ID`; also accepted as a
    /// query param for clients that cannot read the header.
    #[serde(default)]
    since: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CreateBody {
    token: String,
}

#[derive(Debug, Deserialize)]
struct PromptBody {
    token: String,
    conversation_id: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct AbortBody {
    token: String,
    conversation_id: String,
}

#[derive(Debug, Deserialize)]
struct ApproveBody {
    token: String,
    conversation_id: String,
    approval_id: String,
    /// `"allow"` or anything else, which denies. Defaulting an unrecognised
    /// value to deny keeps a typo from becoming an approval.
    decision: String,
}

#[derive(Debug, Serialize)]
struct IssuedToken {
    token: String,
    expires_at: i64,
}

#[derive(Debug, Serialize)]
struct CreatedSession {
    conversation_id: String,
}

// ── handlers ────────────────────────────────────────────────────────────────

async fn create_session(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Extension(deps): Extension<Arc<PiDeps>>,
    headers: HeaderMap,
    Json(body): Json<CreateBody>,
) -> Response {
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
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not mint a governed session",
            );
        },
    };

    let conversation_id = uuid::Uuid::new_v4().to_string();
    match registry
        .create(
            conversation_id.clone(),
            user_id,
            attested,
            SHIM_SOURCE,
        )
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
            (StatusCode::CREATED, Json(CreatedSession { conversation_id })).into_response()
        },
        Err(registry::SpawnError::PerUserCap(_) | registry::SpawnError::TotalCap(_)) => {
            problem(StatusCode::TOO_MANY_REQUESTS, "session limit reached")
        },
        Err(registry::SpawnError::Io(e)) => {
            tracing::error!(error = %e, "could not spawn pi");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "could not start pi")
        },
    }
}

async fn stream(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Path(conversation_id): Path<String>,
    Query(q): Query<TokenQuery>,
    headers: HeaderMap,
) -> Response {
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

fn sse_event(event: &events::PiEvent) -> Event {
    let data = serde_json::to_string(event)
        .unwrap_or_else(|_| "{\"type\":\"error\",\"message\":\"unserialisable\"}".to_owned());
    Event::default().id(event.seq.to_string()).data(data)
}

async fn prompt(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<PromptBody>,
) -> Response {
    say(&pool, &registry, body, Utterance::Prompt).await
}

/// Redirect the turn that is already running.
async fn steer(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<PromptBody>,
) -> Response {
    say(&pool, &registry, body, Utterance::Steer).await
}

/// Queue a message for after the current turn finishes.
async fn follow_up(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<PromptBody>,
) -> Response {
    say(&pool, &registry, body, Utterance::FollowUp).await
}

/// Which message-carrying command a route sends.
///
/// An enum chosen by the route rather than a string taken from the request: the
/// RPC command type is never client-supplied, which is what keeps `bash` — it
/// bypasses the tool hook entirely — unreachable from a browser.
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
    let Some(session) = authorize_session(pool, registry, &body.token, &body.conversation_id).await
    else {
        return unauthorized();
    };
    session.touch();
    send(&session, utterance.command(body.message)).await
}

async fn abort(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<AbortBody>,
) -> Response {
    let Some(session) = authorize_session(&pool, &registry, &body.token, &body.conversation_id).await
    else {
        return unauthorized();
    };
    session.touch();
    send(&session, rpc::RpcCommand::Abort).await
}

async fn approve(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Json(body): Json<ApproveBody>,
) -> Response {
    let Some(session) = authorize_session(&pool, &registry, &body.token, &body.conversation_id).await
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

// ── helpers ─────────────────────────────────────────────────────────────────

async fn send(session: &Arc<session::PiSession>, command: rpc::RpcCommand) -> Response {
    let Ok(line) = command.to_line() else {
        return problem(StatusCode::INTERNAL_SERVER_ERROR, "could not encode command");
    };
    match session.write_line(&line).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "pi stdin write failed");
            problem(StatusCode::GONE, "the pi session has ended")
        },
    }
}

/// Verify the embed token and recheck the revocation version against the DB.
async fn authenticate(pool: &Arc<PgPool>, raw: &str) -> Option<UserId> {
    let secret = SecretsBootstrap::manifest_signing_secret_seed().ok()?;
    let (user_id, version) = match token::verify(&secret, raw, now_secs()) {
        Ok(v) => v,
        Err(reason) => {
            tracing::debug!(?reason, "pi embed token rejected");
            return None;
        },
    };
    // Revocation: bumping `share_token_version` invalidates every token issued
    // against the old one, so the signature alone is not sufficient.
    let current = repositories::users::find_share_token_version(pool, &user_id)
        .await
        .ok()??;
    (current == version).then_some(user_id)
}

async fn authorize_session(
    pool: &Arc<PgPool>,
    registry: &PiRegistry,
    raw_token: &str,
    conversation_id: &str,
) -> Option<Arc<session::PiSession>> {
    let user_id = authenticate(pool, raw_token).await?;
    let session = registry.get(conversation_id)?;
    (session.user_id == user_id).then_some(session)
}

fn header_string(headers: &HeaderMap, name: axum::http::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn unauthorized() -> Response {
    // lint-ok: http-error — a widget-facing endpoint answers in its own shape
    problem(StatusCode::UNAUTHORIZED, "invalid or expired token")
}

fn problem(status: StatusCode, message: &str) -> Response {
    // lint-ok: http-error — small JSON body the widget renders directly
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::SHIM_SOURCE;

    /// Executable lines only. The shim's own comments discuss the things these
    /// tests forbid — a naive substring search over the whole file would match
    /// the prose explaining why the code avoids them.
    fn shim_code() -> String {
        let mut out = String::with_capacity(SHIM_SOURCE.len());
        let mut rest = SHIM_SOURCE;
        // Block comments first, so a `//` inside one cannot confuse the line pass.
        while let Some(start) = rest.find("/*") {
            out.push_str(&rest[..start]);
            rest = rest[start + 2..].find("*/").map_or("", |end| {
                &rest[start + 2 + end + 2..]
            });
        }
        out.push_str(rest);
        out.lines()
            .map(|l| l.split_once("//").map_or(l, |(code, _)| code))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The shim must decide nothing. A policy name or an HTTP call in here would
    /// mean a second place where a rule lives — and the one nobody reviews.
    #[test]
    fn shim_holds_no_policy() {
        let code = shim_code();
        for forbidden in ["FAIL_OPEN", "fetch(", "blocklist", "secret_scan", "XMLHttpRequest"] {
            assert!(
                !code.contains(forbidden),
                "shim code should not contain {forbidden}"
            );
        }
    }

    /// Every path that is not an explicit approval must block.
    #[test]
    fn shim_denies_by_default() {
        let code = shim_code();
        assert!(code.contains("block: true"), "no block path in the shim");
        assert!(
            code.contains("catch"),
            "a channel failure must be caught and denied"
        );
        assert!(
            code.contains("return false"),
            "the catch arm must deny rather than rethrow"
        );
    }

    /// The comment stripper has to survive the shapes the shim actually uses,
    /// or the tests above quietly stop checking anything.
    #[test]
    fn comment_stripper_removes_both_comment_forms() {
        assert!(shim_code().contains("ExtensionAPI"));
        assert!(
            !shim_code().contains("pi runs its tools in-process"),
            "block comment survived stripping"
        );
        assert!(
            !shim_code().contains("Title the proxy matches on"),
            "line comment survived stripping"
        );
    }
}
