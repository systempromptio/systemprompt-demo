//! Writing what happened into the governance spine.
//!
//! Every function here appends a row; none of them mutates one. "Policy
//! allowed, the operator refused" is two facts, not a correction of the first,
//! and the spine is the record of both.
//!
//! The allow/deny split in [`spawn_write`] versus [`write_now`] is the one
//! subtlety: an allow can afford to be eventually consistent, and the row that
//! proves a denial cannot.

use std::sync::Arc;

use sqlx::PgPool;
use systemprompt::identifiers::{AgentId, PluginId, PolicyId};
use systemprompt_security::authz::{Decision, MatchedBy};

use crate::handlers::webhook::governance::audit;
use crate::handlers::webhook::governance::types::{
    AuditTarget, ChainEntryOutcome, ChainEntryResult, DecisionAudit, PrincipalSnapshot,
};

use super::{GovernedCall, PI_AGENT_ID, PI_PLUGIN_ID, PolicyVerdict};

/// Audit a human's approve/deny on a call policy already permitted.
///
/// A second row rather than a mutation of the first: the spine is append-only,
/// and "policy allowed, operator refused" is two facts, not a correction of
/// one. `policy` is `human_approval` so the trace view can tell them apart.
pub(crate) async fn record_human_decision(
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
            policy_id: PolicyId::new("human_approval"),
            result: match outcome {
                HumanOutcome::Approved => ChainEntryResult::Pass,
                _ => ChainEntryResult::Fail,
            },
            detail: outcome.reason().to_owned(),
        }],
    };
    if outcome.allowed() {
        spawn_write(pool, audit);
    } else {
        write_now(pool, audit).await;
    }
}

/// Audit a caller-side policy that refused a call the chain had allowed.
///
/// Some rules cannot live in [`evaluate`] because they need state only the
/// caller holds — workspace confinement needs the session's own directory,
/// which the policy chain has never heard of. They still have to land in the
/// spine, or a denial the user sees has no record behind it.
///
/// A second row rather than a mutation of the first, for the same reason
/// [`record_human_decision`] is: "policy allowed, the caller's own rule
/// refused" is two facts.
pub(crate) async fn record_policy_denial(
    pool: &Arc<PgPool>,
    call: &GovernedCall<'_>,
    verdict: &PolicyVerdict,
    policy_id: &PolicyId,
    detail: &str,
) {
    let audit = DecisionAudit {
        decision: Decision::Deny {
            reason: systemprompt_security::authz::DenyReason::PolicyViolation {
                policy: policy_id.to_string(),
                detail: std::borrow::Cow::Owned(detail.to_owned()),
            },
        },
        principal: PrincipalSnapshot {
            user_id: call.user_id.clone(),
            session_id: verdict.attested.clone(),
            agent_session: Some(call.agent_session.clone()),
            agent_id: Some(AgentId::new(PI_AGENT_ID)),
            agent_scope: verdict.access_scope,
        },
        target: AuditTarget {
            tool_name: call.tool_name.to_owned(),
            plugin_id: Some(PluginId::new(PI_PLUGIN_ID)),
        },
        chain: vec![ChainEntryOutcome {
            policy_id: policy_id.clone(),
            result: ChainEntryResult::Fail,
            detail: detail.to_owned(),
        }],
    };
    write_now(pool, audit).await;
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

pub(super) async fn record(
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
    if verdict.allowed {
        spawn_write(pool, audit);
    } else {
        write_now(pool, audit).await;
    }
}

/// Audit writes never block an *allow*: the tool call is already waiting on the
/// verdict, and a slow `INSERT` must not become a slow gate.
///
/// Denials do not take this path — see [`write_now`].
fn spawn_write(pool: &Arc<PgPool>, audit: DecisionAudit) {
    let pool = Arc::<PgPool>::clone(pool);
    tokio::spawn(async move {
        write_now(&pool, audit).await;
    });
}

/// Write the row before returning the verdict.
///
/// Denials are read back immediately — the caller is shown a refusal and then
/// asks the spine to prove it happened, often within the same second. Spawning
/// that write races the read, and the demonstration reports no denial for a
/// call the user just watched be refused. An allow can afford to be eventually
/// consistent; the row that proves enforcement cannot.
async fn write_now(pool: &Arc<PgPool>, audit: DecisionAudit) {
    let session_id = audit.principal.session_id.clone();
    if let Err(e) = audit::record_decision(pool, &audit).await {
        tracing::error!(
            target: "governance.audit.write_failed",
            error = %e,
            session_id = %session_id,
            "pi governance audit write failed; row dropped",
        );
    }
}
