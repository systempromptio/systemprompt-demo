//! The RPC command surface, which is ungoverned by design — pi runs whatever
//! command type it is handed, with no `tool_call` hook firing at all. These
//! pin the wire shapes, so no client string can ever reach pi as a command.

use systemprompt_web_admin::test_support::{
    GovernancePayload, PayloadKind, RpcCommand, RpcFrame, parse_frame,
};

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
