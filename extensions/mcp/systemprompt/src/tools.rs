//! Tool definitions exposed by the `systemprompt` MCP server.
//!
//! The server is a **documentation hub**: it exposes the systemprompt.io
//! reference topics through three read-only tools — `list_topics`, `get_topic`,
//! and `search_docs` — plus two readbacks, `governance_stats` and
//! `safety_findings`, which return the caller's own audit rows so a client with
//! no shell can still see the spine. They read different layers:
//! `governance_stats` reports the tool-call gate, `safety_findings` the
//! gateway's scan of conversation content on the way to a provider.
//!
//! `render_artifact` is the showcase tool: it emits one artifact of any
//! requested visual type — table, chart, dashboard, list, card, message,
//! copy-paste text, or text — so the terminal's artifact shelf can be
//! demonstrated end-to-end through the same governed path as every other call.
//!
//! `fetch_remote_docs` and `admin_audit_dump` are the odd ones out, and are
//! meant to be. Each exists to be refused by a different policy —
//! `tool_blocklist` for the first, which reaches an internet this deployment
//! does not permit; `scope_check` for the second, whose `admin_` prefix holds
//! it to a scope this terminal never grants. They exist so that a refusal is
//! something a viewer watches happen rather than something a page asserts, and
//! both are real implementations rather than stubs: the demonstration is
//! worthless if the tool that "would have" leaked could not actually have done
//! so.

use crate::tool_inputs::{
    AdminAuditDumpInput, FetchRemoteDocsInput, FetchSitePageInput, GetTopicInput,
    GovernanceStatsInput, ListSitePagesInput, ListTopicsInput, RenderArtifactInput,
    SafetyFindingsInput, SearchDocsInput,
};
use rmcp::model::{Meta, Tool};
use std::sync::Arc;
use systemprompt::mcp::{WEBSITE_URL, default_tool_visibility, tool_ui_meta};
use systemprompt::models::artifacts::{CliArtifact, ToolResponse};

pub const SERVER_NAME: &str = "systemprompt";

#[must_use]
pub fn output_schema() -> serde_json::Value {
    ToolResponse::<CliArtifact>::schema()
}

struct ToolDef<'a> {
    server_name: &'a str,
    name: &'a str,
    title: &'a str,
    description: &'a str,
    // JSON: MCP `Tool` schemas are protocol-defined `serde_json` shapes
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
    let mut tools = docs_tools(&output);
    tools.append(&mut site_tools(&output));
    tools.append(&mut readback_tools(&output));
    tools.append(&mut showcase_tools(&output));
    tools.append(&mut refusal_tools(&output));
    tools
}

fn showcase_tools(output: &serde_json::Value) -> Vec<Tool> {
    vec![create_tool(&ToolDef {
        server_name: SERVER_NAME,
        name: "render_artifact",
        title: "Render a Demo Artifact",
        description: "Render one artifact of the requested type so the terminal's \
             artifact shelf can be seen working, e.g. {\"artifact_type\": \"chart\"}. \
             Valid types: table, chart, list, dashboard, presentation_card, message, \
             copy_paste_text, text. The table, chart, and dashboard variants are built \
             from the calling session's own governance spine — the same rows \
             `governance_stats` reports — while the rest carry curated content about \
             systemprompt.io.",
        input_schema: schemars::schema_for!(RenderArtifactInput).to_value(),
        output_schema: output,
    })]
}

fn docs_tools(output: &serde_json::Value) -> Vec<Tool> {
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
            output_schema: output,
        }),
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "get_topic",
            title: "Get Documentation Topic",
            description: "Return the full Markdown of one documentation topic by its \
                 id (from `list_topics`), e.g. {\"topic_id\": \"governance-pipeline\"}.",
            input_schema: schemars::schema_for!(GetTopicInput).to_value(),
            output_schema: output,
        }),
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "search_docs",
            title: "Search Documentation",
            description: "Keyword search across all documentation topics. Returns the \
                 best-matching topics ranked, with short excerpts, e.g. \
                 {\"query\": \"how are secrets blocked\"}.",
            input_schema: schemars::schema_for!(SearchDocsInput).to_value(),
            output_schema: output,
        }),
    ]
}

fn site_tools(output: &serde_json::Value) -> Vec<Tool> {
    vec![
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "list_site_pages",
            title: "List Live Site Pages",
            description: &format!(
                "List every public page of the live {WEBSITE_URL} site — documentation and \
                 blog — with the section and slug to read one via `fetch_site_page`. Unlike \
                 `list_topics`, which answers from documentation compiled into this server, \
                 this reads the site as it is published right now."
            ),
            input_schema: schemars::schema_for!(ListSitePagesInput).to_value(),
            output_schema: output,
        }),
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "fetch_site_page",
            title: "Fetch Live Site Page",
            description: &format!(
                "Fetch one page of the live {WEBSITE_URL} site as its source markdown, e.g. \
                 {{\"section\": \"documentation\", \"slug\": \"services/ai\"}}. Use \
                 `list_site_pages` to discover sections and slugs. The input is a section \
                 and slug, never a URL: the tool can only ever read {WEBSITE_URL}'s own \
                 markdown endpoint, which is why this egress is permitted while \
                 `fetch_remote_docs` is refused."
            ),
            input_schema: schemars::schema_for!(FetchSitePageInput).to_value(),
            output_schema: output,
        }),
    ]
}

fn readback_tools(output: &serde_json::Value) -> Vec<Tool> {
    vec![
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "governance_stats",
            title: "Read Governance Statistics",
            description: "Return the calling identity's own governance spine: every policy \
                 verdict with its reason, provider spend and latency, and which tools \
                 actually ran. Takes no arguments — the subject is whoever is calling.",
            input_schema: schemars::schema_for!(GovernanceStatsInput).to_value(),
            output_schema: output,
        }),
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "safety_findings",
            title: "Read Gateway Safety Findings",
            description: "Return the calling identity's own gateway safety findings: what the \
                 inference-path scanners caught in conversation content before a \
                 provider was reached, with phase, severity, category, and a \
                 redacted excerpt. Takes no arguments — the subject is whoever is \
                 calling. This is a different layer from the tool-input scan that \
                 `governance_stats` reports on.",
            input_schema: schemars::schema_for!(SafetyFindingsInput).to_value(),
            output_schema: output,
        }),
    ]
}

fn refusal_tools(output: &serde_json::Value) -> Vec<Tool> {
    vec![
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "admin_audit_dump",
            title: "Dump the Deployment Audit Spine",
            description: "Return every identity's governance decisions across the whole \
                 deployment — other people's user ids, sessions, and what they \
                 reached for. This is an administrative capability, and its name \
                 carries the `admin_` prefix the `scope_check` policy holds to \
                 admin scope, so this terminal is expected to refuse it before it \
                 runs. Call it to see a scope refusal happen; use \
                 `governance_stats` for the decisions you are entitled to read.",
            input_schema: schemars::schema_for!(AdminAuditDumpInput).to_value(),
            output_schema: output,
        }),
        create_tool(&ToolDef {
            server_name: SERVER_NAME,
            name: "fetch_remote_docs",
            title: "Fetch Upstream Documentation",
            description: &format!(
                "Fetch a documentation page from the public {WEBSITE_URL} site, e.g. \
                 {{\"path\": \"/docs/governance\"}}. This deployment does not permit \
                 outbound egress, so the `tool_blocklist` policy is expected to refuse \
                 this call before it runs. Call it to see a refusal happen; use \
                 `search_docs` for documentation you can actually read."
            ),
            input_schema: schemars::schema_for!(FetchRemoteDocsInput).to_value(),
            output_schema: output,
        }),
    ]
}
