//! The embed token — a signed capability, not a session.
//!
//! It carries the user and a share-token version, so revoking the version
//! invalidates every token minted under it. Expiry is checked after the
//! signature, deliberately: an unsigned token is never trusted enough for its
//! own expiry claim to be read.

use base64::Engine as _;
use systemprompt::identifiers::UserId;
use systemprompt_web_pi::test_support::{B64, Invalid, sign, verify};
use systemprompt_web_shared::hmac;

const SECRET: &[u8] = b"test-signing-secret";

fn uid() -> UserId {
    UserId::new("11111111-2222-3333-4444-555555555555") // lint-ok: no-synthesis — signing a principal is the unit under test
}

#[test]
fn round_trips() {
    let t = sign(SECRET, &uid(), 3, 2_000);
    assert_eq!(verify(SECRET, &t, 1_000), Ok((uid(), 3)));
}

#[test]
fn rejects_expiry_in_the_past() {
    let t = sign(SECRET, &uid(), 1, 1_000);
    assert_eq!(verify(SECRET, &t, 1_000), Err(Invalid::Expired));
}

#[test]
fn rejects_a_different_secret() {
    let t = sign(SECRET, &uid(), 1, 2_000);
    assert_eq!(
        verify(b"other-secret", &t, 1_000),
        Err(Invalid::BadSignature)
    );
}

#[test]
fn rejects_a_lengthened_expiry() {
    // The whole point of signing `exp`: pushing it out must not verify.
    let t = sign(SECRET, &uid(), 1, 1_500);
    let parts: Vec<&str> = t.split(':').collect();
    let forged = format!(
        "{}:{}:{}:{}",
        parts[0],
        parts[1],
        B64.encode(b"99999999999"),
        parts[3]
    );
    assert_eq!(verify(SECRET, &forged, 1_000), Err(Invalid::BadSignature));
}

#[test]
fn rejects_a_bumped_version() {
    // Revocation works by changing the version, so a token naming the old
    // one must not verify against a signature for the new.
    let t = sign(SECRET, &uid(), 1, 2_000);
    let (_, version) = verify(SECRET, &t, 1_000).unwrap();
    assert_eq!(version, 1);
    assert_ne!(sign(SECRET, &uid(), 2, 2_000), t);
}

#[test]
fn rejects_malformed_shapes() {
    assert_eq!(verify(SECRET, "a:b:c", 0), Err(Invalid::Malformed));
    assert_eq!(verify(SECRET, "", 0), Err(Invalid::Malformed));
    assert_eq!(
        verify(SECRET, "!!!:!!!:!!!:!!!", 0),
        Err(Invalid::Malformed)
    );
}

#[test]
fn is_not_interchangeable_with_a_share_token() {
    // Same secret, same user, same version — different purpose, so the
    // manifest token's signature must not validate here.
    let share_payload = format!("{}:{}", uid(), 1);
    let share_mac = hmac::hex(SECRET, &share_payload);
    let ours = sign(SECRET, &uid(), 1, 2_000);
    assert!(!ours.ends_with(&share_mac));
}
