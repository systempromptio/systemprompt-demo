//! Wire and audit types for the `/api/public/hooks/govern` `PreToolUse`
//! webhook.
//!
//! The on-the-wire response shape is dictated by the Anthropic Claude Code
//! hook contract ([`HookSpecificOutput`]). Internally everything is typed —
//! audit blobs serialize through [`DecisionAudit`] and per-policy trace
//! through [`ChainEntryOutcome`]; the previous `serde_json::json!` blobs are
//! gone.

use axum::http::HeaderMap;
use serde::Serialize;
use std::sync::Arc;

use sqlx::PgPool;
use systemprompt::identifiers::{AgentId, PluginId, PolicyId, SessionId, UserId};
use systemprompt::oauth::SessionCreationService;
use systemprompt_security::authz::{Decision, DecisionTag};
use systemprompt_security::policy::types::AccessScope;

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
pub(super) struct GovernanceResponse {
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: HookSpecificOutput,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct HookSpecificOutput {
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

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "result", rename_all = "lowercase")]
pub(super) enum ChainEntryResult {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Serialize, Clone)]
pub(super) struct ChainEntryOutcome {
    pub policy_id: PolicyId,
    #[serde(flatten)]
    pub result: ChainEntryResult,
    pub detail: String,
    /// Wall-clock cost of evaluating this policy. Zero for entries that never
    /// ran (disabled, skipped-after-deny, or synthesized outcomes).
    pub duration_ms: f64,
}

#[derive(Debug, Serialize, Clone)]
pub(super) struct PrincipalSnapshot {
    pub user_id: UserId,
    /// The credential's session, attested against `user_sessions` — the same
    /// class of evidence as `ai_requests.session_id`, so the inference and
    /// tool-call halves of the audit spine join on comparable ids. Prefixed
    /// `unattested_` when the lookup failed.
    pub session_id: SessionId,
    /// The `session_id` the hook payload carried: the agent's own local
    /// conversation label (Claude Code mints it). Useful for correlating one
    /// agent run, but the server never issued it, so it is recorded here rather
    /// than in the attested column.
    pub agent_session: Option<SessionId>,
    pub agent_id: Option<AgentId>,
    pub agent_scope: AccessScope,
}

#[derive(Debug, Serialize, Clone)]
pub(super) struct AuditTarget {
    pub tool_name: String,
    pub plugin_id: Option<PluginId>,
}

// Why: the human who answered an approval gate is a distinct actor from the
// session principal, stamped with the click instant rather than the
// audit-write instant.
#[derive(Debug, Serialize, Clone)]
pub(crate) struct ApproverStamp {
    pub user_id: UserId,
    pub username: String,
    pub decided_at: chrono::DateTime<chrono::Utc>,
    /// `"approved"` or `"denied"`.
    pub action: &'static str,
}

// Why: the `decision` and `reason` columns are populated from this same
// blob by the repository layer before it lands in `evaluated_rules`.
#[derive(Debug, Serialize, Clone)]
pub(super) struct DecisionAudit {
    /// The `governance_decisions.id` this blob will land under. Minted by the
    /// caller (not the repository) so surfaces that saw the decision live —
    /// e.g. the pi SSE stream — can hand out the same id as a trace link.
    pub id: String,
    pub decision: Decision,
    pub principal: PrincipalSnapshot,
    pub target: AuditTarget,
    pub chain: Vec<ChainEntryOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approver: Option<ApproverStamp>,
}

// Why: the two services the governance webhook needs, layered as one
// extension so the handler stays inside the argument-count lint.
#[derive(Clone)]
pub(crate) struct GovernanceDeps {
    pub session_service: Arc<SessionCreationService>,
    pub analytics: Arc<dyn systemprompt::traits::AnalyticsProvider>,
}

pub(super) struct AuthDenialParams<'a> {
    pub pool: &'a Arc<PgPool>,
    pub session_id: &'a SessionId,
    pub tool_name: &'a str,
    pub agent_id: Option<&'a AgentId>,
    pub plugin_id: Option<&'a PluginId>,
    pub session_service: &'a Arc<SessionCreationService>,
    pub headers: &'a HeaderMap,
}
