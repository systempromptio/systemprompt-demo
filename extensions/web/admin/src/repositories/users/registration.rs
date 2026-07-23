//! Self-registration setup tokens and their rate-limit counters.

use sqlx::PgPool;
use systemprompt::identifiers::UserId;

pub async fn count_recent_setup_tokens(pool: &PgPool, email: &str) -> i64 {
    sqlx::query_scalar!(
        "SELECT COUNT(*) FROM webauthn_setup_tokens
         WHERE user_id IN (SELECT id FROM users WHERE email = $1)
         AND created_at > NOW() - INTERVAL '15 minutes'",
        email,
    )
    .fetch_one(pool)
    .await
    .inspect_err(|e| tracing::warn!(error = %e, email, "count_recent_setup_tokens failed"))
    .ok()
    .flatten()
    .unwrap_or(0)
}

/// Whether the user has completed the onboarding form.
///
/// Onboarding is tracked column-free: self-registration sets only
/// `display_name`, never `full_name`. The onboarding form is the single place
/// that writes `full_name`, so a non-empty `full_name` is the durable marker
/// that a user finished onboarding.
pub async fn is_onboarded(pool: &PgPool, user_id: &UserId) -> bool {
    sqlx::query_scalar!(
        "SELECT (full_name IS NOT NULL AND full_name <> '') AS \"onboarded!\"
         FROM users WHERE id = $1",
        user_id.as_str(),
    )
    .fetch_optional(pool)
    .await
    .inspect_err(|e| tracing::warn!(error = %e, user_id = %user_id, "is_onboarded failed"))
    .ok()
    .flatten()
    .unwrap_or(false)
}

/// Record onboarding completion by persisting the user's full name (the
/// onboarding marker) and, when provided, their preferred display name.
pub async fn mark_onboarded(
    pool: &PgPool,
    user_id: &UserId,
    full_name: &str,
    display_name: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE users
         SET full_name = $2,
             display_name = COALESCE($3, display_name),
             updated_at = NOW()
         WHERE id = $1",
        user_id.as_str(),
        full_name,
        display_name,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_setup_token(
    pool: &PgPool,
    token_id: &str,
    user_id: &UserId,
    token_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO webauthn_setup_tokens (id, user_id, token_hash, purpose, expires_at)
         VALUES ($1, $2, $3, 'credential_link', NOW() + INTERVAL '15 minutes')",
        token_id,
        user_id.as_str(),
        token_hash,
    )
    .execute(pool)
    .await?;
    Ok(())
}
