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
    ApproverStamp, AuditTarget, ChainEntryOutcome, ChainEntryResult, DecisionAudit,
    PrincipalSnapshot,
};

use super::{GovernedCall, PI_AGENT_ID, PI_PLUGIN_ID, PolicyVerdict};

pub(crate) async fn record_human_decision(
    pool: &Arc<PgPool>,
    call: &GovernedCall<'_>,
    verdict: &PolicyVerdict,
    outcome: HumanOutcome,
    approver: Option<ApproverStamp>,
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
        id: uuid::Uuid::new_v4().to_string(),
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
            detail: approver.as_ref().map_or_else(
                || outcome.reason().to_owned(),
                |a| format!("{} by {}", a.action, a.username),
            ),
            duration_ms: 0.0,
        }],
        approver,
    };
    if outcome.allowed() {
        spawn_write(pool, audit);
    } else {
        write_now(pool, audit).await;
    }
}

pub(crate) async fn record_policy_denial(
    pool: &Arc<PgPool>,
    call: &GovernedCall<'_>,
    verdict: &PolicyVerdict,
    policy_id: &PolicyId,
    detail: &str,
) {
    let audit = DecisionAudit {
        id: uuid::Uuid::new_v4().to_string(),
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
            duration_ms: 0.0,
        }],
        approver: None,
    };
    write_now(pool, audit).await;
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum HumanOutcome {
    Approved,
    Denied,
    TimedOut,
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
        id: verdict.decision_id.clone(),
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
        approver: None,
    };
    if verdict.allowed {
        spawn_write(pool, audit);
    } else {
        write_now(pool, audit).await;
    }
}

fn spawn_write(pool: &Arc<PgPool>, audit: DecisionAudit) {
    let pool = Arc::<PgPool>::clone(pool);
    tokio::spawn(async move {
        write_now(&pool, audit).await;
    });
}

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
