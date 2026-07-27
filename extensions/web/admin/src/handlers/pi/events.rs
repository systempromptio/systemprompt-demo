//! What the widget sees.
//!
//! Deliberately not pi's frames: the browser gets a small, stable vocabulary so
//! a pi upgrade that renames `message_update` variants does not break the UI,
//! and so nothing internal (workspace paths, session credentials) can leak into
//! a frame by accident.

use serde::Serialize;

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
        conversation_id: String,
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
        stages: Vec<PolicyStage>,
    },
    ApprovalRequest {
        approval_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        policy_chain: Vec<String>,
        timeout_secs: u64,
    },
    ApprovalResolved {
        approval_id: String,
        outcome: &'static str,
    },
    TurnEnd,
    Stderr {
        line: String,
    },
    Error {
        message: String,
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
    let kind = frame.get("type").and_then(serde_json::Value::as_str)?;
    match kind {
        "turn_start" => Some(PiEventBody::TurnStart),
        "turn_end" => Some(PiEventBody::TurnEnd),
        "message_update" => translate_message_update(frame),
        "message_end" => translate_failed_turn(frame),
        "tool_execution_end" => {
            let tool_name = string_at(frame, "toolName")?;
            Some(PiEventBody::ToolEnd {
                tool_use_id: string_at(frame, "toolCallId"),
                tool_name,
                ok: !frame
                    .get("isError")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            })
        },
        _ => None,
    }
}

fn translate_failed_turn(frame: &serde_json::Value) -> Option<PiEventBody> {
    let message = frame.get("message")?;
    if string_at(message, "role").as_deref() != Some("assistant")
        || string_at(message, "stopReason").as_deref() != Some("error")
    {
        return None;
    }
    let raw = string_at(message, "errorMessage")
        .unwrap_or_else(|| "the provider request failed".to_owned());
    Some(PiEventBody::Error {
        message: readable_provider_error(&raw),
    })
}

/// Pull the human sentence out of a provider error.
///
/// pi hands over the transport status and the raw body — `400 {"type":"error",
/// "error":{"message":"Credit exhausted…"}}` — and the sentence a person needs
/// is the innermost `message`. Rendering the envelope instead buries it in
/// JSON, which in a terminal reads as a crash rather than as an answer.
/// Anything that does not parse is passed through untouched: an unfamiliar
/// error still beats no error.
pub fn readable_provider_error(raw: &str) -> String {
    let Some(start) = raw.find('{') else {
        return raw.to_owned();
    };
    let Ok(body) = serde_json::from_str::<serde_json::Value>(&raw[start..]) else {
        return raw.to_owned();
    };
    body.pointer("/error/message")
        .and_then(serde_json::Value::as_str)
        .or_else(|| body.get("message").and_then(serde_json::Value::as_str))
        .map_or_else(|| raw.to_owned(), ToOwned::to_owned)
}

fn translate_message_update(frame: &serde_json::Value) -> Option<PiEventBody> {
    let ev = frame.get("assistantMessageEvent")?;
    match ev.get("type").and_then(serde_json::Value::as_str)? {
        "text_delta" => Some(PiEventBody::TextDelta {
            text: string_at(ev, "delta").or_else(|| string_at(ev, "text"))?,
        }),
        "thinking_delta" => Some(PiEventBody::ThinkingDelta {
            text: string_at(ev, "delta").or_else(|| string_at(ev, "text"))?,
        }),
        "error" => {
            if string_at(ev, "reason").as_deref() == Some("aborted") {
                return None;
            }
            Some(PiEventBody::Error {
                message: ev
                    .get("error")
                    .and_then(|e| string_at(e, "errorMessage"))
                    .unwrap_or_else(|| "the provider request failed".to_owned()),
            })
        },
        _ => None,
    }
}

fn string_at(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}
