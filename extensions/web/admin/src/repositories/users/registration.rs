//! Self-registration: setup tokens, the company profile, and the approval row
//! every new account is written with.
//!
//! The write side takes `impl PgExecutor` rather than `&PgPool` so
//! `public_register_handler` can run account creation, profile, approval row
//! and setup token inside one transaction. Callers outside a transaction pass
//! `&*pool` unchanged.

use std::net::IpAddr;

use sqlx::{PgExecutor, PgPool};
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

pub async fn count_recent_registration_attempts(pool: &PgPool, ip: IpAddr) -> i64 {
    sqlx::query_scalar!(
        "SELECT COUNT(*) FROM registration_attempts
         WHERE ip_address = $1
         AND created_at > NOW() - INTERVAL '24 hours'",
        ip.to_string(),
    )
    .fetch_one(pool)
    .await
    .inspect_err(|e| tracing::warn!(error = %e, "count_recent_registration_attempts failed"))
    .ok()
    .flatten()
    .unwrap_or(0)
}

pub async fn insert_registration_attempt(pool: &PgPool, ip: IpAddr, email: &str) {
    let ip_text = ip.to_string();
    if let Err(e) = sqlx::query!(
        "INSERT INTO registration_attempts (ip_address, email) VALUES ($1, $2)",
        ip_text,
        email,
    )
    .execute(pool)
    .await
    {
        tracing::warn!(error = %e, email, "insert_registration_attempt failed");
        return;
    }

    // Why: no scheduled sweep owns this table, so each write prunes its own
    // address. Scoped to one IP it touches a handful of rows and rides the
    // index, and the window is wider than the limit so a saturated address
    // stays legible for a few days after it stops.
    if let Err(e) = sqlx::query!(
        "DELETE FROM registration_attempts
         WHERE ip_address = $1 AND created_at < NOW() - INTERVAL '7 days'",
        ip_text,
    )
    .execute(pool)
    .await
    {
        tracing::warn!(error = %e, "registration_attempts prune failed");
    }
}

#[derive(Debug)]
pub struct RegistrationState {
    pub user_id: UserId,
    pub approval_status: Option<String>,
    /// Whether a passkey is already bound to this account. This is the
    /// takeover guard: an account someone can already sign into must never be
    /// handed a fresh credential-link token by an unauthenticated caller.
    pub has_credential: bool,
}

pub async fn find_registration_state(pool: &PgPool, email: &str) -> Option<RegistrationState> {
    sqlx::query!(
        r#"
        SELECT
            u.id AS "user_id!",
            a.status,
            EXISTS(SELECT 1 FROM webauthn_credentials c WHERE c.user_id = u.id)
                AS "has_credential!"
        FROM users u
        LEFT JOIN user_approvals a ON a.user_id = u.id
        WHERE u.email = $1
        "#,
        email,
    )
    .fetch_optional(pool)
    .await
    .inspect_err(|e| tracing::warn!(error = %e, email, "find_registration_state failed"))
    .ok()
    .flatten()
    .map(|r| RegistrationState {
        user_id: UserId::new(r.user_id),
        approval_status: r.status,
        has_credential: r.has_credential,
    })
}

pub async fn mark_onboarded<'e>(
    executor: impl PgExecutor<'e>,
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
    .execute(executor)
    .await?;
    Ok(())
}

#[derive(Debug)]
pub struct NewOnboardingProfile<'a> {
    pub company: &'a str,
    pub role: &'a str,
    pub team_size: &'a str,
    pub why_assessing: &'a str,
    pub credit_plans: Option<&'a str>,
}

pub async fn insert_onboarding_profile<'e>(
    executor: impl PgExecutor<'e>,
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
    .execute(executor)
    .await?;
    Ok(())
}

pub async fn insert_setup_token<'e>(
    executor: impl PgExecutor<'e>,
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
    .execute(executor)
    .await?;
    Ok(())
}
