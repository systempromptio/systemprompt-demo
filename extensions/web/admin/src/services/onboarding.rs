//! Onboarding completion hook.
//!
//! [`onboarding_completed`] is the single seam the funnel calls once a user
//! finishes the onboarding form. Today it only logs; the integration pass wires
//! the credit grant (5,000,000 µ$) and the welcome email into this one function
//! so the HTTP handler never has to know those subsystems exist.

use sqlx::PgPool;
use systemprompt::identifiers::UserId;

/// Details captured by the onboarding form, forwarded to downstream
/// integrations (credits, email, CRM). Kept as an owned struct so the signature
/// is stable as the integration pass grows what it needs.
#[derive(Debug, Clone)]
pub(crate) struct OnboardingProfile {
    pub company: Option<String>,
    pub use_case: Option<String>,
}

/// Called exactly once per user, right after their onboarding form is
/// persisted.
///
/// Grants the $5 signup credit idempotently, then fires the welcome email in a
/// detached task so a slow or unconfigured SMTP relay can never fail
/// onboarding.
pub(crate) async fn onboarding_completed(
    pool: &PgPool,
    user_id: &UserId,
    email: &str,
    name: &str,
    profile: &OnboardingProfile,
) {
    tracing::info!(
        user_id = %user_id,
        email,
        name,
        company = profile.company.as_deref().unwrap_or(""),
        use_case = profile.use_case.as_deref().unwrap_or(""),
        "onboarding completed"
    );

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
