//! The consent half of the gate: putting a policy-cleared call to a person.
//!
//! Reached only after the chain allowed the call and confinement cleared it,
//! so every path here is a human's judgement *on top of* policy — never
//! instead of it. A person can refuse what policy allowed; they are never
//! offered the chance to allow what policy refused.

use std::sync::Arc;

use crate::handlers::webhook::governance::inproc::{
    self, GovernedCall, HumanOutcome, PolicyVerdict,
};

use super::PiDeps;
use super::super::events::PiEventBody;
use super::super::rpc::GovernancePayload;
use super::super::session::{PiSession, Verdict};

/// Poll interval while an approval is outstanding, used to notice that every
/// viewer has gone rather than making the model wait out the full timeout for
/// an answer nobody can give.
const ABANDON_CHECK: std::time::Duration = std::time::Duration::from_secs(5);

/// How long all viewers must be absent before an approval is abandoned. Long
/// enough to ride out an SSE reconnect, short enough that a closed tab does not
/// pin a turn.
const ABANDON_GRACE: std::time::Duration = std::time::Duration::from_secs(15);

/// Put a policy-cleared call to a person and record what they said.
///
/// Reached only after the chain allowed the call and confinement cleared it, so
/// every path here is a human's judgement on top of policy — never instead of it.
pub(super) async fn human_gate(
    deps: &PiDeps,
    session: &Arc<PiSession>,
    ask: ApprovalAsk<'_>,
    call: &GovernedCall<'_>,
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

    let outcome = ask_human(deps, session, ask).await;
    inproc::record_human_decision(&deps.pool, call, verdict, outcome).await;
    session.emit(PiEventBody::ApprovalResolved {
        approval_id,
        outcome: outcome_label(outcome),
    });

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

/// One call put to a person, and what policy already established about it.
///
/// Bundled for the same reason [`PiDeps`] is: the pieces are only ever needed
/// together, and passing them separately puts the function over clippy's
/// argument ceiling.
pub(super) struct ApprovalAsk<'a> {
    pub(super) approval_id: &'a str,
    pub(super) payload: &'a GovernancePayload,
    pub(super) tool_name: &'a str,
    /// The real list of policies that passed. Carried in rather than rebuilt
    /// here, so nothing on this path can assert that a check ran — it can only
    /// relay what the evaluation reported.
    pub(super) cleared: Vec<String>,
}

/// Publish an approval card and wait for it to be answered, time out, or be
/// abandoned. Every non-approval path denies.
async fn ask_human(
    deps: &PiDeps,
    session: &Arc<PiSession>,
    ask: ApprovalAsk<'_>,
) -> HumanOutcome {
    let ApprovalAsk {
        approval_id,
        payload,
        tool_name,
        cleared,
    } = ask;
    let rx = session.park_approval(approval_id.to_owned());
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
            // Biased so a verdict that arrives in the same tick as a timeout is
            // honoured rather than discarded.
            biased;
            answer = &mut rx => {
                return match answer {
                    Ok(Verdict::Allow) => HumanOutcome::Approved,
                    Ok(Verdict::Deny) => HumanOutcome::Denied,
                    // Sender dropped: the session is being torn down. Fail closed.
                    Err(_) => HumanOutcome::Abandoned,
                };
            }
            () = tokio::time::sleep_until(deadline) => {
                session.forget_approval(approval_id);
                return HumanOutcome::TimedOut;
            }
            () = tokio::time::sleep(ABANDON_CHECK) => {
                if session.has_viewers() {
                    viewerless_since = None;
                } else {
                    let since = *viewerless_since.get_or_insert_with(tokio::time::Instant::now);
                    if since.elapsed() >= ABANDON_GRACE {
                        session.forget_approval(approval_id);
                        return HumanOutcome::Abandoned;
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
