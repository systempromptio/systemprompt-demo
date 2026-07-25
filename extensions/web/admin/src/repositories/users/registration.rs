//! Self-registration: setup tokens, the company profile, and the manual
//! approval decision that gates every new account.
//!
//! The write side takes `impl PgExecutor` rather than `&PgPool` so
//! `public_register_handler` can run account creation, profile, approval row
//! and setup token inside one transaction. Callers outside a transaction pass
//! `&*pool` unchanged.

use sqlx::{PgExecutor, PgPool};
use systemprompt::identifiers::UserId;

/// Approval states a `user_approvals` row can hold. A missing row reads as
/// pending everywhere, so an account created down a path that forgets to write
/// one fails closed.
pub const APPROVAL_PENDING: &str = "pending";
pub const APPROVAL_APPROVED: &str = "approved";
pub const APPROVAL_DENIED: &str = "denied";

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

/// What registration needs to know about an email before it writes anything.
#[derive(Debug)]
pub struct RegistrationState {
    pub user_id: UserId,
    /// `None` when no `user_approvals` row exists, which reads as pending.
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

/// Record the user's full name, and their preferred display name when given.
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

/// The applicant's own details, as they submitted them, for the admin review
/// queue and the notification email.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PendingApplicant {
    pub user_id: UserId,
    pub display_name: String,
    pub email: String,
    pub company: String,
    pub role: String,
    pub team_size: String,
    pub why_assessing: String,
    pub credit_plans: Option<String>,
    pub requested_at: String,
}

pub async fn list_pending_applicants(pool: &PgPool) -> Vec<PendingApplicant> {
    sqlx::query!(
        r#"
        SELECT
            u.id AS "user_id!",
            COALESCE(u.full_name, u.display_name, u.name) AS "display_name!",
            u.email AS "email!",
            p.company AS "company!",
            p.role AS "role!",
            p.team_size AS "team_size!",
            p.why_assessing AS "why_assessing!",
            p.credit_plans,
            to_char(a.requested_at, 'YYYY-MM-DD HH24:MI') AS "requested_at!"
        FROM user_approvals a
        JOIN users u ON u.id = a.user_id
        JOIN user_onboarding_profiles p ON p.user_id = a.user_id
        WHERE a.status = 'pending'
        ORDER BY a.requested_at ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .inspect_err(|e| tracing::warn!(error = %e, "list_pending_applicants failed"))
    .unwrap_or_default()
    .into_iter()
    .map(|r| PendingApplicant {
        user_id: UserId::new(r.user_id),
        display_name: r.display_name,
        email: r.email,
        company: r.company,
        role: r.role,
        team_size: r.team_size,
        why_assessing: r.why_assessing,
        credit_plans: r.credit_plans,
        requested_at: r.requested_at,
    })
    .collect()
}

pub async fn find_applicant(pool: &PgPool, user_id: &UserId) -> Option<PendingApplicant> {
    sqlx::query!(
        r#"
        SELECT
            u.id AS "user_id!",
            COALESCE(u.full_name, u.display_name, u.name) AS "display_name!",
            u.email AS "email!",
            p.company AS "company!",
            p.role AS "role!",
            p.team_size AS "team_size!",
            p.why_assessing AS "why_assessing!",
            p.credit_plans,
            to_char(a.requested_at, 'YYYY-MM-DD HH24:MI') AS "requested_at!"
        FROM user_approvals a
        JOIN users u ON u.id = a.user_id
        JOIN user_onboarding_profiles p ON p.user_id = a.user_id
        WHERE a.user_id = $1
        "#,
        user_id.as_str(),
    )
    .fetch_optional(pool)
    .await
    .inspect_err(|e| tracing::warn!(error = %e, user_id = %user_id, "find_applicant failed"))
    .ok()
    .flatten()
    .map(|r| PendingApplicant {
        user_id: UserId::new(r.user_id),
        display_name: r.display_name,
        email: r.email,
        company: r.company,
        role: r.role,
        team_size: r.team_size,
        why_assessing: r.why_assessing,
        credit_plans: r.credit_plans,
        requested_at: r.requested_at,
    })
}

/// Open a review for this account.
///
/// `DO NOTHING` keeps the original
/// `requested_at`, so a user who abandons the passkey step and retries does not
/// jump the review queue — and a denied account cannot reset itself to pending.
pub async fn insert_pending_approval<'e>(
    executor: impl PgExecutor<'e>,
    user_id: &UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO user_approvals (user_id, status)
         VALUES ($1, 'pending')
         ON CONFLICT (user_id) DO NOTHING",
        user_id.as_str(),
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Mark an account approved at creation time.
///
/// For accounts an admin or the demo flow creates directly: the decision the
/// review gate exists to capture has already been made, so they must not land
/// in the queue. Best-effort — a failure here leaves the account pending, which
/// an admin can still clear by hand, and must not fail the creation itself.
pub async fn approve_on_create(pool: &PgPool, user_id: &UserId, decided_by: &UserId) {
    let result = sqlx::query!(
        "INSERT INTO user_approvals (user_id, status, decided_at, decided_by)
         VALUES ($1, 'approved', NOW(), $2)
         ON CONFLICT (user_id) DO UPDATE SET
            status = 'approved',
            decided_at = NOW(),
            decided_by = EXCLUDED.decided_by",
        user_id.as_str(),
        decided_by.as_str(),
    )
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::warn!(error = %e, user_id = %user_id, "approve_on_create failed");
    }
}

/// Record an admin's decision. Returns `false` when the account was already in
/// the target state, which is what makes double-clicking Approve harmless.
pub async fn set_approval_status(
    pool: &PgPool,
    user_id: &UserId,
    status: &str,
    decided_by: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE user_approvals
         SET status = $2, decided_at = NOW(), decided_by = $3
         WHERE user_id = $1 AND status IS DISTINCT FROM $2",
        user_id.as_str(),
        status,
        decided_by,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
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
