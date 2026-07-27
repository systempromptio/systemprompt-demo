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
use crate::tools::SafetyFindingsInput;
use rmcp::ErrorData as McpError;
use std::future::Future;
use systemprompt::database::DbPool;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::McpToolHandler;
use systemprompt::models::artifacts::CliArtifact;
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::{db_error, text_artifact};

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
                0 => "no gateway safety findings for this caller".to_owned(),
                n => format!("{n} gateway safety finding(s)"),
            };
            Ok((
                text_artifact("Gateway Safety Findings", render_findings(&rows)),
                summary,
            ))
        }
    }
}

fn render_findings(rows: &[repositories::SafetyFindingRow]) -> String {
    let mut body = String::from("# Gateway safety findings for this caller\n\n");
    if rows.is_empty() {
        body.push_str(
            "No findings. The gateway's scanners run on every request to a provider; \
             nothing this caller has sent has matched a blocking pattern.\n",
        );
        return body;
    }
    body.push_str(
        "Each row is a request the gateway judged **before** it reached a provider. \
         A finding in a blocking category means the request was refused with a 403 \
         and no tokens were billed. Excerpts are redacted by the scanner that wrote \
         them — the matched credential is never stored.\n\n\
         | When | Phase | Severity | Category | Scanner | Excerpt |\n\
         |---|---|---|---|---|---|\n",
    );
    for row in rows {
        let excerpt = row.excerpt.replace('|', "\\|");
        body.push_str(&format!(
            "| {} | {} | {} | `{}` | `{}` | {} |\n",
            row.at.format("%Y-%m-%d %H:%M:%S"),
            row.phase,
            row.severity,
            row.category,
            row.scanner,
            excerpt
        ));
    }
    body
}
