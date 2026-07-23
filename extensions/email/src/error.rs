//! Error type for the email extension.

#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Failed to build email: {0}")]
    Build(#[from] lettre::error::Error),

    #[error("SMTP transport error: {0}")]
    Smtp(#[from] lettre::transport::smtp::Error),
}
