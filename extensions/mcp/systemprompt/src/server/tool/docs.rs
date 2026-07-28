//! The three handlers that answer out of compiled-in documentation.
//!
//! Topic content is baked into the binary by [`crate::topics`], so these never
//! touch the database and never leave the process. That is what makes them the
//! tools a session can always use, whatever the deployment's egress posture.

use crate::tool_inputs::{GetTopicInput, ListTopicsInput, SearchDocsInput};
use crate::topics;
use rmcp::ErrorData as McpError;
use std::future::{self, Future};
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::McpToolHandler;
use systemprompt::mcp::WEBSITE_URL;
use systemprompt::models::artifacts::{CliArtifact, ListArtifact, ListItem};
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::text_artifact;

// Why: the list renderer reads `description`, not `summary` — set both so the
// item reads the same in the shelf preview and in the raw JSON. The link goes
// to the public docs; the topic itself is read with `get_topic`, and the slug
// field carries the id to pass it.
fn topic_item(topic: &topics::Topic, description: &str) -> ListItem {
    ListItem::new(topic.title, description, format!("{WEBSITE_URL}/docs"))
        .with_id(topic.id)
        .with_slug(topic.id)
        .with_description(description)
        .with_category("documentation")
}

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
        let items: Vec<ListItem> = topics::TOPICS
            .iter()
            .map(|topic| topic_item(topic, topic.summary))
            .collect();
        let summary = format!(
            "{} documentation topics available; read one with `get_topic {{\"topic_id\": \
             \"<id>\"}}` or search with `search_docs`",
            topics::TOPICS.len()
        );
        future::ready(Ok((
            CliArtifact::list(ListArtifact::new().with_items(items)),
            summary,
        )))
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

        let items: Vec<ListItem> = hits
            .iter()
            .map(|hit| {
                let description =
                    format!("{} — {}", hit.topic.summary, topics::excerpt(hit.topic, &terms));
                topic_item(hit.topic, &description)
            })
            .collect();

        let summary = if hits.is_empty() {
            format!(
                "no topics matched \"{}\" — try broader terms, or `list_topics` to browse \
                 everything",
                input.query.trim()
            )
        } else {
            format!(
                "{} topic(s) matched \"{}\"; read one in full with `get_topic {{\"topic_id\": \
                 \"<id>\"}}`",
                hits.len(),
                input.query.trim()
            )
        };
        future::ready(Ok((
            CliArtifact::list(ListArtifact::new().with_items(items)),
            summary,
        )))
    }
}
