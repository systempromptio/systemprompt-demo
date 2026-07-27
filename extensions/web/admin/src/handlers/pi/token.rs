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

const PURPOSE: &str = "pi-embed";

pub(super) const TTL_SECS: i64 = 3_600;

pub const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

#[must_use]
pub fn sign(secret: &[u8], user_id: &UserId, version: i32, exp: i64) -> String {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    Malformed,
    BadSignature,
    Expired,
}

/// Recover `(user_id, version)` from a token, or say why not.
///
/// The version still has to be rechecked against the database by the caller;
/// this only proves the token is intact and current.
pub fn verify(secret: &[u8], token: &str, now: i64) -> Result<(UserId, i32), Invalid> {
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
