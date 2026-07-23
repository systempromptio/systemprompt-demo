//! SMTP-backed send service. Configuration is read from `SMTP_*` environment
//! variables, falling back to the `smtp_*` / `site_url` secrets in the
//! `SecretsBootstrap` store. When SMTP is not configured the service is absent
//! and callers degrade to a logged no-op.

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use crate::error::EmailError;
use crate::templates;

pub(crate) fn read_secret(env_key: &str, secrets_key: &str) -> Option<String> {
    std::env::var(env_key).ok().or_else(|| {
        systemprompt::config::SecretsBootstrap::get()
            .ok()
            .and_then(|s| s.get(secrets_key).cloned())
    })
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
    /// Build a service from the environment/secrets. Returns `None` when any
    /// required SMTP secret is missing, so registration and onboarding never
    /// fail because email is unconfigured.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let host = read_secret("SMTP_HOST", "smtp_host").or_else(|| {
            tracing::warn!("SMTP_HOST secret not configured; email disabled");
            None
        })?;
        let port: u16 = read_secret("SMTP_PORT", "smtp_port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(587);
        let username = read_secret("SMTP_USERNAME", "smtp_username")?;
        let password = read_secret("SMTP_PASSWORD", "smtp_password")?;
        let from_str = read_secret("SMTP_FROM", "smtp_from").unwrap_or_else(|| username.clone());
        let site_url =
            read_secret("SITE_URL", "site_url").unwrap_or_else(|| "https://systemprompt.io".into());

        let from: Mailbox = if from_str.contains('<') {
            from_str
                .parse()
                .map_err(|e| {
                    tracing::error!(from = %from_str, error = %e, "Failed to parse SMTP_FROM address");
                })
                .ok()?
        } else {
            Mailbox::new(
                Some("systemprompt.io".to_owned()),
                from_str
                    .parse()
                    .map_err(|e| {
                        tracing::error!(from = %from_str, error = %e, "Failed to parse SMTP_FROM as email address");
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

    /// The configured public site URL (used to build links in emails).
    #[must_use]
    pub fn site_url(&self) -> &str {
        &self.site_url
    }

    /// Send the welcome / $5-credit email. `site_url` overrides the configured
    /// site URL for link building (pass the request's site URL when known).
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
}
