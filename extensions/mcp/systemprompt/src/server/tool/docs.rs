//! The three handlers that answer out of compiled-in documentation.
//!
//! Topic content is baked into the binary by [`crate::topics`], so these never
//! touch the database and never leave the process. That is what makes them the
//! tools a session can always use, whatever the deployment's egress posture.

use crate::tools::{GetTopicInput, ListTopicsInput, SearchDocsInput};
use crate::topics;
use rmcp::ErrorData as McpError;
use std::future::{self, Future};
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::McpToolHandler;
use systemprompt::models::artifacts::CliArtifact;
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::text_artifact;

pub(in crate::server) struct ListTopicsHandler;

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

pub(in crate::server) struct GetTopicHandler;

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

pub(in crate::server) struct SearchDocsHandler;

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
