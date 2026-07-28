//! The call ledger: which evaluations of a call share its identity.

use serde_json::json;
use systemprompt_web_admin::test_support::CallLedger;

#[test]
fn claim_inherits_the_identity_the_gate_minted() {
    let ledger = CallLedger::default();
    let args = json!({"artifact_type": "dashboard"});

    let minted = ledger.mint("render_artifact", Some(&args));
    let (claimed, _) = ledger.claim("render_artifact", Some(&args));

    assert_eq!(minted, claimed);
}

#[test]
fn an_unclaimed_description_mints_a_fresh_identity() {
    let ledger = CallLedger::default();
    let args = json!({"artifact_type": "dashboard"});

    let minted = ledger.mint("render_artifact", Some(&args));
    let (claimed, _) = ledger.claim("fetch_remote_docs", Some(&args));

    assert_ne!(minted, claimed);
}

#[test]
fn different_arguments_do_not_share_an_identity() {
    let ledger = CallLedger::default();
    let minted = ledger.mint("render_artifact", Some(&json!({"artifact_type": "table"})));
    let (claimed, _) = ledger.claim(
        "render_artifact",
        Some(&json!({"artifact_type": "dashboard"})),
    );

    assert_ne!(minted, claimed);
}

// Why: the guarantee that a replayed call cannot ride one entry forever — the
// second claim has to pay for itself.
#[test]
fn an_entry_can_only_be_claimed_once() {
    let ledger = CallLedger::default();
    let args = json!({"artifact_type": "chart"});

    let minted = ledger.mint("render_artifact", Some(&args));
    let (first, _) = ledger.claim("render_artifact", Some(&args));
    let (second, _) = ledger.claim("render_artifact", Some(&args));

    assert_eq!(minted, first);
    assert_ne!(first, second);
}

#[test]
fn two_calls_of_the_same_shape_claim_their_own_entries() {
    let ledger = CallLedger::default();
    let args = json!({"artifact_type": "list"});

    let first_minted = ledger.mint("render_artifact", Some(&args));
    let second_minted = ledger.mint("render_artifact", Some(&args));
    let (first, _) = ledger.claim("render_artifact", Some(&args));
    let (second, _) = ledger.claim("render_artifact", Some(&args));

    assert_ne!(first_minted, second_minted);
    assert_eq!(first_minted, first);
    assert_eq!(second_minted, second);
}
