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

use super::render::{self, McpCallResult, first_frame, render};
use super::{AGENT_NAME, PROTOCOL_VERSION, TOKEN_TTL_HOURS};
use crate::session::PiSession;

pub(super) async fn forward(
    endpoint: &str,
    session: &Arc<PiSession>,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<(McpCallResult, Option<String>), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let trace_id = format!("pi-mcp-{}", uuid::Uuid::new_v4());
    let token = mint_hub_token(session).map_err(|e| e.to_string())?;
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
    Ok((render(&payload), render::artifact_id(&payload)))
}

struct Identity<'a> {
    user_id: &'a str,
    session_id: &'a str,
    trace_id: &'a str,
    token: &'a str,
}

#[derive(Debug, thiserror::Error)]
enum HubTokenError {
    #[error("no config: {0}")]
    Config(#[from] systemprompt::models::errors::ConfigError),
    #[error("user id is not a uuid: {0}")]
    UserId(#[from] uuid::Error),
    #[error("could not mint an mcp token: {0}")]
    Mint(#[from] systemprompt::oauth::OauthError),
}

fn mint_hub_token(session: &Arc<PiSession>) -> Result<String, HubTokenError> {
    let issuer = Config::get()?.jwt_issuer.clone();
    let id = uuid::Uuid::parse_str(session.user_id.as_str())?;
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
    .map_err(HubTokenError::Mint)
}

struct HubReply {
    // Why: an opaque MCP-transport token whose format is owned by the MCP
    // spec and the hub, so it deliberately stays a String, not a typed id
    mcp_session_id: Option<String>,
    payload: Option<render::McpResponseFrame>,
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
