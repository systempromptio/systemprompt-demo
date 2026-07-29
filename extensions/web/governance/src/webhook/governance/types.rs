//! Wire types for the `/api/public/hooks/govern` `PreToolUse` webhook.
//!
//! The on-the-wire response shape is dictated by the Anthropic Claude Code
//! hook contract ([`HookSpecificOutput`]). The audit blob types
//! (`DecisionAudit` and friends) live in [`systemprompt_security::policy`];
//! this module keeps only what is specific to this extension's HTTP surface.

use axum::http::HeaderMap;
use serde::Serialize;
use std::sync::Arc;

use sqlx::PgPool;
use systemprompt::identifiers::{AgentId, PluginId, SessionId, UserId};
use systemprompt::oauth::SessionCreationService;
use systemprompt_security::authz::{Decision, DecisionTag};
use systemprompt_security::policy::types::AccessScope;
use systemprompt_security::policy::{GovernedInput, GovernedTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GovernanceDecision {
    Allow,
    Deny,
}

impl GovernanceDecision {
    pub const fn from_decision(d: &Decision) -> Self {
        match d {
            Decision::Allow { .. } => Self::Allow,
            Decision::Deny { .. } => Self::Deny,
        }
    }
}

impl From<GovernanceDecision> for DecisionTag {
    fn from(d: GovernanceDecision) -> Self {
        match d {
            GovernanceDecision::Allow => Self::Allow,
            GovernanceDecision::Deny => Self::Deny,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct GovernanceResponse {
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: HookSpecificOutput,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: &'static str,
    #[serde(rename = "permissionDecision")]
    pub permission_decision: GovernanceDecision,
    #[serde(
        rename = "permissionDecisionReason",
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_decision_reason: Option<String>,
}

// Why: the two services the governance webhook needs, layered as one
// extension so the handler stays inside the argument-count lint.
#[derive(Clone)]
pub struct GovernanceDeps {
    pub session_service: Arc<SessionCreationService>,
    pub analytics: Arc<dyn systemprompt::traits::AnalyticsProvider>,
}

impl std::fmt::Debug for GovernanceDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GovernanceDeps").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct AuthDenialParams<'a> {
    pub pool: &'a Arc<PgPool>,
    pub session_id: &'a SessionId,
    pub tool_name: &'a str,
    pub agent_id: Option<&'a AgentId>,
    pub plugin_id: Option<&'a PluginId>,
    pub session_service: &'a Arc<SessionCreationService>,
    pub headers: &'a HeaderMap,
}

#[derive(Debug)]
pub struct GovernedCall<'a> {
    pub pool: &'a Arc<PgPool>,
    pub user_id: UserId,
    pub session_id: SessionId,
    pub agent_session: SessionId,
    pub target: &'a GovernedTarget,
    pub agent_id: Option<&'a AgentId>,
    pub plugin_id: Option<&'a PluginId>,
    pub input: &'a GovernedInput,
    pub principal_scope: AccessScope,
}
