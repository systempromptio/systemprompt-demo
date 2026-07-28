//! What a resumed pi agent is allowed to read back from its own transcript:
//! clamping keeps both ends of a long message, and governance frames never
//! reach the model.

use systemprompt_web_admin::test_support::{MAX_CHARS, clamp, section};

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
