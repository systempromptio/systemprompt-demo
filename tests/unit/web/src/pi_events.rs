//! `translate` — the filter between pi's event stream and the browser's.
//!
//! Two properties are pinned here. A frame that must never reach the browser
//! (`tool_execution_start`, which fires *before* the gate resolves) stays
//! dropped, or a denied call renders as running. And a turn that failed at the
//! provider still says why: dropping that frame is what once made a
//! credit-exhausted account look like four turns that began, ended, and
//! explained nothing.

use systemprompt_web_admin::test_support::{
    PiEvent, PiEventBody, PolicyStage, readable_provider_error, translate,
};


/// Captured verbatim from pi 0.82.0 against a gateway that refused the
/// call. Note the absence of any `error` event: this frame is the only
/// place the reason appears.
#[test]
fn a_failed_turn_reports_the_provider_reason() {
    let frame = serde_json::json!({
        "type": "message_end",
        "message": {
            "role": "assistant",
            "content": [],
            "stopReason": "error",
            "errorMessage": "400 {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"Credit exhausted. Your systemprompt credit has been used up — add credit to continue.\"}}"
        }
    });
    let Some(PiEventBody::Error { message }) = translate(&frame) else {
        panic!("a failed turn must surface an error");
    };
    assert_eq!(
        message,
        "Credit exhausted. Your systemprompt credit has been used up — add credit to continue."
    );
}

/// A turn that ended normally must not render an error card.
#[test]
fn a_successful_turn_reports_nothing() {
    let frame = serde_json::json!({
        "type": "message_end",
        "message": { "role": "assistant", "stopReason": "stop", "content": [] }
    });
    assert!(translate(&frame).is_none());
    let user = serde_json::json!({
        "type": "message_end",
        "message": { "role": "user", "content": [] }
    });
    assert!(translate(&user).is_none());
}

/// An error whose body is not the shape we expect still has to reach the
/// viewer — losing it is the failure this whole path exists to fix.
#[test]
fn an_unparseable_provider_error_is_passed_through() {
    assert_eq!(readable_provider_error("upstream timed out"), "upstream timed out");
    assert_eq!(readable_provider_error("502 {not json"), "502 {not json");
}

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
    let event = PiEvent::new(
        7,
        PiEventBody::PolicyStages {
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
    );
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
