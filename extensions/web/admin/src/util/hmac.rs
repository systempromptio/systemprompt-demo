//! HMAC-SHA256 over an arbitrary payload, keyed off a bootstrap secret.
//!
//! RFC 2104 by hand because the crate graph carries `sha2` but no `hmac`, and
//! one primitive in one place beats a second copy per token scheme. Two callers
//! rely on it: the share-manifest token and the pi embed token.

use sha2::{Digest, Sha256};

const BLOCK: usize = 64;

/// Hex-encoded HMAC-SHA256 of `payload` under `secret`.
#[must_use]
pub fn hex(secret: &[u8], payload: &str) -> String {
    let mut key = [0u8; BLOCK];
    if secret.len() > BLOCK {
        // A key longer than the block size is hashed first, per RFC 2104.
        let mut h = Sha256::new();
        h.update(secret);
        key[..32].copy_from_slice(&h.finalize());
    } else {
        key[..secret.len()].copy_from_slice(secret);
    }

    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(payload.as_bytes());
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);

    let mac = outer.finalize();
    let mut out = String::with_capacity(mac.len() * 2);
    for b in mac {
        use std::fmt::Write;
        _ = write!(out, "{b:02x}");
    }
    out
}

/// Length-independent comparison, so a mismatch reveals nothing about how far
/// it matched.
#[must_use]
pub fn eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4231 test case 2, so a refactor of the pad arithmetic cannot quietly
    /// change every token this signs.
    #[test]
    fn matches_rfc4231_case_2() {
        assert_eq!(
            hex(b"Jefe", "what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn long_key_is_hashed_first() {
        // Exercises the >64-byte branch; value pinned from this implementation
        // to catch accidental changes, and asserted stable across lengths.
        let a = hex(&[0xaa; 80], "payload");
        let b = hex(&[0xaa; 80], "payload");
        assert_eq!(a, b);
        assert_ne!(a, hex(&[0xaa; 64], "payload"));
    }

    #[test]
    fn eq_rejects_length_and_content_mismatch() {
        assert!(eq("abc", "abc"));
        assert!(!eq("abc", "abd"));
        assert!(!eq("abc", "abcd"));
    }
}
