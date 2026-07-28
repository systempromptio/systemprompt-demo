//! The two seams of the reviewed-signup funnel.
//!
//! [`registration_submitted`] fires when someone registers and notifies the
//! admins. [`account_approved`] is the only path that grants the $1
//! signup credit or sends the welcome email. Signups are auto-approved, so
//! registration calls it directly; the admin approve endpoint still calls it
//! too, harmlessly, because the grant is idempotent.
//!
//! Both push their email out on a detached task so a slow or unconfigured SMTP
//! relay can never fail the HTTP request that triggered it.

use sqlx::PgPool;
use systemprompt::identifiers::UserId;

// Why: owned struct so the hook signature stays stable as downstream
// integrations (credits, email, CRM) grow what they need.
#[derive(Debug, Clone)]
pub(crate) struct OnboardingProfile {
    pub company: String,
    pub role: String,
    pub team_size: String,
    pub why_assessing: String,
    pub credit_plans: Option<String>,
}

pub(crate) fn registration_submitted(
    user_id: &UserId,
    email: &str,
    name: &str,
    profile: &OnboardingProfile,
) {
    tracing::info!(
        user_id = %user_id,
        email,
        name,
        company = %profile.company,
        role = %profile.role,
        team_size = %profile.team_size,
        "registration submitted; auto-approved"
    );

    let (email, name, profile) = (email.to_owned(), name.to_owned(), profile.clone());
    tokio::spawn(async move {
        let site_url = systemprompt_email::configured_site_url();
        let notice = systemprompt_email::RegistrationNotice {
            name: &name,
            email: &email,
            company: &profile.company,
            role: &profile.role,
            team_size: &profile.team_size,
            why_assessing: &profile.why_assessing,
            credit_plans: profile.credit_plans.as_deref(),
        };
        if let Err(e) = systemprompt_email::send_registration_notice(&notice, &site_url).await {
            tracing::warn!(email = %email, error = %e, "registration notice not sent");
        }
    });
}

// Why: grants the $1 signup credit idempotently, then fires the welcome email
// in a detached task. Failures are logged and swallowed so the caller's HTTP
// request never fails on a credit or email problem.
pub(crate) async fn account_approved(pool: &PgPool, user_id: &UserId, email: &str, name: &str) {
    tracing::info!(user_id = %user_id, email, name, "account approved");

    match systemprompt_credits::grant_signup_credit(pool, user_id.as_str()).await {
        Ok(newly_granted) => {
            if !newly_granted {
                tracing::info!(user_id = %user_id, "signup credit already granted; skipping");
                return;
            }
        },
        Err(e) => {
            tracing::error!(user_id = %user_id, error = %e, "failed to grant signup credit");
            return;
        },
    }

    let to = email.to_owned();
    let recipient_name = name.to_owned();
    tokio::spawn(async move {
        let site_url = systemprompt_email::configured_site_url();
        if let Err(e) =
            systemprompt_email::send_welcome_email(&to, &recipient_name, &site_url).await
        {
            tracing::warn!(email = %to, error = %e, "welcome email not sent");
        }
    });
}
