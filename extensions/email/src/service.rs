//! SMTP-backed send service. Configuration is read from the `smtp_*` /
//! `site_url` secrets in the validated `SecretsBootstrap` store. When SMTP is
//! not configured the service is absent and callers degrade to a logged no-op.

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use crate::error::EmailError;
use crate::{notice, templates};

pub(crate) fn secret(key: &str) -> Option<String> {
    systemprompt::config::SecretsBootstrap::get()
        .ok()
        .and_then(|s| s.get(key).cloned())
}

pub struct EmailService {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    site_url: String,
}

impl std::fmt::Debug for EmailService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmailService")
            .field("from", &self.from)
            .field("site_url", &self.site_url)
            .finish_non_exhaustive()
    }
}

impl EmailService {
    /// Build a service from the validated secrets store. Returns `None` when
    /// any required SMTP secret is missing, so registration and onboarding
    /// never fail because email is unconfigured.
    #[must_use]
    pub fn from_secrets() -> Option<Self> {
        let host = secret("smtp_host").or_else(|| {
            tracing::warn!("smtp_host secret not configured; email disabled");
            None
        })?;
        let port: u16 = secret("smtp_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(587);
        let username = secret("smtp_username")?;
        let password = secret("smtp_password")?;
        let from_str = secret("smtp_from").unwrap_or_else(|| username.clone());
        let site_url = secret("site_url").unwrap_or_else(|| "https://systemprompt.io".into());

        let from: Mailbox = if from_str.contains('<') {
            from_str
                .parse()
                .map_err(|e| {
                    tracing::error!(from = %from_str, error = %e, "Failed to parse smtp_from address");
                })
                .ok()?
        } else {
            Mailbox::new(
                Some("systemprompt.io".to_owned()),
                from_str
                    .parse()
                    .map_err(|e| {
                        tracing::error!(from = %from_str, error = %e, "Failed to parse smtp_from as email address");
                    })
                    .ok()?,
            )
        };
        let creds = Credentials::new(username, password);

        let transport = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
            .map_err(|e| {
                tracing::error!(host = %host, error = %e, "Failed to create SMTP transport");
            })
            .ok()?
            .port(port)
            .credentials(creds)
            .build();

        Some(Self {
            transport,
            from,
            site_url,
        })
    }

    #[must_use]
    pub fn site_url(&self) -> &str {
        &self.site_url
    }

    pub async fn send_welcome_email(
        &self,
        to_email: &str,
        display_name: &str,
        site_url: &str,
    ) -> Result<(), EmailError> {
        let to: Mailbox = to_email
            .parse()
            .map_err(|e| EmailError::BadRequest(format!("Invalid email address: {e}")))?;

        let email = templates::build_welcome_email(self.from.clone(), to, display_name, site_url)?;

        self.transport.send(email).await?;
        Ok(())
    }

    pub async fn send_registration_notice(
        &self,
        reviewer_email: &str,
        notice: &notice::RegistrationNotice<'_>,
        site_url: &str,
    ) -> Result<(), EmailError> {
        let to: Mailbox = reviewer_email
            .parse()
            .map_err(|e| EmailError::BadRequest(format!("Invalid reviewer address: {e}")))?;

        let email =
            notice::build_registration_notice_email(self.from.clone(), to, notice, site_url)?;

        self.transport.send(email).await?;
        Ok(())
    }
}
