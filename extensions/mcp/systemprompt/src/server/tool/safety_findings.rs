//! `safety_findings` — what the gateway's scanners caught, read back out.
//!
//! The counterpart to `governance_stats`, one layer up. `governance_stats`
//! reports the tool-call gate: a policy chain that runs on tool *input* and
//! answers the caller with the name of the policy and the pattern that matched.
//! This reports the gateway's scan of conversation *content* on the way to a
//! provider — a different code path, in
//! `extensions/web/admin/src/gateway_safety.rs`, that refuses the whole request
//! with a 403 before any tokens are billed.
//!
//! That refusal tells the caller very little on purpose: the response body
//! names only the category, never the pattern or the excerpt. Without this tool
//! the finding is unreadable to the person it was written about —
//! `ai_safety_findings` has an insert path in core and no query path anywhere,
//! so nothing else in this deployment, CLI included, can show it to them.

use crate::repositories::{self, DECISION_LIMIT};
use crate::tool_inputs::SafetyFindingsInput;
use rmcp::ErrorData as McpError;
use serde_json::json;
use std::future::Future;
use systemprompt::database::DbPool;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::McpToolHandler;
use systemprompt::models::artifacts::{CliArtifact, Column, ColumnType, TableArtifact};
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::db_error;

pub(in crate::server) struct SafetyFindingsHandler {
    pub(in crate::server) db_pool: DbPool,
}

impl McpToolHandler for SafetyFindingsHandler {
    type Input = SafetyFindingsInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "safety_findings"
    }

    fn description(&self) -> &'static str {
        "Return this caller's own gateway safety findings, newest first."
    }

    fn handle(
        &self,
        _input: Self::Input,
        ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let db_pool = std::sync::Arc::<systemprompt::database::Database>::clone(&self.db_pool);
        let user_id = ctx.user_id().clone();
        async move {
            let Some(pool) = db_pool.pool() else {
                return Err(McpError::internal_error(
                    "This server has no database connection, so gateway safety findings \
                     cannot be read.",
                    None,
                ));
            };
            let rows = repositories::list_safety_findings(pool.as_ref(), &user_id, DECISION_LIMIT)
                .await
                .map_err(|e| db_error(&e))?;

            let summary = match rows.len() {
                0 => "no gateway safety findings for this caller — nothing sent has \
                      matched a blocking pattern"
                    .to_owned(),
                n => format!(
                    "{n} gateway safety finding(s); each was judged before a provider \
                     was reached, and blocking categories were refused with a 403 \
                     before any tokens were billed"
                ),
            };
            Ok((CliArtifact::table(findings_table(&rows)), summary))
        }
    }
}

// Why: excerpts arrive already redacted by the scanner that wrote them — the
// matched credential is never stored, so it cannot appear in a cell here.
fn findings_table(rows: &[repositories::SafetyFindingRow]) -> TableArtifact {
    let columns = vec![
        Column::new("at", ColumnType::Date).with_header("When"),
        Column::new("phase", ColumnType::String).with_header("Phase"),
        Column::new("severity", ColumnType::String).with_header("Severity"),
        Column::new("category", ColumnType::String).with_header("Category"),
        Column::new("scanner", ColumnType::String).with_header("Scanner"),
        Column::new("excerpt", ColumnType::String).with_header("Excerpt"),
    ];
    let items: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            json!({
                "at": row.at.format("%Y-%m-%d %H:%M:%S").to_string(),
                "phase": row.phase,
                "severity": row.severity,
                "category": row.category,
                "scanner": row.scanner,
                "excerpt": row.excerpt,
            })
        })
        .collect();
    TableArtifact::new(columns).with_rows(items)
}
