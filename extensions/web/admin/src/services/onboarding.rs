//! The two seams of the reviewed-signup funnel.
//!
//! [`registration_submitted`] fires when someone registers and puts the request
//! in front of a reviewer. [`account_approved`] fires when a reviewer says yes,
//! and is the only path that grants the 5,000,000 µ$ signup credit or sends the
//! welcome email — registering alone earns neither.
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

/// Announce a new account awaiting review.
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
        "registration submitted; awaiting approval"
    );

    let (email, name, profile) = (email.to_owned(), name.to_owned(), profile.clone());
    tokio::spawn(async move {
        let site_url = systemprompt_email_extension::configured_site_url();
        let notice = systemprompt_email_extension::RegistrationNotice {
            name: &name,
            email: &email,
            company: &profile.company,
            role: &profile.role,
            team_size: &profile.team_size,
            why_assessing: &profile.why_assessing,
            credit_plans: profile.credit_plans.as_deref(),
        };
        if let Err(e) =
            systemprompt_email_extension::send_registration_notice(&notice, &site_url).await
        {
            tracing::warn!(email = %email, error = %e, "registration notice not sent");
        }
    });
}

// Why: grants the $5 signup credit idempotently, then fires the welcome email
// in a detached task. Only an approval reaches here, so this credit is the thing
// the manual review is actually gating.
pub(crate) async fn account_approved(pool: &PgPool, user_id: &UserId, email: &str, name: &str) {
    tracing::info!(user_id = %user_id, email, name, "account approved");

    match systemprompt_credits_extension::grant_signup_credit(pool, user_id.as_str()).await {
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
        let site_url = systemprompt_email_extension::configured_site_url();
        if let Err(e) =
            systemprompt_email_extension::send_welcome_email(&to, &recipient_name, &site_url).await
        {
            tracing::warn!(email = %to, error = %e, "welcome email not sent");
        }
    });
}
