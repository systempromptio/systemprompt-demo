//! `admin_audit_dump` — the tool `scope_check` exists to refuse.
//!
//! The deployment-wide inverse of `governance_stats`: where that one is scoped
//! to the calling identity, this one returns every identity's decisions, with
//! their user and session ids attached. That is a real administrative
//! capability and a real disclosure, which is the point — the `admin_` prefix
//! in its name is matched by `policies[id=scope_check].admin_only_prefixes` in
//! `services/governance/config.yaml`, and the pi terminal caps every caller at
//! `user` scope, so the call is denied before it runs.
//!
//! Like `fetch_remote_docs`, this is deliberately not a stub. A refusal only
//! demonstrates something if the thing refused could genuinely have done the
//! damage, so the query below really does read the whole spine. If this body
//! ever executes in the terminal, the policy chain has a hole in it.

use crate::repositories::{self, DECISION_LIMIT};
use crate::tool_inputs::AdminAuditDumpInput;
use rmcp::ErrorData as McpError;
use serde_json::json;
use std::future::Future;
use systemprompt::database::DbPool;
use systemprompt::identifiers::McpExecutionId;
use systemprompt::mcp::McpToolHandler;
use systemprompt::models::artifacts::{CliArtifact, Column, ColumnType, TableArtifact};
use systemprompt::models::execution::context::RequestContext as SysRequestContext;

use super::db_error;

pub(in crate::server) struct AdminAuditDumpHandler {
    pub(in crate::server) db_pool: DbPool,
}

impl McpToolHandler for AdminAuditDumpHandler {
    type Input = AdminAuditDumpInput;
    type Output = CliArtifact;

    fn tool_name(&self) -> &'static str {
        "admin_audit_dump"
    }

    fn description(&self) -> &'static str {
        "Return every identity's governance decisions across the deployment."
    }

    fn handle(
        &self,
        _input: Self::Input,
        ctx: &SysRequestContext,
        _exec_id: &McpExecutionId,
    ) -> impl Future<Output = Result<(Self::Output, String), McpError>> + Send {
        let db_pool = std::sync::Arc::<systemprompt::database::Database>::clone(&self.db_pool);
        let caller = ctx.user_id().clone();
        async move {
            tracing::warn!(
                caller = %caller,
                "admin_audit_dump executed — scope_check did not refuse an admin-only tool"
            );

            let Some(pool) = db_pool.pool() else {
                return Err(McpError::internal_error(
                    "This server has no database connection, so the governance spine \
                     cannot be read.",
                    None,
                ));
            };
            let rows = repositories::list_all_decisions(pool.as_ref(), DECISION_LIMIT)
                .await
                .map_err(|e| db_error(&e))?;

            let summary = format!(
                "{} decision(s) across all identities (newest {DECISION_LIMIT} max)",
                rows.len()
            );
            Ok((CliArtifact::table(dump_table(&rows)), summary))
        }
    }
}

fn dump_table(rows: &[repositories::GlobalDecisionRow]) -> TableArtifact {
    let columns = vec![
        Column::new("at", ColumnType::Date).with_header("When"),
        Column::new("user", ColumnType::String).with_header("User"),
        Column::new("session", ColumnType::String).with_header("Session"),
        Column::new("tool", ColumnType::String).with_header("Tool"),
        Column::new("decision", ColumnType::String).with_header("Outcome"),
        Column::new("policy", ColumnType::String).with_header("Policy"),
    ];
    let items: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            json!({
                "at": row.at.format("%Y-%m-%d %H:%M:%S").to_string(),
                "user": row.user_id.to_string(),
                "session": row.session_id.to_string(),
                "tool": row.tool_name,
                "decision": row.decision,
                "policy": row.policy,
            })
        })
        .collect();
    TableArtifact::new(columns).with_rows(items)
}
