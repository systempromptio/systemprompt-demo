//! Email extension for systemprompt.io.
//!
//! Provides an SMTP-backed [`EmailService`] and the two transactional emails
//! the signup funnel needs: the internal notice that an account is waiting to
//! be reviewed, and the welcome / $1-credit message sent once it is approved.
//! When SMTP is unconfigured every send degrades to a logged no-op, so neither
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

/// Links are built against `site_url`.
///
/// Never fails when SMTP is unconfigured — it logs and returns `Ok(())`. A
/// configured-but-failing send returns the transport error.
pub async fn send_welcome_email(to: &str, name: &str, site_url: &str) -> Result<(), EmailError> {
    let Some(service) = EmailService::from_secrets() else {
        tracing::info!(
            to = %to,
            "email not configured; skipping welcome email (no-op)"
        );
        return Ok(());
    };
    service.send_welcome_email(to, name, site_url).await
}

/// Same no-op-when-unconfigured contract as [`send_welcome_email`]: a missing
/// SMTP config must never fail a registration.
pub async fn send_registration_notice(
    notice: &RegistrationNotice<'_>,
    site_url: &str,
) -> Result<(), EmailError> {
    let Some(service) = EmailService::from_secrets() else {
        tracing::info!(
            signup = %notice.email,
            "email not configured; skipping registration notice (no-op)"
        );
        return Ok(());
    };
    let Some(notify) = configured_admin_email() else {
        tracing::warn!(
            signup = %notice.email,
            "admin_notify_email secret not set; skipping registration notice"
        );
        return Ok(());
    };
    service
        .send_registration_notice(&notify, notice, site_url)
        .await
}

/// The public site URL links in outbound email are built against.
///
/// Resolved from the `site_url` secret, falling back to this deployment's own
/// `api_external_url` so a fork never mails links to someone else's domain.
#[must_use]
pub fn configured_site_url() -> String {
    service::secret("site_url").unwrap_or_else(|| {
        systemprompt::models::Config::get().map_or_else(
            |_| String::new(),
            |c| c.api_external_url.trim_end_matches('/').to_owned(),
        )
    })
}

/// Who reviews new accounts, and the address applicants are told to contact.
///
/// Resolved from the `admin_notify_email` secret so a deployment can redirect
/// the queue without a rebuild. `None` means no reviewer is configured —
/// callers skip the notice rather than mailing a hardcoded stranger.
#[must_use]
pub fn configured_admin_email() -> Option<String> {
    service::secret("admin_notify_email")
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
