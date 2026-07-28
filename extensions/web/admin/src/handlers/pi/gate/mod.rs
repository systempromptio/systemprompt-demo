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
use systemprompt::identifiers::{PolicyId, SessionId};
use systemprompt::traits::AnalyticsProvider;
use systemprompt_security::policy::types::AccessScope;

use super::config::PiConfig;
use super::events::PiEventBody;
use super::rpc::{GovernancePayload, PayloadKind};
use super::session::PiSession;
use super::stage::PolicyStage;
use crate::handlers::webhook::governance::inproc::{
    self, GovernedCall, PROMPT_TOOL_NAME, PolicyVerdict,
};
use crate::handlers::webhook::governance::stages::{StageOutcome, StageResult};

mod human;

use human::{ApprovalAsk, human_gate};

const WORKSPACE_SCOPE: &str = "workspace_scope";

fn workspace_scope_policy() -> PolicyId {
    PolicyId::new(WORKSPACE_SCOPE)
}

pub(super) struct PiDeps {
    pub(super) pool: Arc<PgPool>,
    pub(super) analytics: Arc<dyn AnalyticsProvider>,
    pub(super) session_service: Arc<systemprompt::oauth::SessionCreationService>,
    pub(super) cfg: PiConfig,
}

fn governed_parts(payload: &GovernancePayload) -> (String, Option<serde_json::Value>) {
    let tool_name = match payload.kind {
        PayloadKind::Prompt => PROMPT_TOOL_NAME.to_owned(),
        PayloadKind::Tool => payload
            .tool_name
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
    };
    let governed_input = match payload.kind {
        PayloadKind::Prompt => payload
            .prompt
            .as_ref()
            .map(|p| serde_json::json!({ "prompt": p })),
        PayloadKind::Tool => payload.tool_input.clone(),
    };
    (tool_name, governed_input)
}

pub(super) async fn decide(
    deps: &PiDeps,
    session: &Arc<PiSession>,
    approval_id: &str,
    payload: &GovernancePayload,
) -> bool {
    session.touch();

    let (tool_name, governed_input) = governed_parts(payload);

    let agent_session = SessionId::new(session.conversation_id.clone());
    let call = GovernedCall {
        tool_name: &tool_name,
        user_id: &session.user_id,
        agent_session: &agent_session,
        tool_input: governed_input.as_ref(),
        scope_ceiling: AccessScope::User,
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
        && let Some(detail) =
            super::scope::escape_reason(&session.workspace, governed_input.as_ref())
    {
        inproc::record_policy_denial(
            &deps.pool,
            &call,
            &verdict,
            &workspace_scope_policy(),
            &detail,
        )
        .await;
        session.emit(PiEventBody::ToolBlocked {
            tool_use_id: payload.tool_use_id.clone(),
            tool_name,
            reason: format!("[GOVERNANCE] {detail}"),
            policy: Some(WORKSPACE_SCOPE.to_owned()),
        });
        return false;
    }

    if payload.kind == PayloadKind::Prompt {
        return true;
    }

    if !needs_human(&deps.cfg, &tool_name) {
        session.emit(PiEventBody::ApprovalAuto {
            tool_name,
            tool_input: governed_input.unwrap_or(serde_json::Value::Null),
            policy_chain: cleared_policies(&stages),
        });
        return true;
    }

    human_gate(
        deps,
        session,
        ApprovalAsk {
            approval_id,
            payload,
            tool_name: &tool_name,
            cleared: cleared_policies(&stages),
        },
        &call,
        &verdict,
    )
    .await
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
