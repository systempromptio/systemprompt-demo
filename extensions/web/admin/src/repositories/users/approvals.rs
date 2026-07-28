//! The approval row every account carries, and the decisions written to it.
//!
//! Split from [`super::registration`]: registration owns getting an account
//! created (tokens, profile, attempt throttling), this module owns whether
//! that account may sign in.

use sqlx::{PgExecutor, PgPool};
use systemprompt::identifiers::UserId;

/// Approval states a `user_approvals` row can hold. A missing row reads as
/// pending everywhere, so an account created down a path that forgets to write
/// one fails closed.
pub const APPROVAL_PENDING: &str = "pending";
pub const APPROVAL_APPROVED: &str = "approved";
pub const APPROVAL_DENIED: &str = "denied";

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

/// Approve an account inside the registration transaction.
///
/// Signups are currently auto-approved: the review machinery (gate middleware,
/// pending page, admin approve endpoint) stays wired but never triggers,
/// because every account is written approved from the start. `decided_by`
/// records that no human made the call.
pub async fn approve_on_signup<'e>(
    executor: impl PgExecutor<'e>,
    user_id: &UserId,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO user_approvals (user_id, status, decided_at, decided_by)
         VALUES ($1, 'approved', NOW(), 'system:auto-approve')
         ON CONFLICT (user_id) DO UPDATE SET
            status = 'approved',
            decided_at = NOW(),
            decided_by = EXCLUDED.decided_by",
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

    // Why: every approved account must hold the signup credit, whichever path
    // created it; the grant is idempotent, so re-approval cannot double-pay.
    if let Err(e) =
        systemprompt_credits_extension::grant_signup_credit(pool, user_id.as_str()).await
    {
        tracing::warn!(error = %e, user_id = %user_id, "signup credit grant failed");
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
