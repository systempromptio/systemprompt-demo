//! The `systemprompt` MCP server: struct construction and rmcp `ServerHandler`
//! surface (info, tool listing, call dispatch, artifact-viewer resources).
//!
//! Per-call logic (RBAC, auditing, CLI-to-artifact conversion) lives in
//! the `tool` submodule.

mod tool;

use crate::error::SystempromptToolError;
use crate::tools::{self, SERVER_NAME};
use crate::topics;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Icon, Implementation, InitializeRequestParams,
    InitializeResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
    ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::service::{MaybeSendFuture, RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler};
use std::future::Future;
use std::sync::Arc;
use systemprompt::database::DbPool;
use systemprompt::identifiers::McpServerId;
use systemprompt::mcp::repository::ToolUsageRepository;
use systemprompt::mcp::{
    ArtifactViewerConfig, McpArtifactRepository, McpToolExecutor, WEBSITE_URL,
    build_artifact_viewer_resource, build_extension_capabilities, read_artifact_viewer_resource,
};
use systemprompt::security::authz::SharedAuthzHook;
use systemprompt_mcp_shared::{McpAccess, record_mcp_access};

use tool::{authenticate_tool_request, dispatch_tool};

const ARTIFACT_VIEWER_TEMPLATE: &str = include_str!("../../templates/artifact-viewer.html");

/// URI prefix under which each documentation topic is exposed as an MCP
/// `text/markdown` resource: `systemprompt://docs/<topic-id>`.
const DOCS_URI_PREFIX: &str = "systemprompt://docs/";

#[derive(Clone, Debug)]
pub struct SystempromptServer {
    service_id: McpServerId,
    db_pool: DbPool,
    executor: McpToolExecutor,
    authz_hook: SharedAuthzHook,
}

impl SystempromptServer {
    pub fn new(
        db_pool: DbPool,
        service_id: McpServerId,
        authz_hook: SharedAuthzHook,
    ) -> Result<Self, SystempromptToolError> {
        let tool_usage_repo = Arc::new(
            ToolUsageRepository::new(&db_pool)
                .map_err(|e| SystempromptToolError::Internal(e.to_string()))?,
        );
        let artifact_repo = Arc::new(
            McpArtifactRepository::new(&db_pool)
                .map_err(|e| SystempromptToolError::Internal(e.to_string()))?,
        );
        let executor = McpToolExecutor::new(tool_usage_repo, artifact_repo, SERVER_NAME);

        Ok(Self {
            service_id,
            db_pool,
            executor,
            authz_hook,
        })
    }
}

impl ServerHandler for SystempromptServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_extensions_with(build_extension_capabilities())
                .build(),
        )
        .with_protocol_version(ProtocolVersion::V_2024_11_05)
        .with_server_info(
            Implementation::new(
                format!("SystemPrompt ({})", self.service_id),
                env!("CARGO_PKG_VERSION"),
            )
            .with_title("systemprompt.io Docs")
            .with_icons(vec![
                Icon::new(format!("{WEBSITE_URL}/files/images/favicon-32x32.png"))
                    .with_mime_type("image/png")
                    .with_sizes(vec!["32x32".to_owned()]),
                Icon::new(format!("{WEBSITE_URL}/files/images/favicon-96x96.png"))
                    .with_mime_type("image/png")
                    .with_sizes(vec!["96x96".to_owned()]),
            ])
            .with_website_url(WEBSITE_URL),
        )
        .with_instructions(format!(
            "The systemprompt.io documentation hub. Call `list_topics` to see every \
             reference topic, `get_topic {{\"topic_id\": \"<id>\"}}` to read one in full, and \
             `search_docs {{\"query\": \"...\"}}` to search. Topics are also available as \
             resources under systemprompt://docs/<id>. `governance_stats` returns your own \
             policy verdicts and spend. `fetch_remote_docs` reaches the public internet and \
             is expected to be refused by policy — it exists to demonstrate a refusal, not \
             to be a working fetch. Full documentation: {WEBSITE_URL}/docs"
        ))
    }

    fn initialize(
        &self,
        _request: InitializeRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<InitializeResult, McpError>> + MaybeSendFuture + '_ {
        tracing::info!("systemprompt.io MCP server initialized");
        std::future::ready(Ok(self.get_info()))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + MaybeSendFuture + '_ {
        let tool_list = tools::list_tools();
        std::future::ready(Ok(ListToolsResult::with_all_items(tool_list)))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let tool_name = request.name.to_string();
        let server_name = self.service_id.to_string();

        let (request_context, _auth_token) = authenticate_tool_request(
            &self.db_pool,
            &tool_name,
            self.service_id.as_str(),
            &ctx,
            &self.authz_hook,
        )
        .await?;

        record_mcp_access(
            &self.db_pool,
            &McpAccess {
                user_id: request_context.user_id(),
                session_id: request_context.session_id(),
                server: &server_name,
                tool: &tool_name,
                action: "used",
            },
        )
        .await;

        dispatch_tool(
            &self.executor,
            &self.db_pool,
            &tool_name,
            &request,
            &request_context,
        )
        .await
        .map(Into::into)
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpError>> + MaybeSendFuture + '_ {
        let mut result = build_artifact_viewer_resource(&ArtifactViewerConfig {
            server_name: SERVER_NAME,
            title: "systemprompt.io Artifact Viewer",
            description: "Interactive UI viewer for systemprompt.io artifacts. Renders tables, lists, \
                         and text content with syntax highlighting. Template receives artifact data \
                         dynamically via MCP Apps ui/notifications/tool-result protocol.",
            template: ARTIFACT_VIEWER_TEMPLATE,
            icons: Some(vec![
                Icon::new(format!("{WEBSITE_URL}/files/images/favicon-32x32.png"))
                    .with_mime_type("image/png")
                    .with_sizes(vec!["32x32".to_owned()]),
            ]),
        });

        for topic in topics::TOPICS {
            result.resources.push(
                Resource::new(
                    format!("{DOCS_URI_PREFIX}{}", topic.id),
                    topic.title.to_owned(),
                )
                .with_title(topic.title.to_owned())
                .with_description(topic.summary.to_owned())
                .with_mime_type("text/markdown".to_owned())
                .with_size(u64::try_from(topic.body.len()).unwrap_or(u64::MAX)),
            );
        }

        std::future::ready(Ok(result))
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResponse, McpError>> + MaybeSendFuture + '_ {
        if let Some(topic_id) = request.uri.strip_prefix(DOCS_URI_PREFIX) {
            let result = match topics::find(topic_id) {
                Some(topic) => {
                    Ok(
                        ReadResourceResult::new(vec![ResourceContents::TextResourceContents {
                            uri: request.uri.clone(),
                            mime_type: Some("text/markdown".to_owned()),
                            text: topic.body.to_owned(),
                            meta: None,
                        }])
                        .into(),
                    )
                },
                None => Err(McpError::invalid_params(
                    format!("Unknown documentation topic resource: {}", request.uri),
                    None,
                )),
            };
            return std::future::ready(result);
        }

        std::future::ready(
            read_artifact_viewer_resource(&request, SERVER_NAME, ARTIFACT_VIEWER_TEMPLATE)
                .map(Into::into),
        )
    }
}
