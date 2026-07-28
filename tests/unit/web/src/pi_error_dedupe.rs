//! The error pipeline's one-frame guarantee.
//!
//! A failed provider request produces up to two identical error frames per
//! attempt and pi retries a failed prompt as a fresh turn, so one exhausted
//! credit balance used to print the same sentence eight times. `ErrorDeduper`
//! guards the live emit funnel, `collapse_duplicate_errors` mirrors the same
//! rule over rows persisted before the deduper existed, and an agreement test
//! pins the classification needle to the credit guard's actual wording.

use systemprompt_web_admin::test_support::{
    CREDIT_EXHAUSTED_CODE, CREDIT_EXHAUSTED_NEEDLE, ErrorDeduper, PiEventBody,
    collapse_duplicate_errors, upgrade_legacy_error,
};

fn provider_error(message: &str) -> PiEventBody {
    let envelope = serde_json::json!({
        "type": "error",
        "error": { "type": "api_error", "message": message }
    });
    let frame = serde_json::json!({
        "type": "message_end",
        "message": { "role": "assistant", "content": [], "stopReason": "error",
                     "errorMessage": format!("400 {envelope}") }
    });
    systemprompt_web_admin::test_support::translate(&frame)
        .unwrap_or_else(|| panic!("a failed turn must surface an error"))
}

/// The credit-exhausted shape: four retry turns, each carrying the same error
/// twice. Exactly one frame may survive.
#[test]
fn four_retry_turns_collapse_to_one_error() {
    let mut dedupe = ErrorDeduper::default();
    let mut emitted = 0;
    for _ in 0..4 {
        for body in [
            PiEventBody::TurnStart,
            provider_error("Credit exhausted. Add credit to continue."),
            provider_error("Credit exhausted. Add credit to continue."),
            PiEventBody::TurnEnd,
        ] {
            if !dedupe.is_repeat(&body) && matches!(body, PiEventBody::Error { .. }) {
                emitted += 1;
            }
        }
    }
    assert_eq!(emitted, 1);
}

/// A different error is never suppressed, even back-to-back.
#[test]
fn a_new_error_always_shows() {
    let mut dedupe = ErrorDeduper::default();
    assert!(!dedupe.is_repeat(&provider_error("Credit exhausted. Add credit.")));
    assert!(!dedupe.is_repeat(&provider_error("upstream timed out")));
}

/// Any substantive frame clears the memory, so re-prompting into the same
/// failure re-surfaces the error instead of silently swallowing it.
#[test]
fn a_substantive_frame_resets_the_memory() {
    let mut dedupe = ErrorDeduper::default();
    let error = provider_error("Credit exhausted. Add credit.");
    assert!(!dedupe.is_repeat(&error));
    assert!(!dedupe.is_repeat(&PiEventBody::UserMessage {
        text: "hello again".to_owned(),
        via: "prompt",
    }));
    assert!(!dedupe.is_repeat(&error));
}

/// Rows persisted before errors carried `kind`/`code` hold either the raw
/// provider envelope or a `[GOVERNANCE] ` prefix; the upgrade normalises both
/// to the current vocabulary.
#[test]
fn legacy_rows_are_upgraded_on_read() {
    let mut provider = serde_json::json!({
        "type": "error", "seq": 3,
        "message": "400 {\"type\":\"error\",\"error\":{\"message\":\
                    \"Credit exhausted. Add credit to continue.\"}}"
    });
    upgrade_legacy_error(&mut provider);
    assert_eq!(provider["kind"], "provider");
    assert_eq!(provider["code"], CREDIT_EXHAUSTED_CODE);
    assert_eq!(
        provider["message"],
        "Credit exhausted. Add credit to continue."
    );

    let mut governance = serde_json::json!({
        "type": "error", "seq": 4,
        "message": "[GOVERNANCE] unparseable approval request — denied"
    });
    upgrade_legacy_error(&mut governance);
    assert_eq!(governance["kind"], "governance");
    assert_eq!(
        governance["message"],
        "unparseable approval request — denied"
    );

    let mut current = serde_json::json!({
        "type": "error", "seq": 5, "kind": "rpc", "message": "pi rejected the command"
    });
    upgrade_legacy_error(&mut current);
    assert_eq!(current["message"], "pi rejected the command");
}

/// A legacy conversation whose failed prompt wrote the raw envelope and the
/// extracted sentence as two rows, four turns over — history returns one.
#[test]
fn persisted_duplicates_collapse_on_history_read() {
    let envelope = "400 {\"type\":\"error\",\"error\":{\"message\":\
                    \"Credit exhausted. Add credit to continue.\"}}";
    let mut events = vec![serde_json::json!({ "type": "user_message", "text": "hi", "seq": 1 })];
    for turn in 0..4 {
        let base = 2 + turn * 4;
        events.push(serde_json::json!({ "type": "turn_start", "seq": base }));
        events.push(serde_json::json!({ "type": "error", "message": envelope, "seq": base + 1 }));
        events.push(serde_json::json!({
            "type": "error", "seq": base + 2,
            "message": "Credit exhausted. Add credit to continue."
        }));
        events.push(serde_json::json!({ "type": "turn_end", "seq": base + 3 }));
    }
    let collapsed = collapse_duplicate_errors(events);
    let errors: Vec<_> = collapsed.iter().filter(|e| e["type"] == "error").collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0]["message"],
        "Credit exhausted. Add credit to continue."
    );
    // The retry turns were stripped to nothing by the suppression, so only
    // the turn that surfaced the error survives; no bare start/end pairs.
    assert_eq!(
        collapsed
            .iter()
            .filter(|e| e["type"] == "turn_start")
            .count(),
        1
    );
    assert_eq!(
        collapsed.iter().filter(|e| e["type"] == "turn_end").count(),
        1
    );
    assert_eq!(
        collapsed
            .iter()
            .filter(|e| e["type"] == "user_message")
            .count(),
        1
    );
}

/// A turn with real content keeps its start/end frames; only turns stripped
/// empty collapse. A trailing `turn_start` with no `turn_end` — a
/// conversation captured mid-turn — also survives.
#[test]
fn empty_turns_collapse_but_substantive_and_open_turns_survive() {
    let events = vec![
        serde_json::json!({ "type": "turn_start", "seq": 1 }),
        serde_json::json!({ "type": "text_delta", "text": "hi", "seq": 2 }),
        serde_json::json!({ "type": "turn_end", "seq": 3 }),
        serde_json::json!({ "type": "turn_start", "seq": 4 }),
        serde_json::json!({ "type": "turn_end", "seq": 5 }),
        serde_json::json!({ "type": "turn_start", "seq": 6 }),
    ];
    let collapsed = collapse_duplicate_errors(events);
    let kinds: Vec<_> = collapsed.iter().map(|e| e["type"].clone()).collect();
    assert_eq!(kinds, ["turn_start", "text_delta", "turn_end", "turn_start"]);
}

/// The classification needle must keep matching the credit guard's actual
/// sentence. The literal is owned by `extensions/credits/src/guard.rs`; if its
/// wording changes without this needle, credit exhaustion silently degrades to
/// a generic warning line.
#[test]
fn the_needle_matches_the_credit_guards_sentence() {
    let path = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.."))
        .join("extensions/credits/src/guard.rs");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    let sentence = source
        .lines()
        .find(|l| l.contains("Credit exhausted"))
        .unwrap_or_else(|| panic!("the credit guard no longer denies with a credit sentence"));
    assert!(
        sentence.contains(CREDIT_EXHAUSTED_NEEDLE),
        "guard.rs deny text no longer contains {CREDIT_EXHAUSTED_NEEDLE:?}; \
         update events.rs's needle to match"
    );
}
