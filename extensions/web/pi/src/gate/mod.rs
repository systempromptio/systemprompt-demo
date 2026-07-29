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
//!
//! A standing approval (armed from the approval card, scoped to one tool for
//! the life of the session) skips the prompt but not the pipeline: the chain
//! still runs, and each skipped prompt is audited under the stamp of the person
//! who armed it. It is a person answering early, not a person being bypassed.

use std::sync::Arc;

use sqlx::PgPool;
use systemprompt::identifiers::{McpToolName, PolicyId, SessionId};
use systemprompt::traits::AnalyticsProvider;
use systemprompt_security::policy::types::AccessScope;
use systemprompt_security::policy::{GovernedInput, GovernedTarget, McpToolInput};

use super::config::PiConfig;
use super::events::PiEventBody;
use super::rpc::{GovernancePayload, PayloadKind};
use super::session::PiSession;
use super::stage::PolicyStage;
use systemprompt_security::policy::{ApproverStamp, AuditOrigin};
use systemprompt_web_governance::webhook::governance::inproc::{
    self, InprocCall, HumanOutcome, PolicyVerdict,
};
use systemprompt_web_governance::webhook::governance::stages::{StageOutcome, StageResult};

mod human;

use human::{ApprovalAsk, human_gate};

const WORKSPACE_SCOPE: &str = "workspace_scope";

const STANDING_ACTION: &str = "auto_approved";

fn workspace_scope_policy() -> PolicyId {
    PolicyId::new(WORKSPACE_SCOPE)
}

pub(super) struct PiDeps {
    pub(super) pool: Arc<PgPool>,
    pub(super) analytics: Arc<dyn AnalyticsProvider>,
    pub(super) session_service: Arc<systemprompt::oauth::SessionCreationService>,
    pub(super) cfg: PiConfig,
}

fn governed_parts(payload: &GovernancePayload) -> (GovernedTarget, GovernedInput) {
    match payload.kind {
        PayloadKind::Prompt => (
            GovernedTarget::Prompt,
            GovernedInput::prompt(payload.prompt.clone().unwrap_or_default()),
        ),
        PayloadKind::Tool => (
            payload
                .tool_name
                .as_ref()
                .map_or(GovernedTarget::Unknown, |name| GovernedTarget::Tool {
                    tool: McpToolName::new(name.clone()),
                }),
            GovernedInput::tool_arguments(McpToolInput::new(
                payload.tool_input.clone().unwrap_or_default(),
            )),
        ),
    }
}

pub(super) async fn decide(
    deps: &PiDeps,
    session: &Arc<PiSession>,
    approval_id: &str,
    payload: &GovernancePayload,
) -> bool {
    session.touch();

    let (target, input) = governed_parts(payload);
    let tool_name = target.as_str().to_owned();
    let arguments = input.arguments().map(McpToolInput::as_value);

    let agent_session = SessionId::new(session.conversation_id.clone());
    let call_id = session.calls.mint(&tool_name, arguments);
    let call = InprocCall {
        target: &target,
        user_id: &session.user_id,
        agent_session: &agent_session,
        input: &input,
        scope_ceiling: AccessScope::User,
        call_id: &call_id,
        origin: AuditOrigin::Governed,
    };

    let verdict = inproc::govern_call(
        &deps.pool,
        &deps.analytics,
        &session.attested_session,
        &call,
    )
    .await;

    let stages = verdict.stages();
    emit_stages(session, payload, &tool_name, &verdict.decision_id, &stages);

    if !verdict.allowed {
        emit_denial(session, payload, &tool_name, &verdict);
        return false;
    }

    if payload.kind == PayloadKind::Tool
        && let Some(detail) = super::scope::escape_reason(&session.workspace, arguments)
    {
        deny_workspace_escape(
            deps,
            session,
            payload,
            EscapeDenial {
                call: &call,
                verdict: &verdict,
                detail: &detail,
            },
        )
        .await;
        return false;
    }

    if payload.kind == PayloadKind::Prompt {
        return true;
    }

    consent(
        deps,
        session,
        Cleared {
            payload,
            approval_id,
            tool_name: &tool_name,
            arguments,
            stages: &stages,
            call: &call,
            verdict: &verdict,
        },
    )
    .await
}

struct Cleared<'a> {
    payload: &'a GovernancePayload,
    approval_id: &'a str,
    tool_name: &'a str,
    arguments: Option<&'a serde_json::Value>,
    stages: &'a [StageOutcome],
    call: &'a InprocCall<'a>,
    verdict: &'a PolicyVerdict,
}

async fn consent(deps: &PiDeps, session: &Arc<PiSession>, cleared: Cleared<'_>) -> bool {
    let Cleared {
        payload,
        approval_id,
        tool_name,
        arguments,
        stages,
        call,
        verdict,
    } = cleared;

    if !needs_human(&deps.cfg, tool_name) {
        emit_auto_approval(session, tool_name, arguments, stages, None);
        return true;
    }

    if let Some(attribution) = session.standing_approval(tool_name) {
        let username = attribution.username.clone();
        let stamp = ApproverStamp {
            user_id: attribution.user_id,
            username: attribution.username,
            decided_at: attribution.decided_at,
            action: STANDING_ACTION,
        };
        inproc::record_human_decision(
            &deps.pool,
            call,
            verdict,
            HumanOutcome::Approved,
            Some(stamp),
        )
        .await;
        emit_auto_approval(session, tool_name, arguments, stages, Some(username));
        return true;
    }

    human_gate(
        deps,
        session,
        ApprovalAsk {
            approval_id,
            payload,
            tool_name,
            cleared: cleared_policies(stages),
        },
        call,
        verdict,
    )
    .await
}

struct EscapeDenial<'a> {
    call: &'a InprocCall<'a>,
    verdict: &'a PolicyVerdict,
    detail: &'a str,
}

// Why: policy passed but the path escapes the session workspace — recorded as
// a denial under its own policy id so the audit shows which floor tripped.
async fn deny_workspace_escape(
    deps: &PiDeps,
    session: &Arc<PiSession>,
    payload: &GovernancePayload,
    denial: EscapeDenial<'_>,
) {
    let EscapeDenial {
        call,
        verdict,
        detail,
    } = denial;
    inproc::record_policy_denial(&deps.pool, call, verdict, &workspace_scope_policy(), detail)
        .await;
    session.emit(PiEventBody::ToolBlocked {
        tool_use_id: payload.tool_use_id.clone(),
        tool_name: call.target.as_str().to_owned(),
        reason: format!("[GOVERNANCE] {detail}"),
        policy: Some(WORKSPACE_SCOPE.to_owned()),
    });
}

fn emit_auto_approval(
    session: &Arc<PiSession>,
    tool_name: &str,
    governed_input: Option<&serde_json::Value>,
    stages: &[StageOutcome],
    standing_by: Option<String>,
) {
    session.emit(PiEventBody::ApprovalAuto {
        tool_name: tool_name.to_owned(),
        tool_input: governed_input.cloned().unwrap_or(serde_json::Value::Null),
        policy_chain: cleared_policies(stages),
        standing_by,
    });
}

fn emit_stages(
    session: &Arc<PiSession>,
    payload: &GovernancePayload,
    tool_name: &str,
    decision_id: &str,
    stages: &[StageOutcome],
) {
    session.emit(PiEventBody::PolicyStages {
        tool_use_id: payload.tool_use_id.clone(),
        tool_name: tool_name.to_owned(),
        decision_id: decision_id.to_owned(),
        stages: stages.iter().map(PolicyStage::from_outcome).collect(),
    });
}

fn cleared_policies(stages: &[StageOutcome]) -> Vec<String> {
    let mut cleared: Vec<String> = stages
        .iter()
        .filter(|s| s.result == StageResult::Pass)
        .map(|s| s.policy.clone())
        .collect();
    cleared.push(WORKSPACE_SCOPE.to_owned());
    cleared
}

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
