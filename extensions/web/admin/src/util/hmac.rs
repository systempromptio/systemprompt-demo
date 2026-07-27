//! HMAC-SHA256 over an arbitrary payload, keyed off a bootstrap secret.
//!
//! RFC 2104 by hand because the crate graph carries `sha2` but no `hmac`, and
//! one primitive in one place beats a second copy per token scheme. Two callers
//! rely on it: the share-manifest token and the pi embed token.

use sha2::{Digest, Sha256};

const BLOCK: usize = 64;

#[must_use]
pub fn hex(secret: &[u8], payload: &str) -> String {
    let mut key = [0u8; BLOCK];
    if secret.len() > BLOCK {
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
