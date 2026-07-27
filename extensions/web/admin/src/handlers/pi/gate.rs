//! The governance gate: what happens between pi asking and pi being answered.
//!
//! pi's shim calls `ctx.ui.confirm`, which suspends the tool call and emits an
//! `extension_ui_request`. Nothing runs until we write the matching response,
//! so this module can take as long as it needs — including waiting on a person.
//! There is no pi-side timeout to race (measured: an unanswered confirm waits
//! indefinitely), so the only clock is ours.
//!
//! Order matters and is not negotiable: **policy first, human second.** A human
//! is never offered the chance to override a policy deny — policy is a floor,
//! and a person can only be more restrictive than it.

use std::sync::Arc;

use sqlx::PgPool;
use systemprompt::identifiers::SessionId;
use systemprompt::traits::AnalyticsProvider;

use super::super::webhook::governance::inproc::{
    self, GovernedCall, HumanOutcome, PROMPT_TOOL_NAME, PolicyVerdict,
};
use super::super::webhook::governance::stages::{StageOutcome, StageResult};
use super::config::PiConfig;
use super::events::PiEventBody;
use super::stage::PolicyStage;
use super::rpc::{GovernancePayload, PayloadKind};
use super::session::{PiSession, Verdict};

/// Poll interval while an approval is outstanding, used to notice that every
/// viewer has gone rather than making the model wait out the full timeout for
/// an answer nobody can give.
const ABANDON_CHECK: std::time::Duration = std::time::Duration::from_secs(5);

/// How long all viewers must be absent before an approval is abandoned. Long
/// enough to ride out an SSE reconnect, short enough that a closed tab does not
/// pin a turn.
const ABANDON_GRACE: std::time::Duration = std::time::Duration::from_secs(15);

/// The policy id for a path argument that leaves the session workspace. Named
/// in the audit row, in the blocked card, and in the approval card's chain, so
/// all three agree on what cleared or refused the call.
const WORKSPACE_SCOPE: &str = "workspace_scope";

/// Everything the pi surface needs, in one extension layer.
///
/// One bundle rather than four `Extension`s: the handlers were pushing past
/// clippy's argument ceiling, and the pieces are always needed together.
pub(super) struct PiDeps {
    pub(super) pool: Arc<PgPool>,
    pub(super) analytics: Arc<dyn AnalyticsProvider>,
    pub(super) session_service: Arc<systemprompt::oauth::SessionCreationService>,
    pub(super) cfg: PiConfig,
}

/// Decide one `confirm` request. The returned bool is written back to pi
/// verbatim: `true` lets the call proceed, `false` blocks it.
pub(super) async fn decide(
    deps: &PiDeps,
    session: &Arc<PiSession>,
    approval_id: &str,
    payload: &GovernancePayload,
) -> bool {
    session.touch();

    let tool_name = match payload.kind {
        PayloadKind::Prompt => PROMPT_TOOL_NAME.to_owned(),
        PayloadKind::Tool => payload
            .tool_name
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
    };
    // For a prompt the governed body is the prompt text; `secret_scan` reads it
    // out of the same field the HTTP hook uses, so a credential pasted into the
    // box is caught while it is still local.
    let governed_input = match payload.kind {
        PayloadKind::Prompt => payload
            .prompt
            .as_ref()
            .map(|p| serde_json::json!({ "prompt": p })),
        PayloadKind::Tool => payload.tool_input.clone(),
    };

    let agent_session = SessionId::new(session.conversation_id.clone());
    let call = GovernedCall {
        tool_name: &tool_name,
        user_id: &session.user_id,
        agent_session: &agent_session,
        tool_input: governed_input.as_ref(),
    };

    let verdict = inproc::govern_call(
        &deps.pool,
        &deps.analytics,
        &session.attested_session,
        &call,
    )
    .await;

    // Publish the chain before acting on it, so the browser sees the same
    // evaluation the audit row is built from — including on a deny, where the
    // interesting part is which stage stopped it and that the ones after it
    // never ran.
    let stages = verdict.stages();
    emit_stages(session, payload, &tool_name, &stages);

    if !verdict.allowed {
        emit_denial(session, payload, &tool_name, &verdict);
        return false;
    }

    // Confinement before consent. A human must never be shown an approval card
    // for a call the deployment has already decided is out of bounds — the same
    // rule that puts policy ahead of the human, one layer further in.
    if payload.kind == PayloadKind::Tool
        && let Some(detail) = super::scope::escape_reason(&session.workspace, governed_input.as_ref())
    {
        inproc::record_policy_denial(&deps.pool, &call, &verdict, WORKSPACE_SCOPE, &detail);
        session.emit(PiEventBody::ToolBlocked {
            tool_use_id: payload.tool_use_id.clone(),
            tool_name,
            reason: format!("[GOVERNANCE] {detail}"),
            policy: Some(WORKSPACE_SCOPE.to_owned()),
        });
        return false;
    }

    if payload.kind == PayloadKind::Prompt || !needs_human(&deps.cfg, &tool_name) {
        return true;
    }

    // Policy cleared it; now ask a person.
    human_gate(deps, session, ApprovalAsk {
        approval_id,
        payload,
        tool_name: &tool_name,
        cleared: cleared_policies(&stages),
    }, &call, &verdict)
    .await
}

/// Put a policy-cleared call to a person and record what they said.
///
/// Reached only after the chain allowed the call and confinement cleared it, so
/// every path here is a human's judgement on top of policy — never instead of it.
async fn human_gate(
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
    inproc::record_human_decision(&deps.pool, call, verdict, outcome);
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

fn emit_stages(
    session: &Arc<PiSession>,
    payload: &GovernancePayload,
    tool_name: &str,
    stages: &[StageOutcome],
) {
    session.emit(PiEventBody::PolicyStages {
        tool_use_id: payload.tool_use_id.clone(),
        tool_name: tool_name.to_owned(),
        stages: stages.iter().map(PolicyStage::from_outcome).collect(),
    });
}

/// The policies an operator can be told already cleared this call.
///
/// The passed stages, plus workspace confinement — which is not one of the
/// chain's policies but has genuinely run by the time a human is asked, and is
/// the refusal a reader of the card is most likely to have been saved by.
/// Failed and skipped stages are excluded: on this path there are none, and if
/// that ever stops being true the card must not start claiming otherwise.
fn cleared_policies(stages: &[StageOutcome]) -> Vec<String> {
    let mut cleared: Vec<String> = stages
        .iter()
        .filter(|s| s.result == StageResult::Pass)
        .map(|s| s.policy.clone())
        .collect();
    cleared.push(WORKSPACE_SCOPE.to_owned());
    cleared
}

/// V1 asks about everything by default. With a read-only tool set nothing is on
/// a "dangerous" list, so a flagged-only mode would never show the approval UI
/// at all.
const fn needs_human(cfg: &PiConfig, _tool_name: &str) -> bool {
    cfg.approve_all
}

fn emit_denial(
    session: &Arc<PiSession>,
    payload: &GovernancePayload,
    tool_name: &str,
    verdict: &PolicyVerdict,
) {
    let reason = verdict
        .reason
        .clone()
        .unwrap_or_else(|| "[GOVERNANCE] denied".to_owned());
    match payload.kind {
        PayloadKind::Prompt => session.emit(PiEventBody::PromptBlocked {
            reason,
            policy: verdict.policy.clone(),
        }),
        PayloadKind::Tool => session.emit(PiEventBody::ToolBlocked {
            tool_use_id: payload.tool_use_id.clone(),
            tool_name: tool_name.to_owned(),
            reason,
            policy: verdict.policy.clone(),
        }),
    };
}

/// One call put to a person, and what policy already established about it.
///
/// Bundled for the same reason [`PiDeps`] is: the pieces are only ever needed
/// together, and passing them separately puts the function over clippy's
/// argument ceiling.
struct ApprovalAsk<'a> {
    approval_id: &'a str,
    payload: &'a GovernancePayload,
    tool_name: &'a str,
    /// The real list of policies that passed. Carried in rather than rebuilt
    /// here, so nothing on this path can assert that a check ran — it can only
    /// relay what the evaluation reported.
    cleared: Vec<String>,
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
