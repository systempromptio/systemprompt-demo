//! Speaking to the hub: the handshake, the identity it travels under, and the
//! credential that carries it.
//!
//! Identity is derived here and nowhere else. The hub takes both the headers
//! and the token at face value, so every part of them comes off the authorized
//! [`PiSession`] rather than off the request body.

use std::sync::Arc;

use systemprompt::identifiers::SessionId;
use systemprompt::models::Config;
use systemprompt::models::auth::{AuthenticatedUser, JwtAudience, Permission};
use systemprompt::oauth::services::{
    JwtConfig, JwtSigningParams, generate_access_token_jti, generate_jwt,
};

use super::render::{McpCallResult, first_frame, render};
use super::{AGENT_NAME, PROTOCOL_VERSION, TOKEN_TTL_HOURS};
use crate::handlers::pi::session::PiSession;

/// Run one call against the hub: handshake, then `tools/call`.
///
/// The handshake is repeated per call rather than cached. The hub's session is
/// cheap on loopback, and holding one per conversation would add a second
/// lifetime to reason about beside the pi child's — with the failure mode that
/// a stale hub session outlives the conversation whose identity it was opened
/// under. Correctness over three saved round trips.
pub(super) async fn forward(
    endpoint: &str,
    session: &Arc<PiSession>,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<McpCallResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

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
