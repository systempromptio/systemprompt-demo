//! `governance_stats` — the audit spine, read back out of the database.
//!
//! The one handler that answers from the database rather than from compiled-in
//! content. It reports what the four-stage pipeline actually decided, which is
//! the demo's whole claim: these numbers come from the same rows the CLI
//! reports on, not from anything assembled for display.

use crate::repositories::{self, DECISION_LIMIT};
use crate::tools::GovernanceStatsInput;
use systemprompt::database::DbPool;
use rmcp::ErrorData as McpError;
use std::future::Future;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::McpToolHandler;
use systemprompt::models::artifacts::CliArtifact;
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::text_artifact;
use super::db_error;

/// `governance_stats` — read the caller's own audit rows back.
///
/// Holds a pool because this is the one handler that answers from the database
/// rather than from compiled-in content. The caller is taken from the
/// authenticated request context, never from the input, which is why the input
/// type has no fields.
pub(in crate::server) struct GovernanceStatsHandler {
    pub(in crate::server) db_pool: DbPool,
}

impl McpToolHandler for GovernanceStatsHandler {
    type Input = GovernanceStatsInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "governance_stats"
    }

    fn description(&self) -> &'static str {
        "Return the calling identity's own governance verdicts, spend, and tool fires."
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
            // No pool means the server started without a database. Reporting
            // that plainly beats an empty table, which would read as "nothing
            // was governed" rather than "nothing could be read".
            let Some(pool) = db_pool.pool() else {
                return Err(McpError::internal_error(
                    "This server has no database connection, so the governance spine \
                     cannot be read.",
                    None,
                ));
            };
            let pool = pool.as_ref();
            let stats = Spine {
                tallies: repositories::list_policy_tallies(pool, &user_id)
                    .await
                    .map_err(|e| db_error(&e))?,
                decisions: repositories::list_recent_decisions(pool, &user_id, DECISION_LIMIT)
                    .await
                    .map_err(|e| db_error(&e))?,
                spend: repositories::get_spend(pool, &user_id)
                    .await
                    .map_err(|e| db_error(&e))?,
                fires: repositories::list_tool_fires(pool, &user_id, DECISION_LIMIT)
                    .await
                    .map_err(|e| db_error(&e))?,
            };

            let summary = format!(
                "{} allowed, {} denied, {} request(s)",
                stats.allowed(),
                stats.denied(),
                stats.spend.requests
            );
            Ok((
                text_artifact("Governance Statistics", render_spine(&stats)),
                summary,
            ))
        }
    }
}

/// One caller's spine, as the four queries return it.
struct Spine {
    tallies: Vec<repositories::PolicyTally>,
    decisions: Vec<repositories::DecisionRow>,
    spend: repositories::SpendRow,
    fires: Vec<repositories::ToolFireRow>,
}

impl Spine {
    fn allowed(&self) -> i64 {
        self.tallies.iter().map(|t| t.allowed).sum()
    }

    fn denied(&self) -> i64 {
        self.tallies.iter().map(|t| t.denied).sum()
    }
}

/// Render the spine as Markdown for a model to read and summarise.
///
/// Every section says something explicit when it is empty. A blank section
/// reads to a model as "nothing to report", and it would then tell the user
/// that nothing was governed — the opposite of what an empty table means here.
fn render_spine(stats: &Spine) -> String {
    let mut body = String::from("# Governance statistics for the calling identity\n\n## Spend\n\n");

    let spend = &stats.spend;
    body.push_str(&format!(
        "- Provider requests: {}\n- Tokens: {} in / {} out\n- Cost: ${:.4}\n",
        spend.requests,
        spend.input_tokens,
        spend.output_tokens,
        spend.cost_microdollars as f64 / 1_000_000.0,
    ));
    match spend.mean_latency_ms {
        Some(ms) => body.push_str(&format!("- Mean latency: {ms:.0} ms\n")),
        None => body.push_str("- Mean latency: no completed request yet\n"),
    }
    body.push_str(&format!(
        "- Most recent model: {}\n\n## Verdicts by policy\n\n",
        spend.model.as_deref().unwrap_or("none reached yet")
    ));

    if stats.tallies.is_empty() {
        body.push_str("No governance decisions recorded for this identity yet.\n\n");
    } else {
        body.push_str(&format!(
            "{} allowed, {} denied across all policies.\n\n\
             | Policy | Allowed | Denied |\n|---|---|---|\n",
            stats.allowed(),
            stats.denied()
        ));
        for tally in &stats.tallies {
            body.push_str(&format!(
                "| `{}` | {} | {} |\n",
                tally.policy, tally.allowed, tally.denied
            ));
        }
        body.push('\n');
    }

    body.push_str(&format!(
        "## Recent decisions (newest {DECISION_LIMIT} max)\n\n"
    ));
    if stats.decisions.is_empty() {
        body.push_str("None.\n\n");
    } else {
        body.push_str("| When | Tool | Outcome | Policy | Reason |\n|---|---|---|---|---|\n");
        for row in &stats.decisions {
            // A pipe in a policy reason would silently break the table, and the
            // reason is the one field here written by something other than this
            // crate.
            let reason = row.reason.replace('|', "\\|");
            body.push_str(&format!(
                "| {} | `{}` | {} | `{}` | {} |\n",
                row.at.format("%Y-%m-%d %H:%M:%S"),
                row.tool_name,
                row.decision,
                row.policy,
                reason
            ));
        }
        body.push('\n');
    }

    body.push_str("## Tools that actually ran\n\n");
    if stats.fires.is_empty() {
        body.push_str("None recorded.\n");
    } else {
        for row in &stats.fires {
            body.push_str(&format!("- `{}` — {} fire(s)\n", row.tool_name, row.fires));
        }
    }
    body
}
