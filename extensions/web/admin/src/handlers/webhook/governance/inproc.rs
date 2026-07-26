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
use systemprompt::identifiers::{AgentId, PluginId, SessionId, UserId};
use systemprompt::traits::AnalyticsProvider;
use systemprompt_security::authz::{Decision, MatchedBy};
use systemprompt_security::policy::types::AccessScope;

use super::handler::attested_session_id;
use super::handler::evaluate::{EvaluateInput, evaluate};
use super::types::{
    AuditTarget, ChainEntryOutcome, ChainEntryResult, DecisionAudit, PrincipalSnapshot,
};
use super::{audit, scope};

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
    let access_scope = scope::higher_privilege(db_scope, scope::resolve_agent_scope(&agent_id));

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
    record(pool, call, &verdict, &agent_id);
    verdict
}

/// Audit a human's approve/deny on a call policy already permitted.
///
/// A second row rather than a mutation of the first: the spine is append-only,
/// and "policy allowed, operator refused" is two facts, not a correction of
/// one. `policy` is `human_approval` so the trace view can tell them apart.
pub(crate) fn record_human_decision(
    pool: &Arc<PgPool>,
    call: &GovernedCall<'_>,
    verdict: &PolicyVerdict,
    outcome: HumanOutcome,
) {
    let agent_id = AgentId::new(PI_AGENT_ID);
    let decision = match outcome {
        HumanOutcome::Approved => Decision::Allow {
            matched_by: MatchedBy::UserAllow,
        },
        HumanOutcome::Denied | HumanOutcome::TimedOut | HumanOutcome::Abandoned => Decision::Deny {
            reason: systemprompt_security::authz::DenyReason::PolicyViolation {
                policy: "human_approval".to_owned(),
                detail: std::borrow::Cow::Borrowed(outcome.reason()),
            },
        },
    };
    let audit = DecisionAudit {
        decision,
        principal: PrincipalSnapshot {
            user_id: call.user_id.clone(),
            session_id: verdict.attested.clone(),
            agent_session: Some(call.agent_session.clone()),
            agent_id: Some(agent_id),
            agent_scope: verdict.access_scope,
        },
        target: AuditTarget {
            tool_name: call.tool_name.to_owned(),
            plugin_id: Some(PluginId::new(PI_PLUGIN_ID)),
        },
        chain: vec![ChainEntryOutcome {
            policy_id: systemprompt::identifiers::PolicyId::new("human_approval"),
            result: match outcome {
                HumanOutcome::Approved => ChainEntryResult::Pass,
                _ => ChainEntryResult::Fail,
            },
            detail: outcome.reason().to_owned(),
        }],
    };
    spawn_write(pool, audit);
}

/// How an approval ended. Three of the four are denials, which is the point:
/// only an explicit approve lets a call through.
#[derive(Debug, Clone, Copy)]
pub(crate) enum HumanOutcome {
    Approved,
    Denied,
    /// Nobody answered inside the window.
    TimedOut,
    /// Every viewer disconnected, so nobody was left to answer.
    Abandoned,
}

impl HumanOutcome {
    pub(crate) const fn reason(self) -> &'static str {
        match self {
            Self::Approved => "approved by operator",
            Self::Denied => "denied by operator",
            Self::TimedOut => "approval timed out",
            Self::Abandoned => "approval abandoned — no viewer connected",
        }
    }

    pub(crate) const fn allowed(self) -> bool {
        matches!(self, Self::Approved)
    }
}

fn record(
    pool: &Arc<PgPool>,
    call: &GovernedCall<'_>,
    verdict: &PolicyVerdict,
    agent_id: &AgentId,
) {
    let audit = DecisionAudit {
        decision: verdict.decision.clone(),
        principal: PrincipalSnapshot {
            user_id: call.user_id.clone(),
            session_id: verdict.attested.clone(),
            agent_session: Some(call.agent_session.clone()),
            agent_id: Some(agent_id.clone()),
            agent_scope: verdict.access_scope,
        },
        target: AuditTarget {
            tool_name: call.tool_name.to_owned(),
            plugin_id: Some(PluginId::new(PI_PLUGIN_ID)),
        },
        chain: verdict.chain.clone(),
    };
    spawn_write(pool, audit);
}

/// Audit writes never block a verdict: the tool call is already waiting on it,
/// and a slow `INSERT` must not become a slow gate.
fn spawn_write(pool: &Arc<PgPool>, audit: DecisionAudit) {
    let pool = Arc::<PgPool>::clone(pool);
    tokio::spawn(async move {
        let session_id = audit.principal.session_id.clone();
        if let Err(e) = audit::record_decision(&pool, &audit).await {
            tracing::error!(
                target: "governance.audit.write_failed",
                error = %e,
                session_id = %session_id,
                "pi governance audit write failed; row dropped",
            );
        }
    });
}
