//! `governance_stats` — the audit spine, read back out of the database.
//!
//! The one handler that answers from the database rather than from compiled-in
//! content. It reports what the four-stage pipeline actually decided, which is
//! the demo's whole claim: these numbers come from the same rows the CLI
//! reports on, not from anything assembled for display.

use crate::repositories::{self, DECISION_LIMIT};
use crate::tools::GovernanceStatsInput;
use rmcp::ErrorData as McpError;
use std::future::Future;
use systemprompt::database::DbPool;
use systemprompt::identifiers::{McpExecutionId, SessionId};
use systemprompt::mcp::McpToolHandler;
use systemprompt::models::artifacts::CliArtifact;
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::{db_error, text_artifact};

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
        "Return this session's own governance verdicts, spend, and tool fires."
    }

    fn handle(
        &self,
        _input: Self::Input,
        ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let db_pool = std::sync::Arc::<systemprompt::database::Database>::clone(&self.db_pool);
        let user_id = ctx.user_id().clone();
        let session_id = ctx.session_id().clone();
        async move {
            let Some(pool) = db_pool.pool() else {
                return Err(McpError::internal_error(
                    "This server has no database connection, so the governance spine \
                     cannot be read.",
                    None,
                ));
            };
            let pool = pool.as_ref();
            let stats = Spine {
                tallies: repositories::list_policy_tallies(pool, &user_id, &session_id)
                    .await
                    .map_err(|e| db_error(&e))?,
                decisions: repositories::list_recent_decisions(
                    pool,
                    &user_id,
                    &session_id,
                    DECISION_LIMIT,
                )
                .await
                .map_err(|e| db_error(&e))?,
                spend: repositories::get_spend(pool, &user_id, &session_id)
                    .await
                    .map_err(|e| db_error(&e))?,
                fires: repositories::list_tool_fires(pool, &user_id, &session_id, DECISION_LIMIT)
                    .await
                    .map_err(|e| db_error(&e))?,
                session_id,
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

struct Spine {
    tallies: Vec<repositories::PolicyTally>,
    decisions: Vec<repositories::DecisionRow>,
    spend: repositories::SpendRow,
    fires: Vec<repositories::ToolFireRow>,
    session_id: SessionId,
}

impl Spine {
    fn allowed(&self) -> i64 {
        self.tallies.iter().map(|t| t.allowed).sum()
    }

    fn denied(&self) -> i64 {
        self.tallies.iter().map(|t| t.denied).sum()
    }

    fn has_session(&self) -> bool {
        !self.session_id.as_str().is_empty() && self.session_id.as_str() != "unknown"
    }
}

fn render_spine(stats: &Spine) -> String {
    if !stats.has_session() {
        return "# Governance statistics\n\nThis caller presented no session id, so no \
                session-scoped rows can be read. This is not an ungoverned deployment — it \
                is a request that did not say which session to report on.\n"
            .to_owned();
    }

    let mut body = format!(
        "# Governance statistics for this session (`{}`)\n\n\
         Every figure below is scoped to this session alone, not to the account's history.\n\n\
         ## Spend\n\n",
        stats.session_id
    );

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
        body.push_str("No governance decisions recorded in this session yet.\n\n");
    } else {
        body.push_str(&format!(
            "{} allowed, {} denied across all policies in this session.\n\n\
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
        body.push_str("None in this session.\n");
    } else {
        for row in &stats.fires {
            body.push_str(&format!("- `{}` — {} fire(s)\n", row.tool_name, row.fires));
        }
    }
    body
}
