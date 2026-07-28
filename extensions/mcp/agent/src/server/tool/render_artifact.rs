//! `render_artifact` — one artifact of any visual type, on demand.
//!
//! The showcase handler behind the terminal's artifact shelf. The data-shaped
//! variants (table, chart, dashboard) live in [`super::render_spine`] and
//! answer from the caller's own governance spine; the curated variants below
//! carry fixed content about systemprompt.io.

use crate::tool_inputs::{DemoArtifactType, RenderArtifactInput};
use rmcp::ErrorData as McpError;
use std::future::Future;
use systemprompt::database::DbPool;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::{McpToolHandler, WEBSITE_URL};
use systemprompt::models::artifacts::{
    CardCta, CardSection, CliArtifact, CopyPasteTextArtifact, ListArtifact, ListItem,
    MessageArtifact, NoticeLine, PresentationCardArtifact, TextArtifact,
};
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::render_spine::{spine_chart, spine_dashboard, spine_table};

pub(in crate::server) struct RenderArtifactHandler {
    pub(in crate::server) db_pool: DbPool,
}

impl McpToolHandler for RenderArtifactHandler {
    type Input = RenderArtifactInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "render_artifact"
    }

    fn description(&self) -> &'static str {
        "Render one artifact of the requested type for the terminal's artifact shelf."
    }

    fn handle(
        &self,
        input: Self::Input,
        ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let db_pool = std::sync::Arc::<systemprompt::database::Database>::clone(&self.db_pool);
        let user_id = ctx.user_id().clone();
        let session_id = ctx.session_id().clone();
        async move {
            let kind = input.artifact_type;
            let artifact = match kind {
                DemoArtifactType::Table => spine_table(&db_pool, &user_id, &session_id).await?,
                DemoArtifactType::Chart => spine_chart(&db_pool, &user_id, &session_id).await?,
                DemoArtifactType::Dashboard => {
                    spine_dashboard(&db_pool, &user_id, &session_id).await?
                },
                DemoArtifactType::List => governance_stage_list(),
                DemoArtifactType::PresentationCard => product_card(),
                DemoArtifactType::Message => notice_message(),
                DemoArtifactType::CopyPasteText => setup_snippet(),
                DemoArtifactType::Text => explainer_text(),
            };
            Ok((artifact, format!("Rendered a {} artifact", kind.as_str())))
        }
    }
}

fn governance_stage_list() -> CliArtifact {
    let docs = format!("{WEBSITE_URL}/docs");
    // Why: the list renderer reads `description`, not `summary` — set both so
    // the item reads the same in the shelf preview and in the raw JSON.
    let stage = |title: &str, summary: &str| {
        ListItem::new(title, summary, docs.clone()).with_description(summary)
    };
    let items = vec![
        stage(
            "1. Scope check",
            "Holds each tool to the OAuth scope its name claims — `admin_` tools need admin.",
        ),
        stage(
            "2. Secret scan",
            "35+ credential patterns checked against every tool input before it runs.",
        ),
        stage(
            "3. Tool blocklist",
            "Denies tools this deployment forbids outright, like remote fetches.",
        ),
        stage(
            "4. Rate limit",
            "Caps call frequency per identity so a runaway loop cannot drain the budget.",
        ),
    ];
    CliArtifact::list(ListArtifact::new().with_items(items))
}

fn product_card() -> CliArtifact {
    CliArtifact::presentation_card(
        PresentationCardArtifact::new("systemprompt.io")
            .with_subtitle("Governance infrastructure for AI agents")
            .with_sections(vec![
                CardSection::new(
                    "What it is",
                    "A library you embed and own: every inference call and every MCP tool \
                     call passes a four-stage policy pipeline and lands in an audit spine \
                     you can query.",
                ),
                CardSection::new(
                    "Why it matters",
                    "A refusal is something a viewer watches happen in this terminal, not \
                     something a page asserts — the demo tools that get denied are real \
                     implementations.",
                ),
            ])
            .with_ctas(vec![CardCta::new(
                "read-docs",
                "Read the docs",
                "Call list_topics to browse the documentation from this terminal.",
                "primary",
            )]),
    )
}

fn notice_message() -> CliArtifact {
    CliArtifact::message(MessageArtifact::new(vec![
        NoticeLine::new(
            "info",
            "Every artifact on this shelf came through the same governed path: \
             scope check, secret scan, blocklist, rate limit — then the tool ran.",
        ),
        NoticeLine::new(
            "warning",
            "Denied calls land on the spine too. Ask for `admin_audit_dump` to watch one happen.",
        ),
        NoticeLine::new(
            "success",
            "This message artifact rendered from typed data, not from markdown.",
        ),
    ]))
}

fn setup_snippet() -> CliArtifact {
    CliArtifact::copy_paste_text(
        CopyPasteTextArtifact::new(
            "just setup-local <anthropic_key>\njust build\njust start\nsystemprompt --help",
        )
        .with_title("Run systemprompt.io locally"),
    )
}

fn explainer_text() -> CliArtifact {
    CliArtifact::text(
        TextArtifact::new(
            "# Artifacts in this terminal\n\nEvery MCP tool on this server returns a typed \
             artifact rather than plain text. The terminal stores each one on the artifact \
             shelf — the chip in the header — and renders it server-side with the same \
             renderer an MCP host would use, so the preview you open is byte-for-byte what \
             any client of this server would see.\n\nCall `render_artifact` with a different \
             `artifact_type` to see each renderer: table, chart, list, dashboard, \
             presentation_card, message, copy_paste_text, and this one — text.",
        )
        .with_title("How artifacts work here"),
    )
}
