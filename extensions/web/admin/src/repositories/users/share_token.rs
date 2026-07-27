//! Share-token versioning stored on `user_profile_ext`.
//!
//! Rotating `share_token_version` revokes every previously-issued share token
//! for that user; the public manifest endpoint rechecks the stored version
//! against the value encoded in the token.

use sqlx::PgPool;
use systemprompt::identifiers::UserId;

/// A user with no `user_profile_ext` row resolves to `Ok(None)` — absence of
/// a profile is not an error here.
pub async fn find_share_token_version(
    pool: &PgPool,
    user_id: &UserId,
) -> Result<Option<i32>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT share_token_version FROM user_profile_ext WHERE user_id = $1",
        user_id.as_str(),
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.share_token_version))
}

/// The current version for `user_id`, creating the profile row at version 0
/// when it does not exist yet.
///
/// `user_profile_ext` is written lazily — a user who has never been assigned a
/// department or shared a manifest has no row at all. Reading through
/// [`find_share_token_version`] would then report `None` and be
/// indistinguishable from "no such user", which for the pi embed token would
/// mean a freshly registered account could never be issued one.
///
/// `None` here means no such user, never "no profile yet", so the caller's 404
/// still means what it says. Idempotent: an existing row keeps its version, so
/// this can never silently un-revoke a token.
pub async fn find_or_create_share_token_version(
    pool: &PgPool,
    user_id: &UserId,
) -> Result<Option<i32>, sqlx::Error> {
    if let Some(version) = find_share_token_version(pool, user_id).await? {
        return Ok(Some(version));
    }

    let exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)",
        user_id.as_str(),
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(false);
    if !exists {
        return Ok(None);
    }

    let row = sqlx::query!(
        r"INSERT INTO user_profile_ext (user_id)
          VALUES ($1)
          ON CONFLICT (user_id) DO UPDATE SET user_id = EXCLUDED.user_id
          RETURNING share_token_version",
        user_id.as_str(),
    )
    .fetch_one(pool)
    .await?;
    Ok(Some(row.share_token_version))
}
