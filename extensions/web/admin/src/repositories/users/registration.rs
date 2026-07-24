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

/// The user's saved company profile marker, if the onboarding form was ever
/// submitted. Flows that require a complete profile (the bridge device-link)
/// gate on row presence.
pub async fn find_onboarding_profile(pool: &PgPool, user_id: &UserId) -> Option<String> {
    sqlx::query_scalar!(
        "SELECT company FROM user_onboarding_profiles WHERE user_id = $1",
        user_id.as_str(),
    )
    .fetch_optional(pool)
    .await
    .inspect_err(
        |e| tracing::warn!(error = %e, user_id = %user_id, "find_onboarding_profile failed"),
    )
    .ok()
    .flatten()
}

#[derive(Debug)]
pub struct NewOnboardingProfile<'a> {
    pub company: &'a str,
    pub role: &'a str,
    pub team_size: &'a str,
    pub why_assessing: &'a str,
    pub credit_plans: Option<&'a str>,
}

pub async fn insert_onboarding_profile(
    pool: &PgPool,
    user_id: &UserId,
    profile: &NewOnboardingProfile<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO user_onboarding_profiles
            (user_id, company, role, team_size, why_assessing, credit_plans)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (user_id) DO UPDATE SET
            company = EXCLUDED.company,
            role = EXCLUDED.role,
            team_size = EXCLUDED.team_size,
            why_assessing = EXCLUDED.why_assessing,
            credit_plans = EXCLUDED.credit_plans",
        user_id.as_str(),
        profile.company,
        profile.role,
        profile.team_size,
        profile.why_assessing,
        profile.credit_plans,
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
