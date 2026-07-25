//! The pi `--mode rpc` wire protocol, as observed rather than as documented.
//!
//! Every shape here was pinned against a captured transcript from pi 0.82.0.
//! Only the frames the proxy acts on are typed; the rest pass through as raw
//! JSON, because pi is a third-party binary and a new event kind must not be a
//! parse failure that kills a session.

use serde::{Deserialize, Serialize};

/// A command written to the child's stdin, one JSON line each.
///
/// Note `prompt` carries **`message`**, not `prompt` — pi answers a `prompt`
/// key with `Cannot read properties of undefined (reading 'startsWith')`.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum RpcCommand {
    Prompt { message: String },
    Steer { message: String },
    FollowUp { message: String },
    Abort,
}

impl RpcCommand {
    /// Serialise as one JSONL line, newline included.
    pub(super) fn to_line(&self) -> Result<String, serde_json::Error> {
        Ok(format!("{}\n", serde_json::to_string(self)?))
    }
}

/// The reply the proxy writes for an `extension_ui_request`.
///
/// `confirmed` is the whole vocabulary the shim can observe — `confirm()`
/// resolves to a bare `bool` — so a denial's *reason* cannot ride back on this
/// channel. The reason is audited and shown in the widget instead.
#[derive(Debug, Serialize)]
pub(super) struct ExtensionUiResponse<'a> {
    #[serde(rename = "type")]
    pub(super) kind: &'static str,
    pub(super) id: &'a str,
    pub(super) confirmed: bool,
}

impl<'a> ExtensionUiResponse<'a> {
    pub(super) const fn new(id: &'a str, confirmed: bool) -> Self {
        Self {
            kind: "extension_ui_response",
            id,
            confirmed,
        }
    }

    pub(super) fn to_line(&self) -> Result<String, serde_json::Error> {
        Ok(format!("{}\n", serde_json::to_string(self)?))
    }
}

/// One line read from the child's stdout.
///
/// Untagged rather than internally tagged: `extension_ui_request` is the only
/// frame the proxy must never miss, so it is matched first and everything else
/// degrades to [`Self::Other`] with the raw value preserved for the widget.
#[derive(Debug)]
pub(super) enum RpcFrame {
    /// The shim asking for a verdict. The governance gate lives here.
    UiRequest(UiRequest),
    /// A reply to a command we sent.
    Response {
        success: bool,
        error: Option<String>,
    },
    /// Any event frame; forwarded to the widget after translation.
    Event(serde_json::Value),
    /// Not JSON at all. Logged, never fatal.
    Unparseable(String),
}

/// An `extension_ui_request` frame.
#[derive(Debug, Deserialize)]
pub(super) struct UiRequest {
    pub(super) id: String,
    pub(super) method: String,
    /// For our shim this is the JSON-encoded [`GovernancePayload`]; for any
    /// other extension it is prose.
    #[serde(default)]
    pub(super) message: String,
}

/// What the shim smuggles through `confirm`'s `message` so the proxy receives
/// typed data instead of prose.
#[derive(Debug, Deserialize)]
pub(super) struct GovernancePayload {
    pub(super) kind: PayloadKind,
    #[serde(default)]
    pub(super) tool_name: Option<String>,
    #[serde(default)]
    pub(super) tool_use_id: Option<String>,
    #[serde(default)]
    pub(super) tool_input: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) prompt: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum PayloadKind {
    Prompt,
    Tool,
}

/// Parse one stdout line.
pub(super) fn parse_frame(line: &str) -> RpcFrame {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return RpcFrame::Unparseable(line.to_owned());
    };
    match value.get("type").and_then(serde_json::Value::as_str) {
        Some("extension_ui_request") => serde_json::from_value::<UiRequest>(value.clone())
            .map_or(RpcFrame::Event(value), RpcFrame::UiRequest),
        Some("response") => RpcFrame::Response {
            success: value
                .get("success")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            error: value
                .get("error")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
        },
        _ => RpcFrame::Event(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_serialises_with_message_not_prompt() {
        let line = RpcCommand::Prompt {
            message: "hi".to_owned(),
        }
        .to_line()
        .unwrap();
        assert_eq!(line, "{\"type\":\"prompt\",\"message\":\"hi\"}\n");
    }

    #[test]
    fn follow_up_uses_snake_case_on_the_wire() {
        let line = RpcCommand::FollowUp {
            message: "x".to_owned(),
        }
        .to_line()
        .unwrap();
        assert!(line.contains("\"follow_up\""), "got {line}");
    }

    #[test]
    fn parses_a_real_ui_request() {
        // Captured verbatim from pi 0.82.0.
        let line = r#"{"type":"extension_ui_request","id":"242791c9","method":"confirm","title":"sp-governance","message":"{\"kind\":\"tool\",\"tool_name\":\"write\",\"tool_use_id\":\"toolu_01\",\"tool_input\":{\"path\":\"README.md\"}}"}"#;
        let RpcFrame::UiRequest(req) = parse_frame(line) else {
            panic!("expected a UiRequest");
        };
        assert_eq!(req.id, "242791c9");
        assert_eq!(req.method, "confirm");
        let payload: GovernancePayload = serde_json::from_str(&req.message).unwrap();
        assert_eq!(payload.kind, PayloadKind::Tool);
        assert_eq!(payload.tool_name.as_deref(), Some("write"));
    }

    #[test]
    fn parses_a_failed_response() {
        let line = r#"{"id":"1","type":"response","command":"prompt","success":false,"error":"boom"}"#;
        let RpcFrame::Response { success, error } = parse_frame(line) else {
            panic!("expected a Response");
        };
        assert!(!success);
        assert_eq!(error.as_deref(), Some("boom"));
    }

    #[test]
    fn unknown_event_kinds_survive_as_events() {
        // A future pi release inventing a frame must not kill the session.
        let RpcFrame::Event(v) = parse_frame(r#"{"type":"brand_new_thing","x":1}"#) else {
            panic!("expected an Event");
        };
        assert_eq!(v["x"], 1);
    }

    #[test]
    fn non_json_is_not_fatal() {
        assert!(matches!(
            parse_frame("Warning: something on stdout"),
            RpcFrame::Unparseable(_)
        ));
    }
}
