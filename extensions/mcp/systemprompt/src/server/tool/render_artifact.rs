//! `render_artifact` — one artifact of any visual type, on demand.
//!
//! The showcase handler behind the terminal's artifact shelf. The data-shaped
//! variants (table, chart, dashboard) answer from the caller's own governance
//! spine — the same rows `governance_stats` reads — with a curated fallback so
//! a fresh session still renders something. The rest carry fixed content
//! about systemprompt.io.

use crate::repositories::{self, DECISION_LIMIT};
use crate::tools::{DemoArtifactType, RenderArtifactInput};
use rmcp::ErrorData as McpError;
use serde_json::json;
use std::future::Future;
use systemprompt::database::DbPool;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::{McpToolHandler, WEBSITE_URL};
use systemprompt::models::artifacts::{
    CardCta, CardSection, ChartArtifact, ChartDataset, ChartType, CliArtifact, Column, ColumnType,
    CopyPasteTextArtifact, DashboardArtifact, DashboardSection, ListArtifact, ListItem,
    MessageArtifact, MetricCard, MetricsCardsData, NoticeLine, PresentationCardArtifact,
    SectionType, ServiceStatus, StatusSectionData, TableArtifact, TextArtifact,
};
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::db_error;

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
                DemoArtifactType::Table => {
                    spine_table(&db_pool, &user_id, &session_id).await?
                },
                DemoArtifactType::Chart => {
                    spine_chart(&db_pool, &user_id, &session_id).await?
                },
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

/// The connection guard every data-shaped variant shares.
async fn spine_rows(
    db_pool: &DbPool,
    user_id: &systemprompt::identifiers::UserId,
    session_id: &systemprompt::identifiers::SessionId,
) -> Result<
    (
        Vec<repositories::PolicyTally>,
        Vec<repositories::DecisionRow>,
        repositories::SpendRow,
    ),
    McpError,
> {
    let Some(pool) = db_pool.pool() else {
        return Err(McpError::internal_error(
            "This server has no database connection, so the governance spine cannot be read.",
            None,
        ));
    };
    let pool = pool.as_ref();
    let tallies = repositories::list_policy_tallies(pool, user_id, session_id)
        .await
        .map_err(|e| db_error(&e))?;
    let decisions = repositories::list_recent_decisions(pool, user_id, session_id, DECISION_LIMIT)
        .await
        .map_err(|e| db_error(&e))?;
    let spend = repositories::get_spend(pool, user_id, session_id)
        .await
        .map_err(|e| db_error(&e))?;
    Ok((tallies, decisions, spend))
}

async fn spine_table(
    db_pool: &DbPool,
    user_id: &systemprompt::identifiers::UserId,
    session_id: &systemprompt::identifiers::SessionId,
) -> Result<CliArtifact, McpError> {
    let (_, decisions, _) = spine_rows(db_pool, user_id, session_id).await?;
    let columns = vec![
        Column::new("at", ColumnType::Date).with_header("When"),
        Column::new("tool", ColumnType::String).with_header("Tool"),
        Column::new("decision", ColumnType::String).with_header("Outcome"),
        Column::new("policy", ColumnType::String).with_header("Policy"),
        Column::new("reason", ColumnType::String).with_header("Reason"),
    ];
    let rows: Vec<serde_json::Value> = if decisions.is_empty() {
        // Why: a fresh session has no verdicts yet; the demo must still put a
        // populated table on screen, and says so in the rows themselves.
        vec![json!({
            "at": "—",
            "tool": "render_artifact",
            "decision": "allow",
            "policy": "example",
            "reason": "No governance decisions recorded in this session yet; this row is illustrative."
        })]
    } else {
        decisions
            .iter()
            .map(|d| {
                json!({
                    "at": d.at.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "tool": d.tool_name,
                    "decision": d.decision,
                    "policy": d.policy,
                    "reason": d.reason,
                })
            })
            .collect()
    };
    Ok(CliArtifact::table(
        TableArtifact::new(columns).with_rows(rows),
    ))
}

async fn spine_chart(
    db_pool: &DbPool,
    user_id: &systemprompt::identifiers::UserId,
    session_id: &systemprompt::identifiers::SessionId,
) -> Result<CliArtifact, McpError> {
    let (tallies, _, _) = spine_rows(db_pool, user_id, session_id).await?;
    let (labels, allowed, denied): (Vec<String>, Vec<f64>, Vec<f64>) = if tallies.is_empty() {
        // Why: illustrative shape for a session with no verdicts yet — labelled
        // as the four real pipeline stages so the chart still teaches something.
        (
            ["scope_check", "secret_scan", "tool_blocklist", "rate_limit"]
                .map(String::from)
                .to_vec(),
            vec![3.0, 3.0, 2.0, 2.0],
            vec![1.0, 0.0, 1.0, 0.0],
        )
    } else {
        (
            tallies.iter().map(|t| t.policy.clone()).collect(),
            tallies.iter().map(|t| t.allowed as f64).collect(),
            tallies.iter().map(|t| t.denied as f64).collect(),
        )
    };
    Ok(CliArtifact::chart(
        ChartArtifact::new("Governance verdicts by policy", ChartType::Bar)
            .with_x_axis_labels(labels)
            .with_datasets(vec![
                ChartDataset::new("Allowed", allowed),
                ChartDataset::new("Denied", denied),
            ])
            .with_axes("Policy", "Verdicts"),
    ))
}

async fn spine_dashboard(
    db_pool: &DbPool,
    user_id: &systemprompt::identifiers::UserId,
    session_id: &systemprompt::identifiers::SessionId,
) -> Result<CliArtifact, McpError> {
    let (tallies, _, spend) = spine_rows(db_pool, user_id, session_id).await?;
    let allowed: i64 = tallies.iter().map(|t| t.allowed).sum();
    let denied: i64 = tallies.iter().map(|t| t.denied).sum();

    let card = |title: &str, value: String| MetricCard {
        title: title.to_owned(),
        value,
        subtitle: None,
        icon: None,
        status: None,
    };
    let metrics =
        DashboardSection::new("spine-metrics", "Session at a glance", SectionType::MetricsCards)
            .with_data(MetricsCardsData::new(vec![
                card("Verdicts allowed", allowed.to_string()),
                card("Verdicts denied", denied.to_string()),
                card("Provider requests", spend.requests.to_string()),
                card(
                    "Cost (USD)",
                    format!("${:.4}", spend.cost_microdollars as f64 / 1_000_000.0),
                ),
            ]))
            .map_err(|e| McpError::internal_error(format!("dashboard data: {e}"), None))?;

    let stage = |name: &str| ServiceStatus {
        name: name.to_owned(),
        status: "active".to_owned(),
        uptime: None,
    };
    let status = DashboardSection::new("spine-status", "Pipeline stages", SectionType::Status)
        .with_data(StatusSectionData {
            services: ["scope_check", "secret_scan", "tool_blocklist", "rate_limit"]
                .map(|s| stage(s))
                .to_vec(),
            database: None,
            recent_errors: None,
        })
        .map_err(|e| McpError::internal_error(format!("dashboard data: {e}"), None))?;

    Ok(CliArtifact::dashboard(
        DashboardArtifact::new("Governance dashboard")
            .with_description(
                "This session's own audit spine, summarised: verdict counts, provider spend, \
                 and the four pipeline stages every tool call passes through.",
            )
            .with_sections(vec![metrics, status]),
    ))
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
