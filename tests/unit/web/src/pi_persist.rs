//! `Journal` — what a live conversation leaves behind in Postgres.
//!
//! Three properties are pinned. Streaming prose is coalesced, because a row per
//! token is the wrong granularity and would make a restored transcript
//! unreadable. Ordering is preserved across the coalescing, so a tool call
//! never appears above the sentence that introduced it. And the frames that are
//! *not* conversation — stderr from the child, the session's own start and exit
//! — stay out of the record entirely.

use systemprompt::identifiers::ContextId;
use systemprompt_web_admin::test_support::{Journal, NewPiEvent, PiEvent, PiEventBody};

/// Drive a whole conversation through the journal, as the writer task does.
fn journal(bodies: Vec<PiEventBody>) -> Vec<NewPiEvent> {
    let mut journal = Journal::default();
    let mut out = Vec::new();
    for (i, body) in bodies.into_iter().enumerate() {
        journal.absorb(&PiEvent::new(i as u64 + 1, body), &mut out);
    }
    journal.settle(&mut out);
    out
}

fn text(s: &str) -> PiEventBody {
    PiEventBody::TextDelta {
        text: s.to_owned(),
    }
}

#[test]
fn a_run_of_deltas_becomes_one_row() {
    let rows = journal(vec![text("Hel"), text("lo, "), text("world")]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "text_delta");
    assert_eq!(rows[0].body["text"], "Hello, world");
}

/// The run carries the `seq` of its *first* delta, so a viewer resuming at a
/// stored watermark receives every frame exactly once and in order.
#[test]
fn a_coalesced_run_keeps_the_seq_it_started_at() {
    let rows = journal(vec![PiEventBody::TurnStart, text("a"), text("b")]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].kind, "turn_start");
    assert_eq!(rows[0].seq, 1);
    assert_eq!(rows[1].seq, 2);
}

#[test]
fn prose_lands_before_the_tool_call_that_follows_it() {
    let rows = journal(vec![
        text("Let me read that file."),
        PiEventBody::ToolStart {
            tool_use_id: Some("t1".to_owned()),
            tool_name: "read".to_owned(),
            tool_input: serde_json::json!({ "path": "README.md" }),
        },
    ]);
    let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(kinds, ["text_delta", "tool_start"]);
}

/// Thinking and prose are different frames even when they arrive back to back,
/// so a restored transcript can keep the reasoning collapsed.
#[test]
fn thinking_does_not_merge_into_prose() {
    let rows = journal(vec![
        PiEventBody::ThinkingDelta {
            text: "hmm".to_owned(),
        },
        text("Here is the answer."),
    ]);
    let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(kinds, ["thinking_delta", "text_delta"]);
}

/// `Stderr` is an operator signal about the child process and the frame most
/// likely to carry something from the host; `SessionReady` and `Exit` each
/// describe a process starting or ending *now*, so replaying them into a
/// restored transcript would tell the viewer their live session had exited.
#[test]
fn frames_that_are_not_conversation_are_not_stored() {
    let rows = journal(vec![
        PiEventBody::SessionReady {
            conversation_id: ContextId::generate(),
        },
        PiEventBody::Stderr {
            line: "sp-pi-jail: Landlock v5".to_owned(),
        },
        PiEventBody::Exit { code: Some(0) },
    ]);
    assert!(rows.is_empty(), "stored {rows:?}");
}

/// The viewer's own half of the conversation. pi never echoes a prompt back, so
/// without this frame a restored transcript would be the agent talking to
/// nobody.
#[test]
fn the_viewers_message_is_stored() {
    let rows = journal(vec![PiEventBody::UserMessage {
        text: "what is in the workspace?".to_owned(),
        via: "prompt",
    }]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "user_message");
    assert_eq!(rows[0].body["text"], "what is in the workspace?");
}

/// The governance chain is part of the record, not an implementation detail:
/// the stored transcript has to be able to explain a call that was refused.
#[test]
fn a_blocked_call_is_stored_with_its_policy() {
    let rows = journal(vec![PiEventBody::ToolBlocked {
        tool_use_id: Some("t1".to_owned()),
        tool_name: "read".to_owned(),
        reason: "/etc/passwd is outside the workspace".to_owned(),
        policy: Some("workspace_scope".to_owned()),
    }]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "tool_blocked");
    assert_eq!(rows[0].body["policy"], "workspace_scope");
}

/// A conversation that ends mid-sentence still stores the sentence — the tail
/// is the part a returning viewer most wants back.
#[test]
fn an_unterminated_run_is_settled_at_the_end() {
    let rows = journal(vec![text("half a thought")]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].body["text"], "half a thought");
}
