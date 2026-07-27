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
//! # Two hard-won constraints
//!
//! - **The RPC command surface is ungoverned.** `{"type":"bash"}` executes a
//!   shell command with no `tool_call` hook firing at all. Only
//!   [`rpc::RpcCommand`]'s variants are ever constructed here, and no client
//!   string reaches pi as a command type — relaying raw RPC would hand every
//!   viewer a shell. See [`commands`].
//! - **pi has no sandbox.** Tools run with this process's permissions, so the
//!   default tool set is read-only (`--tools read`, enforced by pi itself) and
//!   the child gets a scratch workspace, a cleared environment, and its own
//!   `HOME`. Enabling `bash` needs a container per session, which V1 does not
//!   have.

mod api;
mod auth;
mod commands;
mod config;
mod events;
mod gate;
mod pump;
mod registry;
mod rpc;
mod session;
mod spawn;
mod token;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::{Extension, Router};
use sqlx::PgPool;

pub(crate) use auth::issue_embed_token_handler;
pub(crate) use config::PiConfig;
pub(crate) use registry::PiRegistry;

use gate::PiDeps;

/// The shim pi loads. Compiled in rather than read from disk so a deployment
/// cannot drift into running a stale or edited enforcement point.
const SHIM_SOURCE: &str = include_str!("shim/governance-shim.ts");

/// Public routes for the widget.
///
/// Public on purpose: the site auth gate 302-redirects unauthenticated hits on
/// protected prefixes, and an `EventSource` reports a redirect to HTML as an
/// opaque error. Credentials are checked by hand in each handler — the embed
/// token everywhere except `/embed-token`, which reads the session cookie.
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
        .route("/api/public/pi/embed-token", post(auth::issue_own_embed_token))
        .route("/api/public/pi/session", post(api::create_session))
        .route("/api/public/pi/stream/{conversation_id}", get(api::stream))
        .route("/api/public/pi/prompt", post(commands::prompt))
        .route("/api/public/pi/steer", post(commands::steer))
        .route("/api/public/pi/follow-up", post(commands::follow_up))
        .route("/api/public/pi/abort", post(commands::abort))
        .route("/api/public/pi/approve", post(commands::approve))
        .layer(Extension(registry))
        .layer(Extension(deps))
        .with_state(pool)
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
            rest = rest[start + 2..]
                .find("*/")
                .map_or("", |end| &rest[start + 2 + end + 2..]);
        }
        out.push_str(rest);
        out.lines()
            .map(|l| l.split_once("//").map_or(l, |(code, _)| code))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The shim must decide nothing. A policy name or an HTTP call in here
    /// would mean a second place where a rule lives — and the one nobody
    /// reviews.
    #[test]
    fn shim_holds_no_policy() {
        let code = shim_code();
        for forbidden in [
            "FAIL_OPEN",
            "fetch(",
            "blocklist",
            "secret_scan",
            "XMLHttpRequest",
        ] {
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
