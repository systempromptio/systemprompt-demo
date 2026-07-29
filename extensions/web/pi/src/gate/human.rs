//! The consent half of the gate: putting a policy-cleared call to a person.
//!
//! Reached only after the chain allowed the call and confinement cleared it,
//! so every path here is a human's judgement *on top of* policy — never
//! instead of it. A person can refuse what policy allowed; they are never
//! offered the chance to allow what policy refused.

use std::sync::Arc;

use systemprompt_security::policy::ApproverStamp;
use systemprompt_web_governance::webhook::governance::inproc::{
    self, HumanOutcome, InprocCall, PolicyVerdict,
};

use super::super::events::PiEventBody;
use super::super::rpc::GovernancePayload;
use super::super::session::{Attribution, PiSession, Verdict};
use super::PiDeps;

const ABANDON_CHECK: std::time::Duration = std::time::Duration::from_secs(5);

const ABANDON_GRACE: std::time::Duration = std::time::Duration::from_secs(15);

pub(super) async fn human_gate(
    deps: &PiDeps,
    session: &Arc<PiSession>,
    ask: ApprovalAsk<'_>,
    call: &InprocCall<'_>,
    verdict: &PolicyVerdict,
) -> bool {
    let approval_id = ask.approval_id.to_owned();
    let tool_use_id = ask.payload.tool_use_id.clone();
    let tool_name = ask.tool_name.to_owned();

    session.emit(PiEventBody::ToolStart {
        tool_use_id: tool_use_id.clone(),
        tool_name: tool_name.clone(),
        tool_input: ask
            .payload
            .tool_input
            .clone()
            .unwrap_or(serde_json::Value::Null),
    });

    let (outcome, attribution) = ask_human(deps, session, ask).await;
    let stamp = attribution.map(|a| ApproverStamp {
        user_id: a.user_id,
        username: a.username,
        decided_at: a.decided_at,
        action: outcome_label(outcome),
    });
    session.emit(PiEventBody::ApprovalResolved {
        approval_id,
        outcome: outcome_label(outcome),
        approved_by: stamp.as_ref().map(|s| s.username.clone()),
        decided_at: stamp.as_ref().map(|s| s.decided_at.to_rfc3339()),
        actor: if stamp.is_some() { "user" } else { "system" },
    });
    inproc::record_human_decision(&deps.pool, call, verdict, outcome, stamp).await;

    if outcome.allowed() {
        return true;
    }
    session.emit(PiEventBody::ToolBlocked {
        tool_use_id,
        tool_name,
        reason: outcome.reason().to_owned(),
        policy: Some("human_approval".to_owned()),
    });
    false
}

pub(super) struct ApprovalAsk<'a> {
    pub(super) approval_id: &'a str,
    pub(super) payload: &'a GovernancePayload,
    pub(super) tool_name: &'a str,
    pub(super) cleared: Vec<String>,
}

async fn ask_human(
    deps: &PiDeps,
    session: &Arc<PiSession>,
    ask: ApprovalAsk<'_>,
) -> (HumanOutcome, Option<Attribution>) {
    let ApprovalAsk {
        approval_id,
        payload,
        tool_name,
        cleared,
    } = ask;
    let rx = session
        .approvals
        .park(approval_id.to_owned(), tool_name.to_owned());
    session.emit(PiEventBody::ApprovalRequest {
        approval_id: approval_id.to_owned(),
        tool_name: tool_name.to_owned(),
        tool_input: payload
            .tool_input
            .clone()
            .unwrap_or(serde_json::Value::Null),
        policy_chain: cleared,
        timeout_secs: deps.cfg.approval_timeout.as_secs(),
    });

    let deadline = tokio::time::Instant::now() + deps.cfg.approval_timeout;
    tokio::pin!(rx);
    let mut viewerless_since: Option<tokio::time::Instant> = None;

    loop {
        tokio::select! {
            biased;
            answer = &mut rx => {
                return match answer {
                    Ok(Verdict::Allow(a)) => (HumanOutcome::Approved, Some(a)),
                    Ok(Verdict::Deny(a)) => (HumanOutcome::Denied, Some(a)),
                    Err(_) => (HumanOutcome::Abandoned, None),
                };
            }
            () = tokio::time::sleep_until(deadline) => {
                session.approvals.forget(approval_id);
                return (HumanOutcome::TimedOut, None);
            }
            () = tokio::time::sleep(ABANDON_CHECK) => {
                if session.has_viewers() {
                    viewerless_since = None;
                } else {
                    let since = *viewerless_since.get_or_insert_with(tokio::time::Instant::now);
                    if since.elapsed() >= ABANDON_GRACE {
                        session.approvals.forget(approval_id);
                        return (HumanOutcome::Abandoned, None);
                    }
                }
            }
        }
    }
}

const fn outcome_label(outcome: HumanOutcome) -> &'static str {
    match outcome {
        HumanOutcome::Approved => "approved",
        HumanOutcome::Denied => "denied",
        HumanOutcome::TimedOut => "timeout",
        HumanOutcome::Abandoned => "abandoned",
    }
}
