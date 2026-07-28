//! The conversation itself, and the digest that seals the page.
//!
//! The trace timeline proves what policy decided; the transcript shows what
//! the human asked and what the model said back, so a reader can judge the
//! decisions in context. The integrity digest is a SHA-256 over the exact
//! view data rendered below it, keyed to the attested session — re-fetch the
//! page, re-hash what it shows, and a changed digest means changed evidence.

use serde::Serialize;
use sha2::{Digest, Sha256};
use systemprompt::identifiers::ContextId;

use crate::repositories::pi::events::TranscriptMessage;

#[derive(Debug, Serialize)]
pub(super) struct MessageView {
    role: &'static str,
    text: String,
    at: String,
}

pub(super) fn message_views(messages: &[TranscriptMessage]) -> Vec<MessageView> {
    messages
        .iter()
        .filter(|m| !m.text.trim().is_empty())
        .map(|m| MessageView {
            role: if m.kind == "user_message" {
                "user"
            } else {
                "assistant"
            },
            text: m.text.clone(),
            at: m.at.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string(),
        })
        .collect()
}

#[derive(Debug, Serialize)]
struct AttestedPayload<'a> {
    conversation_id: &'a str,
    attested_session_id: &'a str,
    messages: &'a [MessageView],
    events: &'a serde_json::Value,
}

pub(super) fn integrity_digest(
    conversation_id: &ContextId,
    attested_session_id: &str,
    messages: &[MessageView],
    events: &serde_json::Value,
) -> String {
    let payload = AttestedPayload {
        conversation_id: conversation_id.as_str(),
        attested_session_id,
        messages,
        events,
    };
    let bytes = serde_json::to_vec(&payload).unwrap_or_default();
    let hash = Sha256::digest(&bytes);
    format!("{hash:x}")
}
