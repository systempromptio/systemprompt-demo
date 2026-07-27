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
pub(super) struct PiEvent {
    pub(super) seq: u64,
    #[serde(flatten)]
    pub(super) body: PiEventBody,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum PiEventBody {
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
pub(super) fn translate(frame: &serde_json::Value) -> Option<PiEventBody> {
    let kind = frame.get("type").and_then(serde_json::Value::as_str)?;
    match kind {
        "turn_start" => Some(PiEventBody::TurnStart),
        "turn_end" => Some(PiEventBody::TurnEnd),
        "message_update" => translate_message_update(frame),
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

#[cfg(test)]
#[expect(clippy::panic, reason = "assertions in tests")]
mod tests {
    use super::*;

    #[test]
    fn text_delta_is_forwarded() {
        let frame = serde_json::json!({
            "type": "message_update",
            "assistantMessageEvent": { "type": "text_delta", "delta": "hello" }
        });
        let Some(PiEventBody::TextDelta { text }) = translate(&frame) else {
            panic!("expected TextDelta");
        };
        assert_eq!(text, "hello");
    }

    #[test]
    fn tool_execution_start_is_not_forwarded() {
        // It fires before the governance gate resolves and also fires for
        // blocked calls; rendering it would show denied calls as running.
        let frame = serde_json::json!({ "type": "tool_execution_start", "toolName": "write" });
        assert!(translate(&frame).is_none());
    }

    #[test]
    fn toolcall_deltas_are_dropped_as_noise() {
        let frame = serde_json::json!({
            "type": "message_update",
            "assistantMessageEvent": { "type": "toolcall_delta", "delta": "{\"pa" }
        });
        assert!(translate(&frame).is_none());
    }

    #[test]
    fn provider_failure_surfaces_as_an_error() {
        // Shape taken from pi-ai's `AssistantMessageEvent`: the reason is on the
        // event, the message is on the partial assistant message it carries.
        let frame = serde_json::json!({
            "type": "message_update",
            "assistantMessageEvent": {
                "type": "error",
                "reason": "error",
                "error": { "role": "assistant", "stopReason": "error",
                           "errorMessage": "401 unknown or revoked session" }
            }
        });
        let Some(PiEventBody::Error { message }) = translate(&frame) else {
            panic!("expected Error");
        };
        assert_eq!(message, "401 unknown or revoked session");
    }

    #[test]
    fn a_user_abort_is_not_an_error() {
        let frame = serde_json::json!({
            "type": "message_update",
            "assistantMessageEvent": { "type": "error", "reason": "aborted", "error": {} }
        });
        assert!(translate(&frame).is_none());
    }

    #[test]
    fn policy_stages_serialises_as_a_tagged_frame() {
        let event = PiEvent {
            seq: 7,
            body: PiEventBody::PolicyStages {
                tool_use_id: Some("tu_1".to_owned()),
                tool_name: "read".to_owned(),
                stages: vec![
                    PolicyStage {
                        policy: "scope_check".to_owned(),
                        result: "pass",
                        detail: "read is in scope".to_owned(),
                    },
                    PolicyStage {
                        policy: "rate_limit".to_owned(),
                        result: "skip",
                        detail: "disabled".to_owned(),
                    },
                ],
            },
        };
        let Ok(v) = serde_json::to_value(&event) else {
            panic!("a frame of owned strings cannot fail to serialise");
        };
        assert_eq!(v["type"], "policy_stages");
        assert_eq!(v["seq"], 7);
        assert_eq!(v["stages"][0]["policy"], "scope_check");
        assert_eq!(v["stages"][0]["result"], "pass");
        // Skip must survive as itself. Collapsing it to a pass would tell the
        // visitor a check cleared the call when it never ran.
        assert_eq!(v["stages"][1]["result"], "skip");
    }

    #[test]
    fn unknown_frames_are_dropped_not_fatal() {
        assert!(translate(&serde_json::json!({ "type": "future_thing" })).is_none());
        assert!(translate(&serde_json::json!({ "no_type": 1 })).is_none());
    }
}
