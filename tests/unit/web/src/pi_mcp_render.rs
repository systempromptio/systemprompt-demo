//! The MCP proxy's two pure decisions: what the child may reach, and what a
//! hub frame reads as once it gets to the model.
//!
//! Both fail silently if they regress. An allowlist that quietly widened would
//! hand the child tools nobody reviewed; a frame that renders blank reads to a
//! model as success.

use systemprompt_web_admin::test_support::{FORWARDABLE, first_frame, render};

/// The allowlist is the whole of the proxy's authority over what the child
/// can reach, so an accidental `*` would be silent.
#[test]
fn the_allowlist_is_explicit() {
    assert!(FORWARDABLE.contains(&"list_topics"));
    assert!(FORWARDABLE.contains(&"fetch_remote_docs"));
    assert!(!FORWARDABLE.contains(&"bash"));
    assert!(!FORWARDABLE.contains(&""));
}

/// The summary alone is not the answer — the topics, the audit rows, and
/// the refusal text all live in the artifact body.
#[test]
fn prefers_the_artifact_body_over_the_summary() {
    let frame = first_frame(
        // `r##` because the payload contains `"#` — a Markdown heading
        // right after a quote — which would close an `r#` literal.
        r##"{"jsonrpc":"2.0","id":2,"result":{
             "content":[{"type":"text","text":"7 documentation topics available"}],
             "structuredContent":{"output":{"x-artifact-type":"text",
               "content":"# Topics\n\n- governance-pipeline"}}}}"##,
    )
    .expect("a frame");
    let rendered = render(&frame);
    assert!(rendered.text.contains("governance-pipeline"), "{}", rendered.text);
    assert!(rendered.text.contains("7 documentation topics"), "{}", rendered.text);
}

/// A response with no artifact must still say something rather than going
/// blank, because a blank tool result reads to a model as success.
#[test]
fn falls_back_to_the_summary_when_there_is_no_artifact() {
    let frame = first_frame(
        r#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"done"}]}}"#,
    )
    .expect("a frame");
    assert_eq!(render(&frame).text, "done");
}

#[test]
fn reads_a_frame_out_of_an_sse_body() {
    // Shape captured from the hub: a keepalive `data:` line, then the frame.
    let body = "data: \nid: 0\nretry: 3000\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\
                \"result\":{\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n";
    let frame = first_frame(body).expect("a frame");
    let rendered = render(&frame);
    assert_eq!(rendered.text, "hello");
    assert!(rendered.ok);
}

#[test]
fn a_plain_json_body_still_parses() {
    let frame = first_frame(r#"{"jsonrpc":"2.0","id":2,"result":{"content":[]}}"#);
    assert!(frame.is_some());
}

/// A hub error must reach the model as readable text, not as a transport
/// failure it cannot act on.
#[test]
fn an_error_frame_becomes_text() {
    let frame = first_frame(
        r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"Unknown topic 'x'"}}"#,
    )
    .expect("a frame");
    let rendered = render(&frame);
    assert!(!rendered.ok);
    assert!(rendered.text.contains("Unknown topic"), "{}", rendered.text);
}
