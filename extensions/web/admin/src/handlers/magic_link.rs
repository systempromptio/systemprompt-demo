//! Passwordless email sign-in.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use systemprompt::api::services::middleware::client_addr::ClientIp;
use systemprompt::identifiers::Email;

use crate::repositories::users::magic_links;

use super::shared::ErrorBody;

#[derive(Deserialize, Debug)]
pub(crate) struct MagicLinkRequest {
    pub email: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct MagicLinkResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum MagicLinkRequestResult {
    Ok(MagicLinkResponse),
    Err(ErrorBody),
}

const RATE_LIMITED_MESSAGE: &str =
    "If an account exists for that email, a magic link has been sent.";

pub(crate) async fn request_magic_link(
    State(pool): State<Arc<PgPool>>,
    ClientIp(client_ip): ClientIp,
    Json(body): Json<MagicLinkRequest>,
) -> impl IntoResponse {
    // lint-ok: http-error — answers identically whether or not the account exists,
    // so a typed error would leak it
    let email = body.email.trim().to_lowercase();

    if Email::try_new(email.clone()).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(MagicLinkRequestResult::Err(ErrorBody {
                error: "Invalid email address".to_owned(),
            })),
        );
    }

    let count = magic_links::count_recent_tokens(&pool, &email)
        .await
        .unwrap_or(0);

    if count >= 3 {
        return (
            StatusCode::OK,
            Json(MagicLinkRequestResult::Ok(MagicLinkResponse {
                ok: true,
                message: RATE_LIMITED_MESSAGE.to_owned(),
            })),
        );
    }

    // Why: an address the trust-gated resolver could not establish is left out
    // of the count and off the stored token rather than bucketed under a
    // sentinel, which would merge every such caller into one shared limit.
    let ip_address = client_ip.map(|ip| ip.to_string());

    if let Some(ref ip) = ip_address {
        let ip_count = magic_links::count_recent_tokens_by_ip(&pool, ip)
            .await
            .unwrap_or(0);

        if ip_count >= 10 {
            return (
                StatusCode::OK,
                Json(MagicLinkRequestResult::Ok(MagicLinkResponse {
                    ok: true,
                    message: RATE_LIMITED_MESSAGE.to_owned(),
                })),
            );
        }
    }

    let user_exists = magic_links::user_exists_by_email(&pool, &email)
        .await
        .unwrap_or(false);

    if user_exists
        && let Ok(_raw_token) =
            magic_links::create_magic_link_token(&pool, &email, ip_address.as_deref()).await
    {
        tracing::info!(email = %email, "Magic link token created (email sending not configured in this deployment)");
    }

    (
        StatusCode::OK,
        Json(MagicLinkRequestResult::Ok(MagicLinkResponse {
            ok: true,
            message: RATE_LIMITED_MESSAGE.to_owned(),
        })),
    )
}

#[derive(Deserialize, Debug)]
pub(crate) struct ValidateTokenRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ValidateTokenResponse {
    pub ok: bool,
    pub email: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ValidateTokenError {
    pub ok: bool,
    pub error: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum ValidateTokenResult {
    Ok(ValidateTokenResponse),
    Err(ValidateTokenError),
}

pub(crate) async fn validate_magic_link(
    State(pool): State<Arc<PgPool>>,
    Json(body): Json<ValidateTokenRequest>,
) -> impl IntoResponse {
    // lint-ok: http-error — answers identically whether or not the account exists,
    // so a typed error would leak it
    magic_links::consume_magic_link_token(&pool, &body.token)
        .await
        .map_or_else(
            |_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ValidateTokenResult::Err(ValidateTokenError {
                        ok: false,
                        error: "This link is invalid or has expired. Please request a new one."
                            .to_owned(),
                    })),
                )
            },
            |email| {
                (
                    StatusCode::OK,
                    Json(ValidateTokenResult::Ok(ValidateTokenResponse {
                        ok: true,
                        email,
                    })),
                )
            },
        )
}
