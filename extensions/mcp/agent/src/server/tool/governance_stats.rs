//! `governance_stats` — the audit spine, read back out of the database.
//!
//! Answers from the database rather than from compiled-in content. It reports
//! what the four-stage pipeline actually decided, which is the demo's whole
//! claim: these rows come from the same tables the CLI reports on, not from
//! anything assembled for display.
//!
//! The artifact is a **table** of this session's decisions — the typed shape
//! the terminal's renderer draws as one — while the aggregate the old markdown
//! carried (verdict counts, spend, tokens, cost) travels in the one-line
//! summary, where the model quotes it from.

use crate::repositories::{self, DECISION_LIMIT};
use crate::tool_inputs::GovernanceStatsInput;
use rmcp::ErrorData as McpError;
use std::future::Future;
use systemprompt::database::DbPool;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::McpToolHandler;
use systemprompt::models::artifacts::CliArtifact;
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::db_error;
use super::render_spine::decisions_table;

// Why: the caller comes from the authenticated request context, never from
// the input — which is why the input type has no fields.
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
        "Return this session's own governance verdicts as a table, with spend in the summary."
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

            if session_id.as_str().is_empty() || session_id.as_str() == "unknown" {
                // Why: an empty table alone would read as "an ungoverned
                // deployment"; the summary says what it actually is — a request
                // that did not name a session to report on.
                return Ok((
                    CliArtifact::table(decisions_table(&[])),
                    "no session id presented — no session-scoped rows can be read".to_owned(),
                ));
            }

            let tallies = repositories::list_policy_tallies(pool, &user_id, &session_id)
                .await
                .map_err(|e| db_error(&e))?;
            let decisions =
                repositories::list_recent_decisions(pool, &user_id, &session_id, DECISION_LIMIT)
                    .await
                    .map_err(|e| db_error(&e))?;
            let spend = repositories::get_spend(pool, &user_id, &session_id)
                .await
                .map_err(|e| db_error(&e))?;

            let allowed: i64 = tallies.iter().map(|t| t.allowed).sum();
            let denied: i64 = tallies.iter().map(|t| t.denied).sum();
            let summary = format!(
                "{allowed} allowed, {denied} denied across {} decision(s); {} provider \
                 request(s), {} tokens in / {} out, ${:.4}",
                decisions.len(),
                spend.requests,
                spend.input_tokens,
                spend.output_tokens,
                spend.cost_microdollars as f64 / 1_000_000.0,
            );
            Ok((CliArtifact::table(decisions_table(&decisions)), summary))
        }
    }
}
