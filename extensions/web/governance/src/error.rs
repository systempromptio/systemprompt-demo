//! The governance plane's HTTP error type.
//!
//! Mirrors the admin plane's contract: domain errors convert in via `From`,
//! the variant alone decides the status, and logging happens once in
//! `into_response`. Admin absorbs this type through its own `From` impl so a
//! failure crossing the crate boundary keeps its classification.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug, Error)]
pub enum GovernanceError {
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

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Bridge repository error: {0}")]
    BridgeRepo(#[from] crate::repositories::bridge::BridgeRepoError),

    #[error("Internal error: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl GovernanceError {
    #[must_use]
    pub fn internal<E>(err: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self::Internal(err.into())
    }

    #[must_use]
    pub fn unauthenticated<E>(err: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self::Unauthenticated(err.into())
    }

    #[must_use]
    pub const fn status(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) | Self::Unauthenticated(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::BridgeRepo(e) => e.status(),
            Self::Database(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    #[must_use]
    pub fn public_message(&self) -> String {
        match self {
            Self::NotFound(msg)
            | Self::BadRequest(msg)
            | Self::Unauthorized(msg)
            | Self::Forbidden(msg) => msg.clone(),
            Self::BridgeRepo(e) => e.public_message(),
            Self::Unauthenticated(_) => "Unauthorized".to_owned(),
            Self::Database(_) | Self::Internal(_) => "Internal server error".to_owned(),
        }
    }

    fn log(&self, status: StatusCode) {
        if status.is_server_error() {
            tracing::error!(error = %self, "Governance handler returned server error");
        } else {
            tracing::warn!(error = %self, "Governance handler returned client error");
        }
    }
}

impl From<systemprompt::models::errors::ConfigError> for GovernanceError {
    fn from(value: systemprompt::models::errors::ConfigError) -> Self {
        Self::Internal(Box::new(value))
    }
}

impl From<systemprompt::oauth::OauthError> for GovernanceError {
    fn from(value: systemprompt::oauth::OauthError) -> Self {
        Self::Unauthenticated(Box::new(value))
    }
}

impl IntoResponse for GovernanceError {
    fn into_response(self) -> Response {
        let status = self.status();
        self.log(status);
        let body = Json(ErrorBody {
            error: self.public_message(),
        });
        (status, body).into_response()
    }
}

pub type GovernanceResult<T> = Result<T, GovernanceError>;
