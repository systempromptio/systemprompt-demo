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
use super::types::{
    ChainEntryOutcome, ChainEntryResult,
};
use super::stages::{StageOutcome, StageResult};
use super::scope;

mod record;

pub(crate) use record::{HumanOutcome, record_human_decision, record_policy_denial};
use record::record;

/// The agent id every pi run is audited under, on both the CLI and widget
/// paths, so `/admin/demo/trace` shows one timeline per user rather than two.
pub(crate) const PI_AGENT_ID: &str = "pi_agent";

/// The plugin whose policy config governs pi runs.
pub(crate) const PI_PLUGIN_ID: &str = "enterprise-demo";

/// One thing to govern: a tool call, or the prompt that preceded it.
pub(crate) struct GovernedCall<'a> {
    /// For a prompt this is [`PROMPT_TOOL_NAME`]; the audit spine keys on a
    /// tool name, and a prompt gate needs a stable synthetic one.
    pub(crate) tool_name: &'a str,
    pub(crate) user_id: &'a UserId,
    /// The pi conversation. Rate limiting keys on this, not the credential, so
    /// one runaway conversation cannot throttle a user's other sessions.
    pub(crate) agent_session: &'a SessionId,
    pub(crate) tool_input: Option<&'a serde_json::Value>,
    /// The most privilege this surface may ever evaluate at, whatever the
    /// caller's roles say. `Admin` means "no ceiling".
    ///
    /// Scope is otherwise resolved upwards — DB roles joined with the agent's
    /// declared scope, taking the higher. That is right for the admin console
    /// and for `/hooks/govern`, and wrong for a sandboxed surface, where an
    /// operator signed in as admin would silently skip the policies that
    /// surface exists to demonstrate. There is no `Default`: a new surface has
    /// to say which it is.
    pub(crate) scope_ceiling: AccessScope,
}

/// The synthetic tool name a governed prompt is audited under, matching the
/// HTTP hook path so both spines agree.
pub(crate) const PROMPT_TOOL_NAME: &str = "user_prompt";

/// What policy decided, plus everything needed to audit it or explain it.
pub(crate) struct PolicyVerdict {
    pub(crate) allowed: bool,
    /// Present only on a deny — the operator-facing explanation.
    pub(crate) reason: Option<String>,
    /// The policy that denied, for the widget's blocked row.
    pub(crate) policy: Option<String>,
    decision: Decision,
    chain: Vec<ChainEntryOutcome>,
    access_scope: AccessScope,
    attested: SessionId,
}

impl PolicyVerdict {
    /// The chain exactly as [`evaluate`] ran it.
    ///
    /// Guarantees the order, count, and results are the evaluation's own, so a
    /// policy added, removed, or reordered in `policies/mod.rs` is reflected
    /// without anything restating the list. Nothing can report a stage that did
    /// not run.
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

/// Run the four-stage chain and audit the outcome.
///
/// Always writes an audit row, allow or deny — an unaudited allow is
/// indistinguishable from an ungoverned call when someone reads the spine back.
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
    // The capped scope is what the chain evaluates at *and* what gets audited.
    // Recording the uncapped one would make the trace disagree with the policy
    // that ran.
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
