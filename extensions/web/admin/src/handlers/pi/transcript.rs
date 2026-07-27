//! Rendering a stored conversation into something a resumed agent can read.
//!
//! pi runs with `--no-session` and its RPC accepts only prompt, steer,
//! follow-up and abort — there is no frame that loads prior messages into a
//! fresh child. So a restored conversation's history goes into the workspace as
//! Markdown, and the agent reads it if the question needs it.
//!
//! What is rendered is narrower than what is stored. The transcript the
//! *viewer* sees includes policy chains, approval cards and blocked calls,
//! because the governance spine is the thing they came to watch. The transcript
//! the *model* reads is the conversation: who said what, and which tools ran.
//! Feeding an agent its own audit trail invites it to argue with the policy
//! engine rather than answer the question.

use std::fmt::Write as _;

use sqlx::PgPool;

use crate::repositories::pi::events as event_repo;

const MAX_FRAMES: i64 = 400;

const MAX_CHARS: usize = 4000;

pub(super) async fn render(pool: &PgPool, conversation_id: &str) -> Option<String> {
    let stored = match event_repo::list_conversation_events(pool, conversation_id, 0, MAX_FRAMES)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, conversation_id, "could not read a transcript to resume");
            return None;
        },
    };

    let mut out = String::from(
        "# Earlier in this conversation\n\n\
         This is the record of an earlier run of the conversation you are now in.\n\
         The user can see all of it; you were restarted and cannot. Treat it as\n\
         something you said, not as something you were told about.\n\n",
    );
    let before = out.len();

    for row in stored {
        let Some(section) = section(&row.kind, &row.body) else {
            continue;
        };
        _ = writeln!(out, "{section}\n");
    }

    (out.len() > before).then_some(out)
}

fn section(kind: &str, body: &serde_json::Value) -> Option<String> {
    match kind {
        "user_message" => Some(format!("## User\n\n{}", clamp(text_of(body, "text")?))),
        "text_delta" => Some(format!("## Assistant\n\n{}", clamp(text_of(body, "text")?))),
        "tool_end" => {
            let name = text_of(body, "tool_name")?;
            let ok = body
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let outcome = if ok { "succeeded" } else { "failed" };
            Some(format!("## Tool\n\n`{name}` {outcome}."))
        },
        "tool_blocked" => {
            let name = text_of(body, "tool_name")?;
            Some(format!("## Tool\n\n`{name}` was not permitted."))
        },
        _ => None,
    }
}

fn text_of<'a>(body: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    let text = body.get(key)?.as_str()?;
    (!text.trim().is_empty()).then_some(text)
}

fn clamp(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text.to_owned();
    }
    let half = MAX_CHARS / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = {
        let chars: Vec<char> = text.chars().collect();
        chars[chars.len() - half..].iter().collect()
    };
    format!("{head}\n\n[… elided …]\n\n{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_message_is_untouched() {
        assert_eq!(clamp("hello"), "hello");
    }

    #[test]
    fn a_long_message_keeps_both_ends() {
        let text = format!("START{}END", "x".repeat(MAX_CHARS * 2));
        let clamped = clamp(&text);
        assert!(clamped.starts_with("START"));
        assert!(clamped.ends_with("END"));
        assert!(clamped.contains("[… elided …]"));
    }

    #[test]
    fn governance_frames_are_not_shown_to_the_model() {
        for kind in ["policy_stages", "approval_request", "thinking_delta"] {
            assert!(
                section(
                    kind,
                    &serde_json::json!({ "text": "x", "tool_name": "read" })
                )
                .is_none(),
                "{kind} should not reach the resumed agent"
            );
        }
    }

    #[test]
    fn a_blocked_call_names_the_tool_but_not_the_reason() {
        let body = serde_json::json!({
            "tool_name": "read",
            "reason": "workspace_scope: /etc/passwd is outside the workspace",
        });
        let rendered = section("tool_blocked", &body).unwrap_or_default();
        assert!(rendered.contains("read"));
        assert!(!rendered.contains("workspace_scope"));
    }

    #[test]
    fn an_empty_message_is_skipped() {
        assert!(section("user_message", &serde_json::json!({ "text": "   " })).is_none());
    }
}
