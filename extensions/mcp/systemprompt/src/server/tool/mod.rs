//! Tool handlers, authentication, and dispatch for the `systemprompt` MCP
//! documentation hub.
//!
//! The server in the parent module owns the rmcp `ServerHandler` surface; this
//! module owns what happens per tool call: RBAC enforcement against the
//! registry, access auditing, and turning topic content into text artifacts.

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RequestContext, RoleServer};
use systemprompt::database::DbPool;
use systemprompt::mcp::middleware::enforce_rbac_from_registry;
use systemprompt::mcp::McpToolExecutor;
use systemprompt::models::artifacts::{CliArtifact, TextArtifact};
use systemprompt::models::execution::context::RequestContext as SysRequestContext;
use systemprompt::security::authz::SharedAuthzHook;
use systemprompt_mcp_shared::{McpAccess, record_mcp_access, record_mcp_access_rejected};

/// How long `fetch_remote_docs` waits before giving up. Short on purpose: where
/// the boundary is a firewall rather than a refusal the failure mode is a hang,
/// and a demo tool that appears to freeze teaches the wrong lesson about what
/// stopped it.
const REMOTE_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Host and port `fetch_remote_docs` dials. Deliberately the public docs site
/// on 443 — the thing a deployment with no egress must not be able to reach.
const REMOTE_FETCH_HOST: &str = "systemprompt.io";
const REMOTE_FETCH_PORT: u16 = 443;

fn text_artifact(title: &str, body: impl Into<String>) -> CliArtifact {
    CliArtifact::text(TextArtifact::new(body).with_title(title))
}

mod docs;
mod egress;
mod governance_stats;

pub(super) use docs::{GetTopicHandler, ListTopicsHandler, SearchDocsHandler};
pub(super) use egress::FetchRemoteDocsHandler;
pub(super) use governance_stats::GovernanceStatsHandler;

fn db_error(e: &sqlx::Error) -> McpError {
    tracing::error!(error = %e, "governance_stats query failed");
    McpError::internal_error("Could not read the governance spine.", None)
}

pub(super) async fn authenticate_tool_request(
    db_pool: &DbPool,
    tool_name: &str,
    service_id: &str,
    ctx: &RequestContext<RoleServer>,
    authz_hook: &SharedAuthzHook,
) -> Result<(SysRequestContext, String), McpError> {
    let server_name = service_id;
    let rbac_result = enforce_rbac_from_registry(ctx, service_id, authz_hook).await;

    match rbac_result {
        Ok(result) => {
            match result
                .expect_authenticated("BUG: systemprompt requires OAuth but auth was not enforced")
            {
                Ok(authenticated) => {
                    record_mcp_access(
                        db_pool,
                        &McpAccess {
                            user_id: authenticated.context.user_id(),
                            session_id: authenticated.context.session_id(),
                            server: server_name,
                            tool: tool_name,
                            action: "authenticated",
                        },
                    )
                    .await;
                    let token = authenticated.token().to_owned();
                    Ok((authenticated.context.clone(), token))
                },
                Err(e) => {
                    record_mcp_access_rejected(db_pool, server_name, tool_name, e.message.as_ref())
                        .await;
                    Err(e)
                },
            }
        },
        Err(e) => {
            record_mcp_access_rejected(db_pool, server_name, tool_name, &format!("{e}")).await;
            Err(e)
        },
    }
}

pub(super) async fn dispatch_tool(
    executor: &McpToolExecutor,
    db_pool: &DbPool,
    tool_name: &str,
    request: &CallToolRequestParams,
    request_context: &SysRequestContext,
) -> Result<CallToolResult, McpError> {
    match tool_name {
        "list_topics" => {
            executor
                .execute(&ListTopicsHandler, request, request_context)
                .await
        },
        "get_topic" => {
            executor
                .execute(&GetTopicHandler, request, request_context)
                .await
        },
        "search_docs" => {
            executor
                .execute(&SearchDocsHandler, request, request_context)
                .await
        },
        "governance_stats" => {
            executor
                .execute(
                    &GovernanceStatsHandler {
                        db_pool: std::sync::Arc::<systemprompt::database::Database>::clone(db_pool),
                    },
                    request,
                    request_context,
                )
                .await
        },
        "fetch_remote_docs" => {
            executor
                .execute(&FetchRemoteDocsHandler, request, request_context)
                .await
        },
        _ => Err(McpError::invalid_params(
            format!(
                "Unknown tool: '{tool_name}'. Available tools: list_topics, get_topic, \
                 search_docs, governance_stats, fetch_remote_docs. Call `list_topics` \
                 first to see the documentation topics."
            ),
            None,
        )),
    }
}
