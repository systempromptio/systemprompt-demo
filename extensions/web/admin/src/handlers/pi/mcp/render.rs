//! Turning what the hub said into what the model reads.
//!
//! Everything here is a pure function over a JSON-RPC frame — no identity, no
//! transport, no session. That is what makes the rendering rules testable
//! without a hub to answer, and the rules are worth pinning: a frame that
//! renders blank reads to a model as success.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct McpCallResult {
    pub text: String,
    /// True when the hub answered with a result rather than an error. The
    /// extension surfaces both — a tool error is information, not a failure of
    /// the transport.
    pub ok: bool,
}

#[derive(Debug, Deserialize)]
pub struct McpResponseFrame {
    #[serde(default)]
    result: Option<McpToolResult>,
    #[serde(default)]
    error: Option<McpError>,
}

#[derive(Debug, Deserialize)]
struct McpError {
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpToolResult {
    #[serde(default)]
    content: Vec<McpContentItem>,
    #[serde(default)]
    is_error: bool,
    // JSON: artifact payload — each tool owns its own shape, walked generically
    #[serde(default)]
    structured_content: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct McpContentItem {
    #[serde(default)]
    text: Option<String>,
}

impl McpResponseFrame {
    fn is_answer(&self) -> bool {
        self.result.is_some() || self.error.is_some()
    }
}

/// Pull the first JSON-RPC frame out of an SSE body.
///
/// The hub replies `text/event-stream` even to a single request, so the frame
/// arrives as a `data:` line rather than as the whole body. A plain JSON body
/// is accepted too, so this keeps working if the hub ever stops streaming.
pub fn first_frame(body: &str) -> Option<McpResponseFrame> {
    if let Ok(frame) = serde_json::from_str::<McpResponseFrame>(body) {
        return Some(frame);
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<McpResponseFrame>(data).ok())
        .find(McpResponseFrame::is_answer)
}

/// Turn a JSON-RPC frame into the text the model will read.
///
/// A hub error is returned as text with `ok: false` rather than as a transport
/// failure: "no such topic" is an answer the model should see and act on, and
/// turning it into a 502 would tell the model only that something broke.
pub fn render(frame: &McpResponseFrame) -> McpCallResult {
    if let Some(error) = &frame.error {
        return McpCallResult {
            text: error
                .message
                .clone()
                .unwrap_or_else(|| "the documentation hub refused the call".to_owned()),
            ok: false,
        };
    }

    let Some(result) = &frame.result else {
        return McpCallResult {
            text: String::new(),
            ok: true,
        };
    };

    let summary = result
        .content
        .iter()
        .filter_map(|item| item.text.as_deref())
        .collect::<Vec<_>>()
        .join("\n");

    let body = result.structured_content.as_ref().and_then(artifact_body);

    let text = match body {
        Some(body) if !body.trim().is_empty() => {
            if summary.trim().is_empty() {
                body
            } else {
                format!("{summary}\n\n{body}")
            }
        },
        _ => summary,
    };

    McpCallResult {
        text,
        ok: !result.is_error,
    }
}

fn artifact_body(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(s)) = map.get("content") {
                return Some(s.clone());
            }
            map.values().find_map(artifact_body)
        },
        serde_json::Value::Array(items) => items.iter().find_map(artifact_body),
        _ => None,
    }
}
