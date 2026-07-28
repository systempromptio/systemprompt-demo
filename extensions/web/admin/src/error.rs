//! The admin plane's HTTP error type.
//!
//! Domain errors convert in via `From`, and the variant alone decides the
//! status code, so handlers propagate with a bare `?` rather than mapping at
//! each call site. Logging happens once, in `into_response`.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use systemprompt_web_shared::html_escape;
use thiserror::Error;

use crate::handlers::shared::ErrorBody;
use crate::repositories::bridge::BridgeRepoError;
use crate::repositories::secrets::secret_crypto::SecretCryptoError;
use systemprompt::traits::ExtensionError;
use systemprompt_web_shared::error::WebError;

#[derive(Debug, Error)]
pub enum AdminError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Authentication failed: {0}")]
    Unauthenticated(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Too many requests: {0}")]
    RateLimited(String),

    #[error("Unavailable: {0}")]
    Unavailable(String),

    #[error("Upstream error: {0}")]
    Upstream(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Bridge repository error: {0}")]
    BridgeRepo(BridgeRepoError),

    #[error("Marketplace error: {0}")]
    Marketplace(WebError),

    #[error("Crypto error: {0}")]
    Crypto(#[from] SecretCryptoError),

    #[error("Internal error: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl AdminError {
    #[must_use]
    pub fn internal<E>(err: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self::Internal(err.into())
    }

    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) | Self::Unauthenticated(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::BridgeRepo(e) => e.status(),
            Self::Marketplace(e) => ExtensionError::status(e),
            Self::Database(_) | Self::Crypto(_) | Self::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            },
        }
    }

    fn public_message(&self) -> String {
        match self {
            Self::NotFound(msg)
            | Self::BadRequest(msg)
            | Self::Unauthorized(msg)
            | Self::Forbidden(msg)
            | Self::Conflict(msg)
            | Self::RateLimited(msg)
            | Self::Unavailable(msg) => msg.clone(),
            Self::BridgeRepo(e) => e.public_message(),
            Self::Marketplace(e) => e.public_message(),
            Self::Upstream(_) => "Upstream service error".to_owned(),
            Self::Unauthenticated(_) => "Unauthorized".to_owned(),
            Self::Crypto(_) => "Internal configuration error".to_owned(),
            Self::Database(_) | Self::Internal(_) => "Internal server error".to_owned(),
        }
    }
}

impl From<BridgeRepoError> for AdminError {
    fn from(value: BridgeRepoError) -> Self {
        Self::BridgeRepo(value)
    }
}

impl From<WebError> for AdminError {
    fn from(value: WebError) -> Self {
        Self::Marketplace(value)
    }
}

impl AdminError {
    #[must_use]
    pub fn unauthenticated<E>(err: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self::Unauthenticated(err.into())
    }
}

impl From<systemprompt::oauth::OauthError> for AdminError {
    fn from(value: systemprompt::oauth::OauthError) -> Self {
        Self::Unauthenticated(Box::new(value))
    }
}

impl From<systemprompt::models::errors::ConfigError> for AdminError {
    fn from(value: systemprompt::models::errors::ConfigError) -> Self {
        Self::Internal(Box::new(value))
    }
}

impl From<systemprompt::config::ProfileBootstrapError> for AdminError {
    fn from(value: systemprompt::config::ProfileBootstrapError) -> Self {
        Self::Internal(Box::new(value))
    }
}

impl AdminError {
    fn log(&self, status: StatusCode) {
        if status.is_server_error() {
            tracing::error!(error = %self, "Admin handler returned server error");
        } else {
            tracing::warn!(error = %self, "Admin handler returned client error");
        }
    }
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let status = self.status();
        self.log(status);
        let body = Json(ErrorBody {
            error: self.public_message(),
        });
        (status, body).into_response()
    }
}

/// The HTML face of [`AdminError`], for the server-rendered admin pages.
///
/// A browser navigating to a page needs a page, not a JSON body — but the
/// status and the client-visible text come from the same classification either
/// way, so an SSR handler cannot accidentally disagree with an API handler
/// about what a given failure means. Unlike the hand-rolled error pages this
/// replaces, it renders the error's public message, so an internal cause
/// is logged rather than interpolated into the page.
#[derive(Debug, Error)]
#[error(transparent)]
pub struct AdminHtmlError(pub AdminError);

impl IntoResponse for AdminHtmlError {
    fn into_response(self) -> Response {
        let status = self.0.status();
        self.0.log(status);
        let body = Html(format!(
            "<h1>{}</h1><p>{}</p>",
            status.canonical_reason().unwrap_or("Error"),
            html_escape(&self.0.public_message())
        ));
        (status, body).into_response()
    }
}

impl AdminHtmlError {
    #[must_use]
    pub fn internal<E>(err: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self(AdminError::Internal(err.into()))
    }
}

impl<E: Into<AdminError>> From<E> for AdminHtmlError {
    fn from(value: E) -> Self {
        Self(value.into())
    }
}

pub type AdminResult<T> = Result<T, AdminError>;

pub type AdminHtmlResult<T> = Result<T, AdminHtmlError>;
