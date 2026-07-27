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
pub enum RpcCommand {
    Prompt { message: String },
    Steer { message: String },
    FollowUp { message: String },
    Abort,
}

impl RpcCommand {
    /// Serialise as one JSONL line, newline included.
    pub fn to_line(&self) -> Result<String, serde_json::Error> {
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
    pub kind: &'static str,
    pub id: &'a str,
    pub confirmed: bool,
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
pub enum RpcFrame {
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
pub struct UiRequest {
    pub id: String,
    pub method: String,
    /// For our shim this is the JSON-encoded [`GovernancePayload`]; for any
    /// other extension it is prose.
    #[serde(default)]
    pub message: String,
}

/// What the shim smuggles through `confirm`'s `message` so the proxy receives
/// typed data instead of prose.
#[derive(Debug, Deserialize)]
pub struct GovernancePayload {
    pub kind: PayloadKind,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PayloadKind {
    Prompt,
    Tool,
}

/// Parse one stdout line.
pub fn parse_frame(line: &str) -> RpcFrame {
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
