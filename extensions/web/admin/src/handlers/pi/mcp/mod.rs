//! The bridge between a governed pi session and the `systemprompt` MCP hub.
//!
//! pi ships no MCP client — the project says so outright and points at its
//! extension API instead — so the hub reaches a session as a pi extension that
//! registers one tool per hub tool (`shim/mcp-client.ts`). This module is the
//! other half: the endpoint that extension calls.
//!
//! **Why a proxy rather than letting the child speak MCP directly.** Two
//! independent reasons, either sufficient:
//!
//! 1. The hub listens on its own port, and the session's Landlock jail grants
//!    outbound TCP to the gateway's port alone (`jail.rs`). Reaching the hub
//!    directly would mean widening the sandbox, which is the one thing the
//!    sandbox exists to avoid.
//! 2. The hub identifies its caller from injected headers — `x-user-id`,
//!    `x-session-id`, `x-trace-id`, `x-agent-name` — and separately requires a
//!    Bearer token carrying the `mcp` audience. A child able to set either
//!    could name any user it liked. Here both are derived from the authorized
//!    [`PiSession`], and the caller cannot influence them. The conversation's
//!    gateway PAT is deliberately *not* reused: it carries no audience, and the
//!    hub's RBAC rejects it.
//!
//! **This endpoint runs the policy chain itself, and has to.** The shim's
//! `tool_call` hook does fire on extension-registered tools, so a call arriving
//! from the model has already been judged. But the endpoint is reachable
//! directly by anything holding an embed token — the page that opened the
//! conversation, a curl with the token out of the URL, the child deciding to
//! skip its own tools — and none of those paths pass through the shim.
//! Trusting the caller to have been gated would mean `fetch_remote_docs`
//! executes for anyone who asks for it by hand, which is precisely the tool
//! whose refusal the demo is built to show. So the chain runs here too, on the
//! `mcp__systemprompt__*` name, and the duplicate verdict for a model-issued
//! call is the cheap half of the trade.

mod hub;
pub(crate) mod render;

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;
use sqlx::PgPool;

use systemprompt::identifiers::SessionId;

use crate::handlers::webhook::governance::inproc::{self, GovernedCall};
use super::auth::{authorize_session, problem, unauthorized};
use super::gate::PiDeps;
use super::registry::PiRegistry;
use render::McpCallResult;

/// Tools the proxy will forward.
///
/// An allowlist rather than a passthrough, for the same reason `commands.rs`
/// picks its RPC command by route: the child is the least trusted thing in this
/// system, and "whatever name it sent" is not a property anyone can reason
/// about later. Adding a hub tool means adding it here, which is the point.
pub const FORWARDABLE: &[&str] = &[
    "list_topics",
    "get_topic",
    "search_docs",
    "governance_stats",
    "fetch_remote_docs",
];

/// The MCP protocol revision the hub was built against.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// What the hub records as the calling agent. The widget is one agent as far as
/// the audit spine is concerned, distinct from the configured A2A agents.
const AGENT_NAME: &str = "pi-widget";

/// Lifetime of the token minted per call. One hour is the floor `generate_jwt`
/// works in, and the token never leaves this process — it is created, used for
/// three loopback requests, and dropped.
const TOKEN_TTL_HOURS: i64 = 1;

#[derive(Debug, Deserialize)]
pub(super) struct McpCallBody {
    token: String,
    conversation_id: String,
    /// Hub tool name, checked against [`FORWARDABLE`].
    tool: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

pub(super) async fn call(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Extension(deps): Extension<Arc<PiDeps>>,
    Json(body): Json<McpCallBody>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(session) =
        authorize_session(&pool, &registry, &body.token, &body.conversation_id).await
    else {
        return unauthorized();
    };
    session.touch();

    if !FORWARDABLE.contains(&body.tool.as_str()) {
        return problem(StatusCode::BAD_REQUEST, "unknown tool");
    }

    // The governed name is the MCP name, so a verdict here is the same verdict
    // the shim would produce for the same call, and lands one audit row shape.
    let tool_name = format!("mcp__systemprompt__{}", body.tool);
    let agent_session = SessionId::new(session.conversation_id.clone());
    let verdict = inproc::govern_call(
        &deps.pool,
        &deps.analytics,
        &session.attested_session,
        &GovernedCall {
            tool_name: &tool_name,
            user_id: &session.user_id,
            agent_session: &agent_session,
            tool_input: Some(&body.arguments),
            // Sandboxed demo surface: hold every caller at user scope so an
            // admin does not silently skip `tool_blocklist` and reach the
            // network through a tool the deployment blocks. See `pi/gate/mod.rs`.
            scope_ceiling: AccessScope::User,
        },
    )
    .await;
    if !verdict.allowed {
        let reason = verdict
            .reason
            .unwrap_or_else(|| "[GOVERNANCE] denied".to_owned());
        session.emit(super::events::PiEventBody::ToolBlocked {
            tool_use_id: None,
            tool_name,
            reason: reason.clone(),
            policy: verdict.policy.clone(),
        });
        // 200 with the refusal as the tool's own text, not an HTTP error: this
        // is an answer the model must read and explain, and a 403 would reach
        // it as "the tool broke" with the reason discarded.
        return Json(McpCallResult {
            text: reason,
            ok: false,
        })
        .into_response();
    }

    let endpoint = registry.config().mcp_url.clone();
    match hub::forward(&endpoint, &session, &body.tool, &body.arguments).await {
        Ok(result) => Json(result).into_response(),
        Err(e) => {
            tracing::warn!(
                tool = %body.tool,
                endpoint = %endpoint,
                error = %e,
                "pi MCP proxy call failed"
            );
            problem(StatusCode::BAD_GATEWAY, "the documentation hub is unreachable")
        },
    }
}
