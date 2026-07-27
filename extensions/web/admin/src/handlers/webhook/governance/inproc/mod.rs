//! In-process governance for callers that are not HTTP hooks.
//!
//! `/hooks/govern` exists for out-of-process agents that speak the Claude Code
//! hook wire. The pi web terminal is different: the agent's enforcement point
//! and this policy chain live in the same binary, connected by the child's own
//! RPC stream rather than by a request. Re-POSTing to ourselves would mean
//! minting a JWT to satisfy our own authenticator, re-parsing a payload we
//! already have typed, and — the reason that settles it — accepting a
//! decision-shaped answer with nowhere to suspend for a human.
//!
//! So this module is the seam: the same [`evaluate`] chain and the same
//! [`DecisionAudit`] row, reached by function call, with the human round-trip
//! layered on top by the caller.
//!
//! Nothing here decides *whether* to ask a human — that is the proxy's job.
//! This module answers "does policy permit it" and records what happened.

use std::sync::Arc;

use sqlx::PgPool;
use systemprompt::identifiers::{AgentId, SessionId, UserId};
use systemprompt::traits::AnalyticsProvider;
use systemprompt_security::authz::Decision;
use systemprompt_security::policy::types::AccessScope;

use super::handler::attested_session_id;
use super::handler::evaluate::{EvaluateInput, evaluate};
use super::scope;
use super::stages::{StageOutcome, StageResult};
use super::types::{ChainEntryOutcome, ChainEntryResult};

mod record;

use record::record;
pub(crate) use record::{HumanOutcome, record_human_decision, record_policy_denial};

pub(crate) const PI_AGENT_ID: &str = "pi_agent";

pub(crate) const PI_PLUGIN_ID: &str = "enterprise-demo";

pub(crate) struct GovernedCall<'a> {
    pub(crate) tool_name: &'a str,
    pub(crate) user_id: &'a UserId,
    pub(crate) agent_session: &'a SessionId,
    pub(crate) tool_input: Option<&'a serde_json::Value>,
    pub(crate) scope_ceiling: AccessScope,
}

pub(crate) const PROMPT_TOOL_NAME: &str = "user_prompt";

pub(crate) struct PolicyVerdict {
    pub(crate) allowed: bool,
    pub(crate) reason: Option<String>,
    pub(crate) policy: Option<String>,
    decision: Decision,
    chain: Vec<ChainEntryOutcome>,
    access_scope: AccessScope,
    attested: SessionId,
}

impl PolicyVerdict {
    pub(crate) fn stages(&self) -> Vec<StageOutcome> {
        self.chain
            .iter()
            .map(|entry| StageOutcome {
                policy: entry.policy_id.to_string(),
                result: match entry.result {
                    ChainEntryResult::Pass => StageResult::Pass,
                    ChainEntryResult::Fail => StageResult::Fail,
                    ChainEntryResult::Skip => StageResult::Skip,
                },
                detail: entry.detail.clone(),
            })
            .collect()
    }
}

pub(crate) async fn govern_call(
    pool: &Arc<PgPool>,
    analytics: &Arc<dyn AnalyticsProvider>,
    claimed_session: &SessionId,
    call: &GovernedCall<'_>,
) -> PolicyVerdict {
    let agent_id = AgentId::new(PI_AGENT_ID);
    let attested = attested_session_id(analytics, claimed_session, call.user_id).await;

    let db_scope = scope::scope_from_user_roles(pool, call.user_id).await;
    let resolved = scope::higher_privilege(db_scope, scope::resolve_agent_scope(&agent_id));
    let access_scope = scope::cap_at(resolved, call.scope_ceiling);

    let (decision, chain) = evaluate(&EvaluateInput {
        tool_name: call.tool_name,
        session_id: call.agent_session,
        user_id: call.user_id,
        access_scope,
        tool_input: call.tool_input,
    });

    let (allowed, reason) = match &decision {
        Decision::Allow { .. } => (true, None),
        Decision::Deny { reason } => (false, Some(format!("[GOVERNANCE] {reason}"))),
    };
    let policy = chain
        .iter()
        .find(|e| matches!(e.result, ChainEntryResult::Fail))
        .map(|e| e.policy_id.to_string());

    let verdict = PolicyVerdict {
        allowed,
        reason,
        policy,
        decision,
        chain,
        access_scope,
        attested,
    };
    record(pool, call, &verdict, &agent_id).await;
    verdict
}
