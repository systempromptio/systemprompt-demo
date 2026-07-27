//! The credential an embedded widget carries.
//!
//! Why not the session cookie: the widget is meant to be dropped into a page on
//! another origin, where a `SameSite` cookie will not be sent — and the site
//! auth gate answers an unauthenticated hit on a protected prefix with a 302 to
//! the login page, which an `EventSource` reports as an opaque error rather
//! than a 401. Why not an API key: `EventSource` cannot set headers, so the
//! credential has to survive in a URL.
//!
//! So: the share-manifest token's construction (HMAC-SHA256 keyed off the JWT
//! signing secret, revoked by bumping `users.share_token_version`) with an
//! expiry added. The expiry is the difference that matters — a share token
//! reveals a catalog, this one starts a process.

use base64::Engine;
use systemprompt::identifiers::UserId;

use crate::util::hmac;

/// Ties a signature to this purpose, so a share-manifest token cannot be
/// replayed as a terminal token or the reverse.
const PURPOSE: &str = "pi-embed";

/// Token lifetime. Short because it is a bearer credential living in a URL, and
/// a widget only needs it long enough to open a stream.
pub(super) const TTL_SECS: i64 = 3_600;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// `b64(user_id):b64(version):b64(exp):hex(mac)`
#[must_use]
pub(super) fn sign(secret: &[u8], user_id: &UserId, version: i32, exp: i64) -> String {
    let mac = hmac::hex(secret, &payload(user_id, version, exp));
    format!(
        "{}:{}:{}:{mac}",
        B64.encode(user_id.as_str().as_bytes()),
        B64.encode(version.to_string().as_bytes()),
        B64.encode(exp.to_string().as_bytes()),
    )
}

fn payload(user_id: &UserId, version: i32, exp: i64) -> String {
    format!("{PURPOSE}:{user_id}:{version}:{exp}")
}

/// Why a token was refused. The caller maps all of these to one opaque 401 —
/// the distinction is for logs, not for the client.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Invalid {
    Malformed,
    BadSignature,
    Expired,
}

/// Recover `(user_id, version)` from a token, or say why not.
///
/// The version still has to be rechecked against the database by the caller;
/// this only proves the token is intact and current.
pub(super) fn verify(secret: &[u8], token: &str, now: i64) -> Result<(UserId, i32), Invalid> {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 4 {
        return Err(Invalid::Malformed);
    }
    let decode = |s: &str| -> Option<String> { String::from_utf8(B64.decode(s).ok()?).ok() };
    let user_id = UserId::new(decode(parts[0]).ok_or(Invalid::Malformed)?);
    let version: i32 = decode(parts[1])
        .ok_or(Invalid::Malformed)?
        .parse()
        .map_err(|_unparseable| Invalid::Malformed)?;
    let exp: i64 = decode(parts[2])
        .ok_or(Invalid::Malformed)?
        .parse()
        .map_err(|_unparseable| Invalid::Malformed)?;

    // Signature before expiry, so a tampered `exp` cannot be probed by watching
    // which error comes back.
    if !hmac::eq(
        &hmac::hex(secret, &payload(&user_id, version, exp)),
        parts[3],
    ) {
        return Err(Invalid::BadSignature);
    }
    if exp <= now {
        return Err(Invalid::Expired);
    }
    Ok((user_id, version))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "assertions in tests")]
mod tests {
    use super::*;

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
}
