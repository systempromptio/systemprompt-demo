//! Email extension for systemprompt.io.
//!
//! Provides an SMTP-backed [`EmailService`] and the two transactional emails the
//! signup funnel needs: the internal notice that an account is waiting to be
//! reviewed, and the welcome / $5-credit message sent once it is approved. When
//! SMTP is unconfigured every send degrades to a logged no-op, so neither
//! registration nor approval can fail on account of email.
pub mod error;
pub mod notice;
pub mod palette;
mod service;
pub mod templates;

pub use error::EmailError;
pub use notice::RegistrationNotice;
pub use service::EmailService;

use systemprompt::extension::prelude::*;

/// Send the welcome / $5-credit email to `to`, addressed to `name`.
///
/// Links are built against `site_url`. Callable from other extension crates
/// (e.g. the onboarding handler). Never fails when SMTP is unconfigured — it
/// logs and returns `Ok(())`. A configured-but-failing send returns the
/// transport error.
pub async fn send_welcome_email(to: &str, name: &str, site_url: &str) -> Result<(), EmailError> {
    let Some(service) = EmailService::from_env() else {
        tracing::info!(
            to = %to,
            "email not configured; skipping welcome email (no-op)"
        );
        return Ok(());
    };
    service.send_welcome_email(to, name, site_url).await
}

/// Tell the reviewer an account is waiting on them.
///
/// Same no-op-when-unconfigured contract as [`send_welcome_email`]: a missing
/// SMTP config must never fail a registration.
pub async fn send_registration_notice(
    notice: &RegistrationNotice<'_>,
    site_url: &str,
) -> Result<(), EmailError> {
    let Some(service) = EmailService::from_env() else {
        tracing::info!(
            applicant = %notice.email,
            reviewer = %configured_admin_email(),
            "email not configured; skipping registration notice (no-op)"
        );
        return Ok(());
    };
    service
        .send_registration_notice(&configured_admin_email(), notice, site_url)
        .await
}

/// The public site URL links in outbound email are built against, resolved from
/// the `SITE_URL` env var or the `site_url` secret, defaulting to production.
#[must_use]
pub fn configured_site_url() -> String {
    service::read_secret("SITE_URL", "site_url")
        .unwrap_or_else(|| "https://systemprompt.io".to_owned())
}

/// Who reviews new accounts, and the address applicants are told to contact.
///
/// Resolved from `ADMIN_NOTIFY_EMAIL` or the `admin_notify_email` secret so a
/// deployment can redirect the queue without a rebuild.
#[must_use]
pub fn configured_admin_email() -> String {
    service::read_secret("ADMIN_NOTIFY_EMAIL", "admin_notify_email")
        .unwrap_or_else(|| "ed@systemprompt.io".to_owned())
}

#[derive(Debug, Default, Clone, Copy)]
pub struct EmailExtension;

impl EmailExtension {
    pub const PREFIX: &'static str = "email";

    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Extension for EmailExtension {
    fn metadata(&self) -> ExtensionMetadata {
        ExtensionMetadata {
            id: "email",
            name: "Email",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

register_extension!(EmailExtension);
