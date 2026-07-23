//! Tool handlers, authentication, and dispatch for the `systemprompt` MCP
//! documentation hub.
//!
//! The server in the parent module owns the rmcp `ServerHandler` surface; this
//! module owns what happens per tool call: RBAC enforcement against the
//! registry, access auditing, and turning topic content into text artifacts.

use crate::tools::{GetTopicInput, ListTopicsInput, SearchDocsInput};
use crate::topics;
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RequestContext, RoleServer};
use std::future::{self, Future};
use systemprompt::database::DbPool;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::middleware::enforce_rbac_from_registry;
use systemprompt::mcp::{McpToolExecutor, McpToolHandler};
use systemprompt::models::artifacts::{CliArtifact, TextArtifact};
use systemprompt::models::execution::context::RequestContext as SysRequestContext;
use systemprompt::security::authz::SharedAuthzHook;
use systemprompt_mcp_shared::{record_mcp_access, record_mcp_access_rejected};

fn text_artifact(title: &str, body: impl Into<String>) -> CliArtifact {
    CliArtifact::text(TextArtifact::new(body).with_title(title))
}

// Why: `list_topics` — enumerate every documentation topic.
pub(super) struct ListTopicsHandler;

impl McpToolHandler for ListTopicsHandler {
    type Input = ListTopicsInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "list_topics"
    }

    fn description(&self) -> &'static str {
        "List every systemprompt.io documentation topic."
    }

    fn handle(
        &self,
        _input: Self::Input,
        _ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let mut body = String::from("# systemprompt.io documentation topics\n\n");
        for topic in topics::TOPICS {
            body.push_str(&format!(
                "- **{}** (`{}`) — {}\n",
                topic.title, topic.id, topic.summary
            ));
        }
        body.push_str(
            "\nRead one with `get_topic {\"topic_id\": \"<id>\"}` or search with `search_docs`.\n",
        );
        let summary = format!("{} documentation topics available", topics::TOPICS.len());
        future::ready(Ok((text_artifact("Documentation Topics", body), summary)))
    }
}

// Why: `get_topic` — return the full Markdown of one topic.
pub(super) struct GetTopicHandler;

impl McpToolHandler for GetTopicHandler {
    type Input = GetTopicInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "get_topic"
    }

    fn description(&self) -> &'static str {
        "Return the full Markdown of one documentation topic by id."
    }

    fn handle(
        &self,
        input: Self::Input,
        _ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let Some(topic) = topics::find(&input.topic_id) else {
            let ids: Vec<&str> = topics::TOPICS.iter().map(|t| t.id).collect();
            return future::ready(Err(McpError::invalid_params(
                format!(
                    "Unknown topic '{}'. Valid topic ids: {}. Call `list_topics` to see them.",
                    input.topic_id,
                    ids.join(", ")
                ),
                None,
            )));
        };
        let summary = format!("Topic: {}", topic.title);
        future::ready(Ok((text_artifact(topic.title, topic.body), summary)))
    }
}

// Why: `search_docs` — keyword search across all topics.
pub(super) struct SearchDocsHandler;

impl McpToolHandler for SearchDocsHandler {
    type Input = SearchDocsInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "search_docs"
    }

    fn description(&self) -> &'static str {
        "Keyword search across all documentation topics."
    }

    fn handle(
        &self,
        input: Self::Input,
        _ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let terms: Vec<String> = input
            .query
            .split_whitespace()
            .map(|t| {
                t.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .filter(|t| t.len() >= 2)
            .collect();

        let hits = topics::search(&input.query);

        let mut body = format!("# Search results for: {}\n\n", input.query.trim());
        if hits.is_empty() {
            body.push_str(
                "No topics matched. Try broader terms, or call `list_topics` to browse everything.\n",
            );
        } else {
            for hit in &hits {
                body.push_str(&format!(
                    "## {} (`{}`)\n\n{}\n\n> {}\n\n",
                    hit.topic.title,
                    hit.topic.id,
                    hit.topic.summary,
                    topics::excerpt(hit.topic, &terms)
                ));
            }
            body.push_str("Read any of these in full with `get_topic {\"topic_id\": \"<id>\"}`.\n");
        }

        let summary = format!("{} topic(s) matched \"{}\"", hits.len(), input.query.trim());
        future::ready(Ok((text_artifact("Search Results", body), summary)))
    }
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
                        authenticated.context.user_id(),
                        server_name,
                        tool_name,
                        "authenticated",
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
        _ => Err(McpError::invalid_params(
            format!(
                "Unknown tool: '{tool_name}'. Available tools: list_topics, get_topic, \
                 search_docs. Call `list_topics` first to see the documentation topics."
            ),
            None,
        )),
    }
}
