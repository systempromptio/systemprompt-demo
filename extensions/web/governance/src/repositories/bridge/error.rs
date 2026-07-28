//! Error type shared by the bridge repositories.

use axum::http::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BridgeRepoError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}

impl BridgeRepoError {
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        match self {
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    #[must_use]
    pub fn public_message(&self) -> String {
        match self {
            Self::Validation(msg) => msg.clone(),
            Self::Database(_) => "Internal server error".to_owned(),
        }
    }
}

pub type Result<T> = std::result::Result<T, BridgeRepoError>;
