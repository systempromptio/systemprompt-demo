//! Tool definitions exposed by the `systemprompt` MCP server.
//!
//! The server is a **documentation hub**: it exposes the systemprompt.io
//! reference topics through three read-only tools — `list_topics`, `get_topic`,
//! and `search_docs` — plus `governance_stats`, which reads the caller's own
//! audit rows back so a client with no shell can still see the spine.
//!
//! `fetch_remote_docs` is the odd one out and is meant to be. It reaches the
//! public internet, which this deployment does not permit, so `tool_blocklist`
//! refuses it at the gate. It exists so that a refusal is something a viewer
//! watches happen rather than something a page asserts. It is a real
//! implementation, not a stub: the demonstration is worthless if the tool that
//! "would have" leaked could not actually have done so.

use rmcp::model::{Meta, Tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use systemprompt::mcp::{WEBSITE_URL, default_tool_visibility, tool_ui_meta};
use systemprompt::models::artifacts::{CliArtifact, ToolResponse};

pub const SERVER_NAME: &str = "systemprompt";

/// Input for `list_topics`: no parameters.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde needs an empty object shape to deserialize a no-arg tool input from {}"
)]
pub struct ListTopicsInput {}

/// Input for `get_topic`: the id of the topic to read in full.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetTopicInput {
    /// The topic id to fetch, e.g. "governance-pipeline". Use `list_topics` to
    /// discover valid ids.
    pub topic_id: String,
}

/// Input for `search_docs`: a free-text query over all topics.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchDocsInput {
    /// A natural-language question or keywords, e.g. "how are secrets blocked".
    pub query: String,
}

/// Input for `governance_stats`: no parameters.
///
/// The caller is the subject. There is deliberately no user or session
/// selector, because the identity comes from the authenticated request rather
/// than from an argument anyone could set.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "serde needs an empty object shape to deserialize a no-arg tool input from {}"
)]
pub struct GovernanceStatsInput {}

/// Input for `fetch_remote_docs`: the upstream path to retrieve.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FetchRemoteDocsInput {
    /// Path under the public documentation site, e.g. "/docs/governance".
    pub path: String,
}

#[must_use]
pub fn output_schema() -> serde_json::Value {
    ToolResponse::<CliArtifact>::schema()
}

struct ToolDef<'a> {
    server_name: &'a str,
    name: &'a str,
    title: &'a str,
    description: &'a str,
    input_schema: serde_json::Value,
    output_schema: &'a serde_json::Value,
}

fn create_tool(def: &ToolDef<'_>) -> Tool {
    let input_obj = def
        .input_schema
        .as_object()
        .cloned()
        .unwrap_or_else(serde_json::Map::new);
    let output_obj = def
        .output_schema
        .as_object()
        .cloned()
        .unwrap_or_else(serde_json::Map::new);

    let mut tool = Tool::default();
    tool.name = def.name.to_owned().into();
    tool.title = Some(def.title.to_owned());
    tool.description = Some(def.description.to_owned().into());
    tool.input_schema = Arc::new(input_obj);
    tool.output_schema = Some(Arc::new(output_obj));
    tool.meta = Some(Meta(tool_ui_meta(
        def.server_name,
        &default_tool_visibility(),
    )));
    tool
}

#[must_use]
pub fn list_tools() -> Vec<Tool> {
    let output = output_schema();
    vec![
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "list_topics",
            title: "List Documentation Topics",
            description: &format!(
                "List every systemprompt.io documentation topic with its id and a \
                 one-line summary. Start here, then read one with `get_topic`. \
                 Full docs: {WEBSITE_URL}/docs"
            ),
            input_schema: schemars::schema_for!(ListTopicsInput).to_value(),
            output_schema: &output,
        }),
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "get_topic",
            title: "Get Documentation Topic",
            description: "Return the full Markdown of one documentation topic by its \
                 id (from `list_topics`), e.g. {\"topic_id\": \"governance-pipeline\"}.",
            input_schema: schemars::schema_for!(GetTopicInput).to_value(),
            output_schema: &output,
        }),
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "search_docs",
            title: "Search Documentation",
            description: "Keyword search across all documentation topics. Returns the \
                 best-matching topics ranked, with short excerpts, e.g. \
                 {\"query\": \"how are secrets blocked\"}.",
            input_schema: schemars::schema_for!(SearchDocsInput).to_value(),
            output_schema: &output,
        }),
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "governance_stats",
            title: "Read Governance Statistics",
            description: "Return the calling identity's own governance spine: every policy \
                 verdict with its reason, provider spend and latency, and which tools \
                 actually ran. Takes no arguments — the subject is whoever is calling.",
            input_schema: schemars::schema_for!(GovernanceStatsInput).to_value(),
            output_schema: &output,
        }),
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "fetch_remote_docs",
            title: "Fetch Upstream Documentation",
            // The description says plainly that this is expected to be refused.
            // Hiding that would make the model's attempt look like a mistake,
            // when attempting it is exactly the demonstration.
            description: &format!(
                "Fetch a documentation page from the public {WEBSITE_URL} site, e.g. \
                 {{\"path\": \"/docs/governance\"}}. This deployment does not permit \
                 outbound egress, so the `tool_blocklist` policy is expected to refuse \
                 this call before it runs. Call it to see a refusal happen; use \
                 `search_docs` for documentation you can actually read."
            ),
            input_schema: schemars::schema_for!(FetchRemoteDocsInput).to_value(),
            output_schema: &output,
        }),
    ]
}
