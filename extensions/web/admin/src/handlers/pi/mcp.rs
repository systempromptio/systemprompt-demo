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
//! Governance is unaffected by any of this. The shim's `tool_call` hook fires
//! on extension-registered tools exactly as it does on `read`, so a call
//! reaching this endpoint has already cleared the policy chain and, where
//! configured, a human. This module deliberately registers no policy of its
//! own: a second opinion here would be a second place to get it wrong.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use systemprompt::models::Config;
use systemprompt::identifiers::SessionId;
use systemprompt::models::auth::{AuthenticatedUser, JwtAudience, Permission};
use systemprompt::oauth::services::{
    JwtConfig, JwtSigningParams, generate_access_token_jti, generate_jwt,
};

use super::auth::{authorize_session, problem, unauthorized};
use super::registry::PiRegistry;
use super::session::PiSession;

/// Tools the proxy will forward. An allowlist rather than a passthrough, for
/// the same reason `commands.rs` picks its RPC command by route: the child is
/// the least trusted thing in this system, and "whatever name it sent" is not a
/// property anyone can reason about later. Adding a hub tool means adding it
/// here, which is the point.
const FORWARDABLE: &[&str] = &[
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

#[derive(Debug, Serialize)]
struct McpCallResult {
    /// Rendered text the extension hands back to the model.
    text: String,
    /// True when the hub answered with a result rather than an error. The
    /// extension surfaces both — a tool error is information, not a failure of
    /// the transport.
    ok: bool,
}

pub(super) async fn call(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
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

    let endpoint = registry.config().mcp_url.clone();
    match forward(&endpoint, &session, &body.tool, &body.arguments).await {
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

/// Run one call against the hub: handshake, then `tools/call`.
///
/// The handshake is repeated per call rather than cached. The hub's session is
/// cheap on loopback, and holding one per conversation would add a second
/// lifetime to reason about beside the pi child's — with the failure mode that
/// a stale hub session outlives the conversation whose identity it was opened
/// under. Correctness over three saved round trips.
async fn forward(
    endpoint: &str,
    session: &Arc<PiSession>,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<McpCallResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // Identity is derived here and nowhere else. The hub takes both the headers
    // and the token at face value, so every part of them comes off the
    // authorized session rather than off the request body.
    let trace_id = format!("pi-mcp-{}", uuid::Uuid::new_v4());
    let token = mint_hub_token(session)?;
    let headers = Identity {
        user_id: session.user_id.as_str(),
        session_id: session.attested_session.as_str(),
        trace_id: &trace_id,
        token: &token,
    };

    let init = post(
        &client,
        endpoint,
        &headers,
        None,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "systemprompt-pi-widget", "version": "1" },
            },
        }),
    )
    .await?;

    let mcp_session = init
        .mcp_session_id
        .ok_or_else(|| "hub did not issue an mcp-session-id".to_owned())?;

    post(
        &client,
        endpoint,
        &headers,
        Some(&mcp_session),
        &serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await?;

    let called = post(
        &client,
        endpoint,
        &headers,
        Some(&mcp_session),
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments },
        }),
    )
    .await?;

    let payload = called
        .payload
        .ok_or_else(|| "hub returned no JSON-RPC frame".to_owned())?;
    Ok(render(&payload))
}

/// Identity headers the hub reads. Grouped so they cannot be passed in the
/// wrong order — three `&str` arguments in a row is a bug waiting to be typed.
struct Identity<'a> {
    user_id: &'a str,
    session_id: &'a str,
    trace_id: &'a str,
    /// Bearer token carrying the `mcp` audience, which the hub's RBAC requires
    /// on top of the headers above.
    token: &'a str,
}

/// Mint the short-lived `mcp`-audience token one hub call travels on.
///
/// Minted per call rather than held, and never handed to the child. The pi
/// session already holds a gateway PAT, but that credential carries no audience
/// and no roles — it is bearer-equivalent to the user for `/v1/*` and nothing
/// else — so the hub rejects it. This is the narrower credential the hub
/// actually asks for, scoped to one user, alive for the length of three
/// loopback requests.
fn mint_hub_token(session: &Arc<PiSession>) -> Result<String, String> {
    let issuer = Config::get()
        .map_err(|e| format!("no config: {e}"))?
        .jwt_issuer
        .clone();
    let id = uuid::Uuid::parse_str(session.user_id.as_str())
        .map_err(|e| format!("user id is not a uuid: {e}"))?;
    // Username and email are not read by the hub's RBAC — it resolves roles
    // from the database against `sub` — so they are set to something that names
    // where the token came from rather than to a value we would have to fetch.
    // `User` and nothing more. The hub's RBAC requires it, and the widget's
    // whole tool surface is open to any signed-in identity — an admin claim
    // here would silently exempt every terminal session from `scope_check` and
    // `tool_blocklist`, which are two of the four policies the demo exists to
    // show working.
    let permissions = vec![Permission::User];
    let user = AuthenticatedUser::new(
        id,
        AGENT_NAME.to_owned(),
        String::new(),
        permissions.clone(),
    );
    let config = JwtConfig {
        permissions,
        audience: vec![JwtAudience::Mcp],
        expires_in_hours: Some(TOKEN_TTL_HOURS),
        resource: None,
        plugin_id: None,
    };
    generate_jwt(
        &user,
        config,
        generate_access_token_jti(),
        &SessionId::new(session.attested_session.to_string()),
        &JwtSigningParams { issuer: &issuer },
    )
    .map_err(|e| format!("could not mint an mcp token: {e}"))
}

struct HubReply {
    /// The hub's `mcp-session-id` response header — an MCP transport handle,
    /// not a systemprompt session id. The two never mix: the governed session
    /// travels separately, in `Identity::session_id`.
    mcp_session_id: Option<String>,
    payload: Option<serde_json::Value>,
}

async fn post(
    client: &reqwest::Client,
    endpoint: &str,
    identity: &Identity<'_>,
    mcp_session: Option<&str>,
    payload: &serde_json::Value,
) -> Result<HubReply, String> {
    let mut request = client
        .post(endpoint)
        .header("content-type", "application/json")
        // The hub answers streamable-http, so it needs both offered.
        .header("accept", "application/json, text/event-stream")
        .header("authorization", format!("Bearer {}", identity.token))
        .header("x-user-id", identity.user_id)
        .header("x-session-id", identity.session_id)
        .header("x-trace-id", identity.trace_id)
        .header("x-agent-name", AGENT_NAME);
    if let Some(id) = mcp_session {
        request = request.header("mcp-session-id", id);
    }

    let response = request
        .json(payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let mcp_session_id = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned);
    let body = response.text().await.map_err(|e| e.to_string())?;
    Ok(HubReply {
        mcp_session_id,
        payload: first_frame(&body),
    })
}

/// Pull the first JSON-RPC frame out of an SSE body.
///
/// The hub replies `text/event-stream` even to a single request, so the frame
/// arrives as a `data:` line rather than as the whole body. A plain JSON body
/// is accepted too, so this keeps working if the hub ever stops streaming.
fn first_frame(body: &str) -> Option<serde_json::Value> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        return Some(value);
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<serde_json::Value>(data).ok())
        .find(|value| value.get("result").is_some() || value.get("error").is_some())
}

/// Turn a JSON-RPC frame into the text the model will read.
///
/// A hub error is returned as text with `ok: false` rather than as a transport
/// failure: "no such topic" is an answer the model should see and act on, and
/// turning it into a 502 would tell the model only that something broke.
fn render(frame: &serde_json::Value) -> McpCallResult {
    if let Some(error) = frame.get("error") {
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("the documentation hub refused the call");
        return McpCallResult {
            text: message.to_owned(),
            ok: false,
        };
    }

    let content = frame.pointer("/result/content").and_then(|c| c.as_array());
    let text = content.map_or_else(
        || frame.get("result").map(ToString::to_string).unwrap_or_default(),
        |items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        },
    );

    let is_error = frame
        .pointer("/result/isError")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    McpCallResult {
        text,
        ok: !is_error,
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "assertions in tests")]
mod tests {
    use super::{FORWARDABLE, first_frame, render};

    /// The allowlist is the whole of the proxy's authority over what the child
    /// can reach, so an accidental `*` would be silent.
    #[test]
    fn the_allowlist_is_explicit() {
        assert!(FORWARDABLE.contains(&"list_topics"));
        assert!(FORWARDABLE.contains(&"fetch_remote_docs"));
        assert!(!FORWARDABLE.contains(&"bash"));
        assert!(!FORWARDABLE.contains(&""));
    }

    #[test]
    fn reads_a_frame_out_of_an_sse_body() {
        // Shape captured from the hub: a keepalive `data:` line, then the frame.
        let body = "data: \nid: 0\nretry: 3000\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\
                    \"result\":{\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n";
        let frame = first_frame(body).expect("a frame");
        let rendered = render(&frame);
        assert_eq!(rendered.text, "hello");
        assert!(rendered.ok);
    }

    #[test]
    fn a_plain_json_body_still_parses() {
        let frame = first_frame(r#"{"jsonrpc":"2.0","id":2,"result":{"content":[]}}"#);
        assert!(frame.is_some());
    }

    /// A hub error must reach the model as readable text, not as a transport
    /// failure it cannot act on.
    #[test]
    fn an_error_frame_becomes_text() {
        let frame = first_frame(
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"Unknown topic 'x'"}}"#,
        )
        .expect("a frame");
        let rendered = render(&frame);
        assert!(!rendered.ok);
        assert!(rendered.text.contains("Unknown topic"), "{}", rendered.text);
    }
}
