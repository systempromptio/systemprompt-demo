//! Tool handlers, authentication, and dispatch for the `systemprompt` MCP
//! documentation hub.
//!
//! The server in the parent module owns the rmcp `ServerHandler` surface; this
//! module owns what happens per tool call: RBAC enforcement against the
//! registry, access auditing, and turning each tool's answer into the typed
//! artifact its shape calls for — lists for indexes, tables for audit rows,
//! text for documents.

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RequestContext, RoleServer};
use systemprompt::database::DbPool;
use systemprompt::mcp::middleware::enforce_rbac_from_registry;
use systemprompt::mcp::{ClientProfile, McpToolExecutor};
use systemprompt::models::artifacts::{CliArtifact, TextArtifact};
use systemprompt::models::execution::context::RequestContext as SysRequestContext;
use systemprompt::security::authz::SharedAuthzHook;
use systemprompt_mcp_shared::{McpAccess, record_mcp_access, record_mcp_access_rejected};

const REMOTE_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

const REMOTE_FETCH_HOST: &str = "systemprompt.io";
const REMOTE_FETCH_PORT: u16 = 443;

fn text_artifact(title: &str, body: impl Into<String>) -> CliArtifact {
    CliArtifact::text(TextArtifact::new(body).with_title(title))
}

mod admin_audit_dump;
mod docs;
mod egress;
mod governance_stats;
mod render_artifact;
mod render_spine;
mod safety_findings;
pub(crate) mod site_pages;

pub(super) use admin_audit_dump::AdminAuditDumpHandler;
pub(super) use docs::{GetTopicHandler, ListTopicsHandler, SearchDocsHandler};
pub(super) use egress::FetchRemoteDocsHandler;
pub(super) use governance_stats::GovernanceStatsHandler;
pub(super) use render_artifact::RenderArtifactHandler;
pub(super) use safety_findings::SafetyFindingsHandler;
pub(super) use site_pages::{FetchSitePageHandler, ListSitePagesHandler};

fn db_error(e: &sqlx::Error) -> McpError {
    tracing::error!(error = %e, "governance spine query failed");
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

const TOOL_NAMES: &str = "list_topics, get_topic, search_docs, list_site_pages, fetch_site_page, \
                          governance_stats, safety_findings, render_artifact, admin_audit_dump, \
                          fetch_remote_docs";

fn unknown_tool(tool_name: &str) -> McpError {
    McpError::invalid_params(
        format!(
            "Unknown tool: '{tool_name}'. Available tools: {TOOL_NAMES}. Call `list_topics` \
             first to see the documentation topics."
        ),
        None,
    )
}

fn clone_pool(db_pool: &DbPool) -> DbPool {
    std::sync::Arc::<systemprompt::database::Database>::clone(db_pool)
}

pub(super) struct Dispatch<'a> {
    pub(super) executor: &'a McpToolExecutor,
    pub(super) db_pool: &'a DbPool,
    pub(super) request: &'a CallToolRequestParams,
    pub(super) request_context: &'a SysRequestContext,
    pub(super) client: &'a ClientProfile,
}

pub(super) async fn dispatch_tool(
    ctx: &Dispatch<'_>,
    tool_name: &str,
) -> Result<CallToolResult, McpError> {
    let Dispatch {
        executor,
        db_pool,
        request,
        request_context,
        client,
    } = *ctx;
    match tool_name {
        "list_topics" => {
            executor
                .execute(&ListTopicsHandler, request, request_context, client)
                .await
        },
        "get_topic" => {
            executor
                .execute(&GetTopicHandler, request, request_context, client)
                .await
        },
        "search_docs" => {
            executor
                .execute(&SearchDocsHandler, request, request_context, client)
                .await
        },
        "list_site_pages" => {
            executor
                .execute(&ListSitePagesHandler, request, request_context, client)
                .await
        },
        "fetch_site_page" => {
            executor
                .execute(&FetchSitePageHandler, request, request_context, client)
                .await
        },
        "governance_stats" => {
            let handler = GovernanceStatsHandler {
                db_pool: clone_pool(db_pool),
            };
            executor
                .execute(&handler, request, request_context, client)
                .await
        },
        "safety_findings" => {
            let handler = SafetyFindingsHandler {
                db_pool: clone_pool(db_pool),
            };
            executor
                .execute(&handler, request, request_context, client)
                .await
        },
        "render_artifact" => {
            let handler = RenderArtifactHandler {
                db_pool: clone_pool(db_pool),
            };
            executor
                .execute(&handler, request, request_context, client)
                .await
        },
        "admin_audit_dump" => {
            let handler = AdminAuditDumpHandler {
                db_pool: clone_pool(db_pool),
            };
            executor
                .execute(&handler, request, request_context, client)
                .await
        },
        "fetch_remote_docs" => {
            executor
                .execute(&FetchRemoteDocsHandler, request, request_context, client)
                .await
        },
        _ => Err(unknown_tool(tool_name)),
    }
}
