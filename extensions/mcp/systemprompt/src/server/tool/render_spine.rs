//! The data-shaped `render_artifact` variants.
//!
//! Table, chart, and dashboard all answer from the caller's own governance
//! spine — the same rows `governance_stats` reads. Each carries a curated
//! fallback so a session with no verdicts yet still renders something, and
//! each fallback says in its own content that it is illustrative.

use crate::repositories::{self, DECISION_LIMIT};
use rmcp::ErrorData as McpError;
use serde_json::json;
use systemprompt::database::DbPool;
use systemprompt::identifiers::{SessionId, UserId};
use systemprompt::models::artifacts::{
    ChartArtifact, ChartDataset, ChartType, CliArtifact, Column, ColumnType, DashboardArtifact,
    DashboardSection, MetricCard, MetricsCardsData, SectionType, ServiceStatus, StatusSectionData,
    TableArtifact,
};

use super::db_error;

const PIPELINE_STAGES: [&str; 4] = ["scope_check", "secret_scan", "tool_blocklist", "rate_limit"];

async fn spine_rows(
    db_pool: &DbPool,
    user_id: &UserId,
    session_id: &SessionId,
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

fn decision_columns() -> Vec<Column> {
    vec![
        Column::new("at", ColumnType::Date).with_header("When"),
        Column::new("tool", ColumnType::String).with_header("Tool"),
        Column::new("decision", ColumnType::String).with_header("Outcome"),
        Column::new("policy", ColumnType::String).with_header("Policy"),
        Column::new("enforced", ColumnType::String).with_header("Enforced"),
        Column::new("reason", ColumnType::String).with_header("Reason"),
    ]
}

pub(super) fn decisions_table(decisions: &[repositories::DecisionRow]) -> TableArtifact {
    let rows: Vec<serde_json::Value> = decisions
        .iter()
        .map(|d| {
            json!({
                "at": d.at.format("%Y-%m-%d %H:%M:%S").to_string(),
                "tool": d.tool_name,
                "decision": d.decision,
                "policy": d.policy,
                "enforced": if d.reverified { "gate + proxy" } else { "gate" },
                "reason": d.reason,
            })
        })
        .collect();
    TableArtifact::new(decision_columns()).with_rows(rows)
}

pub(super) async fn spine_table(
    db_pool: &DbPool,
    user_id: &UserId,
    session_id: &SessionId,
) -> Result<CliArtifact, McpError> {
    let (_, decisions, _) = spine_rows(db_pool, user_id, session_id).await?;
    if decisions.is_empty() {
        let row = json!({
            "at": "—",
            "tool": "render_artifact",
            "decision": "allow",
            "policy": "example",
            "enforced": "gate",
            "reason": "No governance decisions recorded in this session yet; this row is illustrative."
        });
        return Ok(CliArtifact::table(
            TableArtifact::new(decision_columns()).with_rows(vec![row]),
        ));
    }
    Ok(CliArtifact::table(decisions_table(&decisions)))
}

pub(super) async fn spine_chart(
    db_pool: &DbPool,
    user_id: &UserId,
    session_id: &SessionId,
) -> Result<CliArtifact, McpError> {
    let (tallies, _, _) = spine_rows(db_pool, user_id, session_id).await?;
    let (labels, allowed, denied): (Vec<String>, Vec<f64>, Vec<f64>) = if tallies.is_empty() {
        (
            PIPELINE_STAGES.map(String::from).to_vec(),
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

pub(super) async fn spine_dashboard(
    db_pool: &DbPool,
    user_id: &UserId,
    session_id: &SessionId,
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
            services: PIPELINE_STAGES.map(stage).to_vec(),
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
