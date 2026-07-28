//! What the widget sees.
//!
//! Deliberately not pi's frames: the browser gets a small, stable vocabulary so
//! a pi upgrade that renames `message_update` variants does not break the UI,
//! and so nothing internal (workspace paths, session credentials) can leak into
//! a frame by accident.

use serde::{Deserialize as _, Serialize};
use systemprompt::identifiers::ArtifactId;

pub use super::events_error::{
    CREDIT_EXHAUSTED_CODE, CREDIT_EXHAUSTED_NEEDLE, ErrorDeduper, ErrorKind,
    readable_provider_error, upgrade_legacy_error,
};
use super::rpc;
use super::stage::PolicyStage;

/// One frame to one widget. `seq` is per-session and monotonic so a reconnect
/// can resume with `Last-Event-ID`.
#[derive(Debug, Clone, Serialize)]
pub struct PiEvent {
    seq: u64,
    #[serde(flatten)]
    body: PiEventBody,
}

impl PiEvent {
    pub const fn new(seq: u64, body: PiEventBody) -> Self {
        Self { seq, body }
    }

    pub(super) const fn seq(&self) -> u64 {
        self.seq
    }

    pub(super) const fn body(&self) -> &PiEventBody {
        &self.body
    }
}

impl PiEventBody {
    pub(super) const fn kind(&self) -> &'static str {
        match self {
            Self::SessionReady { .. } => "session_ready",
            Self::TurnStart => "turn_start",
            Self::UserMessage { .. } => "user_message",
            Self::TextDelta { .. } => "text_delta",
            Self::ThinkingDelta { .. } => "thinking_delta",
            Self::ToolStart { .. } => "tool_start",
            Self::ToolEnd { .. } => "tool_end",
            Self::ToolBlocked { .. } => "tool_blocked",
            Self::PromptBlocked { .. } => "prompt_blocked",
            Self::PolicyStages { .. } => "policy_stages",
            Self::ApprovalRequest { .. } => "approval_request",
            Self::ApprovalAuto { .. } => "approval_auto",
            Self::ToolArtifact { .. } => "tool_artifact",
            Self::ApprovalResolved { .. } => "approval_resolved",
            Self::TurnEnd => "turn_end",
            Self::Stderr { .. } => "stderr",
            Self::Error { .. } => "error",
            Self::Exit { .. } => "exit",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PiEventBody {
    SessionReady {
        conversation_id: systemprompt::identifiers::ContextId,
    },
    TurnStart,
    UserMessage {
        text: String,
        via: &'static str,
    },
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        text: String,
    },
    ToolStart {
        tool_use_id: Option<String>,
        tool_name: String,
        // JSON: arbitrary tool arguments, shape owned by whichever tool ran
        tool_input: serde_json::Value,
    },
    ToolEnd {
        tool_use_id: Option<String>,
        tool_name: String,
        ok: bool,
    },
    ToolBlocked {
        tool_use_id: Option<String>,
        tool_name: String,
        reason: String,
        policy: Option<String>,
    },
    PromptBlocked {
        reason: String,
        policy: Option<String>,
    },
    PolicyStages {
        tool_use_id: Option<String>,
        tool_name: String,
        /// The `governance_decisions.id` these stages were recorded under, so
        /// the UI can link straight to the audit trail for this call.
        decision_id: String,
        stages: Vec<PolicyStage>,
    },
    ApprovalRequest {
        approval_id: String,
        tool_name: String,
        // JSON: arbitrary tool arguments, rendered to the approver as-is
        tool_input: serde_json::Value,
        policy_chain: Vec<String>,
        timeout_secs: u64,
    },
    ApprovalResolved {
        approval_id: String,
        outcome: &'static str,
        /// Display name of the human who answered; absent on system paths
        /// (timeout, abandonment) so the UI can style attribution honestly.
        #[serde(skip_serializing_if = "Option::is_none")]
        approved_by: Option<String>,
        /// RFC 3339 click instant — the moment the human decided, not the
        /// moment the audit row landed.
        #[serde(skip_serializing_if = "Option::is_none")]
        decided_at: Option<String>,
        actor: &'static str,
    },
    /// The structured artifact a tool's response was persisted as. The hub
    /// stores one per successful call; this frame is the pointer the widget
    /// needs to offer "view result" without carrying the payload itself.
    ToolArtifact {
        tool_name: String,
        artifact_id: ArtifactId,
        artifact_type: String,
        title: Option<String>,
        server_name: String,
    },
    /// A call the gate cleared without asking anyone — `approve_all` is off, so
    /// policy alone decided. Emitted so the transcript still shows what ran and
    /// under which chain, instead of the call passing silently.
    ApprovalAuto {
        tool_name: String,
        // JSON: arbitrary tool arguments, rendered to the viewer as-is
        tool_input: serde_json::Value,
        policy_chain: Vec<String>,
    },
    TurnEnd,
    Stderr {
        line: String,
    },
    Error {
        message: String,
        kind: ErrorKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<&'static str>,
    },
    Exit {
        code: Option<i32>,
    },
}

/// Translate one pi event frame into zero or one widget frames.
///
/// Dropping a frame is a filtering decision, not a fallback: the browser never
/// sees `tool_execution_start`, so a call the gate later denies cannot have
/// already rendered as running. See [`PiEventBody::ToolStart`].
pub fn translate(frame: &serde_json::Value) -> Option<PiEventBody> {
    match rpc::PiWireFrame::deserialize(frame).ok()? {
        rpc::PiWireFrame::TurnStart => Some(PiEventBody::TurnStart),
        rpc::PiWireFrame::TurnEnd => Some(PiEventBody::TurnEnd),
        rpc::PiWireFrame::MessageUpdate { event } => translate_message_update(event),
        rpc::PiWireFrame::MessageEnd { message } => translate_failed_turn(message),
        rpc::PiWireFrame::ToolExecutionEnd {
            tool_name,
            tool_call_id,
            is_error,
        } => Some(PiEventBody::ToolEnd {
            tool_use_id: tool_call_id,
            tool_name,
            ok: !is_error,
        }),
        rpc::PiWireFrame::Other => None,
    }
}

fn translate_failed_turn(message: rpc::EndedMessage) -> Option<PiEventBody> {
    if message.role != "assistant" || message.stop_reason.as_deref() != Some("error") {
        return None;
    }
    let raw = message
        .error_message
        .unwrap_or_else(|| "the provider request failed".to_owned());
    Some(PiEventBody::provider_error(&raw))
}

fn translate_message_update(event: rpc::AssistantMessageEvent) -> Option<PiEventBody> {
    match event {
        rpc::AssistantMessageEvent::TextDelta { delta, text } => Some(PiEventBody::TextDelta {
            text: delta.or(text)?,
        }),
        rpc::AssistantMessageEvent::ThinkingDelta { delta, text } => {
            Some(PiEventBody::ThinkingDelta {
                text: delta.or(text)?,
            })
        },
        rpc::AssistantMessageEvent::Error { reason, error } => {
            if reason.as_deref() == Some("aborted") {
                return None;
            }
            let raw = error
                .and_then(|e| e.error_message)
                .unwrap_or_else(|| "the provider request failed".to_owned());
            Some(PiEventBody::provider_error(&raw))
        },
        rpc::AssistantMessageEvent::Other => None,
    }
}
