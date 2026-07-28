//! The governed pi web terminal.
//!
//! One `pi --mode rpc` child per conversation, driven from a browser widget.
//! Output streams down over SSE; prompts and approvals come up over POST. The
//! child's stdin is held by the registry, not by a request, so the transport
//! can be ordinary request/response while the process lives for minutes.
//!
//! # Where enforcement actually happens
//!
//! pi runs its own tools in-process. Watching the event stream from outside is
//! too late — `tool_execution_start` is emitted *before* the gate resolves, and
//! the only external lever is `abort`, which kills a whole turn rather than
//! denying one call. So the enforcement point is inside pi: a shim extension
//! whose `tool_call` handler calls `ctx.ui.confirm`, which suspends the call
//! and emits an `extension_ui_request`. The shim decides nothing; this module
//! decides everything, and answers on the same stream.
//!
//! # Three hard-won constraints
//!
//! - **The RPC command surface is ungoverned.** `{"type":"bash"}` executes a
//!   shell command with no `tool_call` hook firing at all. Only
//!   [`rpc::RpcCommand`]'s variants are ever constructed here, and no client
//!   string reaches pi as a command type — relaying raw RPC would hand every
//!   viewer a shell. See [`commands`].
//! - **The gateway credential must belong to the conversation's own user.** The
//!   gateway attests `x-session-id` against the identity that authenticated,
//!   and answers a mismatch with the same opaque `401 unknown or revoked
//!   session` it gives a session that does not exist. A single shared PAT
//!   therefore works for exactly one account and leaves everyone else with a
//!   turn that starts and ends with no output — measured against a live
//!   gateway. So [`registry`] mints a PAT per conversation for that
//!   conversation's user, and revokes it, along with the attested session, when
//!   the conversation ends. That is also why teardown revokes rather than
//!   waiting out an expiry: the gateway's session lookup filters on
//!   `revoked_at` and ignores `expires_at` entirely.
//! - **pi confines nothing itself, and `read` was the proof.** pi's tools run
//!   with whatever permissions the process has, and its `read` applies no path
//!   containment at all — an absolute path is passed through to `readFile`. As
//!   the child runs the server's uid, one approved call could read the
//!   deployment's provider keys, its database URL, its OAuth at-rest pepper,
//!   and the child's own gateway credential. Nothing stopped it but a person
//!   clicking Approve, and in this deployment that person is the untrusted
//!   party. (What actually masked it was the credit guard: an unapproved
//!   account 429s before the model runs, so it can never issue a tool call — a
//!   billing control doing security work, one growth decision from
//!   evaporating.)
//!
//!   Two independent layers close it. Every child now starts through
//!   [`spawn`]'s `sp-pi-jail` wrapper, which applies a Landlock ruleset to
//!   itself and `exec`s pi: read-write on the session workspace, read-execute
//!   on the interpreter and its libraries, `connect()` on the gateway's port,
//!   and nothing else — no `/proc`, so `/proc/<server-pid>/environ` stays
//!   unreadable. And [`scope`] rejects out-of-workspace path arguments before
//!   a human is ever asked, so a denial is a `workspace_scope` audit row and a
//!   legible card rather than a bare `EACCES`.
//!
//!   The residual gaps are real and worth naming. Landlock is a path- and
//!   port-based LSM, not a namespace: the child still shares the pid and
//!   network namespaces, still runs the server's uid, and the granted port is
//!   granted on *any* reachable host rather than loopback alone. Kernels below
//!   6.7 get filesystem confinement only, leaving loopback services reachable
//!   — harmless while the tool set is `read`, not harmless after. A container
//!   per session remains the prerequisite for enabling `bash`: this makes
//!   `read` safe, not arbitrary execution.

mod api;
mod auth;
mod commands;
pub(crate) mod config;
mod conversations;
mod credentials;
pub(crate) mod events;
pub(crate) mod format;
mod gate;
pub(crate) mod jail;
pub(crate) mod mcp;
mod models;
pub(crate) mod normalize;
pub(crate) mod persist;
mod pulse;
mod pump;
mod registry;
pub(crate) mod rpc;
pub(crate) mod scope;
mod session;
pub(crate) mod skills;
mod spawn;
pub(crate) mod stage;
mod stats;
mod tier;
pub(crate) mod token;
pub(crate) mod transcript;
mod watch;

use std::sync::Arc;

use axum::routing::{get, patch, post};
use axum::{Extension, Router};
use sqlx::PgPool;

pub(crate) use auth::issue_embed_token_handler;
pub(crate) use config::PiConfig;
pub(crate) use registry::PiRegistry;

use gate::PiDeps;

/// The shim pi loads. Compiled in rather than read from disk so a deployment
/// cannot drift into running a stale or edited enforcement point.
pub const SHIM_SOURCE: &str = include_str!("shim/governance-shim.ts");

const MCP_CLIENT_SOURCE: &str = include_str!("shim/mcp-client.ts");

pub(crate) fn pi_router(
    pool: Arc<PgPool>,
    registry: PiRegistry,
    session_service: Arc<systemprompt::oauth::SessionCreationService>,
    analytics: Arc<dyn systemprompt::traits::AnalyticsProvider>,
) -> Router {
    registry.config().warn_if_unsandboxed();
    let deps = Arc::new(PiDeps {
        pool: Arc::clone(&pool),
        analytics,
        session_service,
        cfg: registry.config().clone(),
    });
    Router::new()
        .route(
            "/api/public/pi/embed-token",
            post(auth::issue_own_embed_token),
        )
        .route("/api/public/pi/session", post(api::create_session))
        .route(
            "/api/public/pi/stream/{conversation_id}",
            get(watch::stream),
        )
        .route("/api/public/pi/stats/{conversation_id}", get(stats::stats))
        .route("/api/public/pi/pulse", get(pulse::pulse))
        .route("/api/public/pi/models", get(api::models))
        .route("/api/public/pi/prompt", post(commands::prompt))
        .route("/api/public/pi/steer", post(commands::steer))
        .route("/api/public/pi/follow-up", post(commands::follow_up))
        .route("/api/public/pi/abort", post(commands::abort))
        .route("/api/public/pi/approve", post(commands::approve))
        .route("/api/public/pi/mcp", post(mcp::call))
        .route(
            "/api/public/pi/commands/{conversation_id}",
            get(watch::commands),
        )
        .route("/api/public/pi/conversations", get(conversations::list))
        .route(
            "/api/public/pi/conversations/{conversation_id}",
            patch(conversations::rename).delete(conversations::remove),
        )
        .route(
            "/api/public/pi/conversations/{conversation_id}/history",
            get(conversations::history),
        )
        .layer(Extension(registry))
        .layer(Extension(deps))
        .with_state(pool)
}
