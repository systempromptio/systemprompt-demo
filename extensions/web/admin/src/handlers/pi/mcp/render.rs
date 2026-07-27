//! Turning what the hub said into what the model reads.
//!
//! Everything here is a pure function over a JSON-RPC frame — no identity, no
//! transport, no session. That is what makes the rendering rules testable
//! without a hub to answer, and the rules are worth pinning: a frame that
//! renders blank reads to a model as success.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct McpCallResult {
    /// Rendered text the extension hands back to the model.
    pub text: String,
    /// True when the hub answered with a result rather than an error. The
    /// extension surfaces both — a tool error is information, not a failure of
    /// the transport.
    pub ok: bool,
}

/// Pull the first JSON-RPC frame out of an SSE body.
///
/// The hub replies `text/event-stream` even to a single request, so the frame
/// arrives as a `data:` line rather than as the whole body. A plain JSON body
/// is accepted too, so this keeps working if the hub ever stops streaming.
pub fn first_frame(body: &str) -> Option<serde_json::Value> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        return Some(value);
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<serde_json::Value>(data).ok())
        .find(|value| value.get("result").is_some() || value.get("error").is_some())
}

/// Turn a JSON-RPC frame into the text the model will read.
///
/// A hub error is returned as text with `ok: false` rather than as a transport
/// failure: "no such topic" is an answer the model should see and act on, and
/// turning it into a 502 would tell the model only that something broke.
pub fn render(frame: &serde_json::Value) -> McpCallResult {
    if let Some(error) = frame.get("error") {
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("the documentation hub refused the call");
        return McpCallResult {
            text: message.to_owned(),
            ok: false,
        };
    }

    // `content` carries only the one-line summary — the artifact body lives in
    // `structuredContent` (see core's `response.rs`, which pushes a single
    // summary text block and attaches the artifact separately). Handing the
    // model the summary alone would give it "7 documentation topics available"
    // and none of the topics, so the body is preferred and the summary is the
    // fallback.
    let summary = frame
        .pointer("/result/content")
        .and_then(|c| c.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    let body = frame
        .pointer("/result/structuredContent")
        .and_then(artifact_body);

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

    let is_error = frame
        .pointer("/result/isError")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    McpCallResult {
        text,
        ok: !is_error,
    }
}

/// Find the artifact's rendered text inside a `ToolResponse` payload.
///
/// A recursive search for the first `content` string rather than a fixed
/// pointer: the artifact is flattened into a response envelope whose exact
/// nesting is core's to change, and a hardcoded path would fail silently — the
/// model would get a summary and no one would notice until a demo went quiet.
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
