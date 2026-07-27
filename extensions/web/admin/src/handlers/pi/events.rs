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

    /// Monotonic per session, so a reconnect can resume with `Last-Event-ID`.
    pub(super) const fn seq(&self) -> u64 {
        self.seq
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PiEventBody {
    /// The child is up and accepting prompts.
    SessionReady {
        conversation_id: String,
    },
    TurnStart,
    /// Streaming assistant prose.
    TextDelta {
        text: String,
    },
    /// Streaming chain-of-thought, rendered dimmed and collapsible.
    ThinkingDelta {
        text: String,
    },
    /// A tool the model wants to run. Emitted from the governance gate, not
    /// from `tool_execution_start` — that frame fires before the gate resolves
    /// and also fires for blocked calls, so it is not a "running" signal.
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
    /// Policy or a human refused it. Carries the real reason, which the model
    /// never sees (`confirm` answers a bare boolean).
    ToolBlocked {
        tool_use_id: Option<String>,
        tool_name: String,
        reason: String,
        policy: Option<String>,
    },
    /// The prompt itself was refused; no provider request was made.
    PromptBlocked {
        reason: String,
        policy: Option<String>,
    },
    /// What the policy chain did, stage by stage, for the call named here.
    ///
    /// Emitted for every governed call — allow *and* deny — because a gate that
    /// is only visible when it blocks something looks like an error path rather
    /// than a pipeline. Always derived from the chain that ran, never from a
    /// fixed list: the widget cannot show a check that did not happen.
    PolicyStages {
        tool_use_id: Option<String>,
        tool_name: String,
        stages: Vec<PolicyStage>,
    },
    /// A call is waiting on a human. The widget renders a card per id.
    ApprovalRequest {
        approval_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        /// Every policy that passed, so the operator sees what already cleared
        /// it rather than being asked to trust a bare prompt.
        policy_chain: Vec<String>,
        timeout_secs: u64,
    },
    /// Cleared — by this viewer, another viewer, or the timeout.
    ApprovalResolved {
        approval_id: String,
        outcome: &'static str,
    },
    TurnEnd,
    /// A line pi wrote to stderr. Surfaced because a provider misconfiguration
    /// shows up here and nowhere else.
    Stderr {
        line: String,
    },
    Error {
        message: String,
    },
    /// The child is gone; the widget stops accepting input.
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
        // Everything else — agent_start, message_start/end, tool_execution_start
        // (fires before the gate, so not a "running" signal) — is dropped.
        _ => None,
    }
}

/// Surface a turn that failed at the provider.
///
/// Measured against pi 0.82.0, and the shape is not what the docs suggest: a
/// rejected provider call emits **no** `error` event at all. The whole turn is
/// `turn_start`, an assistant `message_end` carrying `stopReason: "error"` and
/// an `errorMessage`, then `turn_end` — repeated once per automatic retry.
///
/// Dropping it is what made a credit-exhausted account look like four turns
/// that each began, ended, and said nothing: the terminal sat there having
/// silently swallowed the one sentence that explained itself. A turn that
/// produced no output and no reason is the worst answer this widget can give,
/// because the viewer's only conclusion is that the feature is broken.
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
        // The provider call failed. pi carries the reason on the partial
        // assistant message rather than on the event, and emits nothing else —
        // dropping this is what made a rejected credential look like a turn
        // that started, ended, and said nothing.
        "error" => {
            if string_at(ev, "reason").as_deref() == Some("aborted") {
                // Someone pressed stop. `ApprovalResolved` and `TurnEnd` already
                // say so; an error card would be inventing a fault.
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
