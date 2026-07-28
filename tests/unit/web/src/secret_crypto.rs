//! Envelope encryption for stored secrets.
//!
//! Two properties carry the whole scheme and neither is visible from the
//! signatures. The construction is an AEAD, so a ciphertext that has been
//! edited must fail to open rather than decrypt to something plausible — the
//! stored bytes are the only integrity check there is. And the per-user DEK is
//! itself just a payload sealed under the master key, so rotating the master
//! key has to invalidate every DEK sealed under the old one; that is what makes
//! a leaked database useless without it.

use systemprompt_web_admin::repositories::secrets::secret_crypto::{
    SecretCryptoError, decrypt, encrypt, generate_dek, generate_nonce,
};
use systemprompt_web_admin::repositories::secrets::secret_resolve::token_hash;

const KEY: [u8; 32] = [7u8; 32];
const NONCE: [u8; 12] = [3u8; 12];

fn assert_decryption_failed(result: Result<Vec<u8>, SecretCryptoError>) {
    match result {
        Err(SecretCryptoError::DecryptionFailed(_)) => {},
        Err(other) => panic!("expected DecryptionFailed, got {other}"),
        Ok(plaintext) => panic!("expected failure, opened {} bytes", plaintext.len()),
    }
}

#[test]
fn round_trips_a_secret() {
    let sealed = encrypt(&KEY, &NONCE, b"sk-live-abc123").unwrap();
    assert_ne!(sealed.as_slice(), b"sk-live-abc123");
    assert_eq!(decrypt(&KEY, &NONCE, &sealed).unwrap(), b"sk-live-abc123");
}

#[test]
fn round_trips_an_empty_value() {
    let sealed = encrypt(&KEY, &NONCE, b"").unwrap();
    // Empty plaintext still produces the authentication tag, so nothing about
    // the value leaks through the stored length being zero.
    assert_eq!(sealed.len(), 16);
    assert_eq!(decrypt(&KEY, &NONCE, &sealed).unwrap(), b"");
}

#[test]
fn round_trips_multibyte_text() {
    let secret = "パスワード-Ω-🔐";
    let sealed = encrypt(&KEY, &NONCE, secret.as_bytes()).unwrap();
    let opened = decrypt(&KEY, &NONCE, &sealed).unwrap();
    assert_eq!(String::from_utf8(opened).unwrap(), secret);
}

#[test]
fn appends_a_sixteen_byte_tag() {
    let sealed = encrypt(&KEY, &NONCE, b"0123456789").unwrap();
    assert_eq!(sealed.len(), 26);
}

#[test]
fn refuses_a_different_key() {
    let sealed = encrypt(&KEY, &NONCE, b"sk-live-abc123").unwrap();
    assert_decryption_failed(decrypt(&[8u8; 32], &NONCE, &sealed));
}

#[test]
fn refuses_a_different_nonce() {
    let sealed = encrypt(&KEY, &NONCE, b"sk-live-abc123").unwrap();
    assert_decryption_failed(decrypt(&KEY, &[4u8; 12], &sealed));
}

#[test]
fn refuses_an_edited_ciphertext() {
    let sealed = encrypt(&KEY, &NONCE, b"sk-live-abc123").unwrap();
    for index in [0, sealed.len() / 2, sealed.len() - 1] {
        let mut tampered = sealed.clone();
        tampered[index] ^= 0x01;
        assert_decryption_failed(decrypt(&KEY, &NONCE, &tampered));
    }
}

#[test]
fn refuses_a_truncated_ciphertext() {
    let sealed = encrypt(&KEY, &NONCE, b"sk-live-abc123").unwrap();
    assert_decryption_failed(decrypt(&KEY, &NONCE, &sealed[..sealed.len() - 1]));
    assert_decryption_failed(decrypt(&KEY, &NONCE, &[]));
}

#[test]
fn a_reused_nonce_leaks_that_two_secrets_match() {
    // Not a bug to fix here, but the reason every stored value carries its own
    // nonce: under one nonce, equal plaintexts are visibly equal.
    let same = encrypt(&KEY, &NONCE, b"shared").unwrap();
    let again = encrypt(&KEY, &NONCE, b"shared").unwrap();
    assert_eq!(same, again);

    let fresh = encrypt(&KEY, &generate_nonce(), b"shared").unwrap();
    assert_ne!(fresh, same);
}

#[test]
fn generated_key_material_is_not_constant() {
    let (first, second) = (generate_dek(), generate_dek());
    assert_ne!(first, second);
    assert_ne!(first, [0u8; 32]);
    assert_ne!(generate_nonce(), generate_nonce());
}

#[test]
fn a_dek_sealed_under_one_master_key_does_not_open_under_another() {
    let dek = generate_dek();
    let dek_nonce = generate_nonce();
    let master = [1u8; 32];
    let sealed_dek = encrypt(&master, &dek_nonce, &dek).unwrap();

    assert_eq!(decrypt(&master, &dek_nonce, &sealed_dek).unwrap(), dek);
    assert_decryption_failed(decrypt(&[2u8; 32], &dek_nonce, &sealed_dek));
}

#[test]
fn a_secret_sealed_under_one_dek_does_not_open_under_a_rotated_one() {
    // Rotation re-seals every value; a row missed by that pass is unreadable
    // rather than silently wrong.
    let old_dek = generate_dek();
    let value_nonce = generate_nonce();
    let sealed = encrypt(&old_dek, &value_nonce, b"sk-live-abc123").unwrap();

    let new_dek = generate_dek();
    assert_decryption_failed(decrypt(&new_dek, &value_nonce, &sealed));

    let resealed = encrypt(&new_dek, &value_nonce, b"sk-live-abc123").unwrap();
    assert_eq!(
        decrypt(&new_dek, &value_nonce, &resealed).unwrap(),
        b"sk-live-abc123"
    );
}

#[test]
fn a_resolution_token_is_stored_only_as_its_digest() {
    let raw = "b3f1c2d4-0000-4000-8000-000000000001";
    let hash = token_hash(raw);
    assert_eq!(hash.len(), 64);
    assert!(!hash.contains(raw));
    assert_eq!(hash, token_hash(raw));
    assert_ne!(hash, token_hash("b3f1c2d4-0000-4000-8000-000000000002"));
}

#[test]
fn the_resolution_token_digest_is_sha256() {
    // Pinned against a known vector so the stored digests of existing tokens
    // cannot be invalidated by swapping the hash out from under them.
    assert_eq!(
        token_hash("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
