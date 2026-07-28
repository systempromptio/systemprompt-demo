//! Resolving a caller identity from the request's cookie or bearer token.
//!
//! The admin plane, the governance webhooks, and the pi terminal all need the
//! same answer to "who is this", so the decode lives beside the authorization
//! dimensions rather than in any one surface's handler module.

use axum::http::HeaderMap;
use serde::Serialize;
use systemprompt::identifiers::{Email, UserId};
use systemprompt::models::Config;
use systemprompt::models::auth::JwtAudience;
use systemprompt::oauth::validate_jwt_token;

use crate::error::GovernanceError;

#[derive(Debug, Clone, Serialize)]
pub struct CookieSession {
    pub user_id: UserId,
    pub username: String,
    pub email: Email,
}

pub fn extract_user_from_cookie(
    headers: &HeaderMap,
) -> Result<CookieSession, GovernanceError> {
    let token = extract_token_from_headers(headers)?;

    let jwt_issuer = Config::get()?.jwt_issuer.clone();

    let claims = validate_jwt_token(&token, &jwt_issuer, &[JwtAudience::Api])?;

    let email = Email::try_new(claims.email.clone()).map_err(GovernanceError::unauthenticated)?;

    Ok(CookieSession {
        user_id: UserId::new(claims.sub),
        username: claims.username,
        email,
    })
}

fn extract_token_from_headers(headers: &HeaderMap) -> Result<String, GovernanceError> {
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok())
        && let Some(token) = auth
            .strip_prefix("Bearer ")
            .or_else(|| auth.strip_prefix("bearer "))
    {
        let t = token.trim();
        if !t.is_empty() {
            return Ok(t.to_owned());
        }
    }

    let cookie_header = headers
        .get("cookie")
        .ok_or_else(|| GovernanceError::Unauthorized("No cookie or Authorization header".to_owned()))?
        .to_str()
        // lint-ok: http-error — adapting `ToStrError`, which has no variants.
        .map_err(|e| GovernanceError::Unauthorized(format!("Invalid cookie header: {e}")))?;

    let token = cookie_header
        .split(';')
        .find_map(|c| c.trim().strip_prefix("access_token="))
        .ok_or_else(|| {
            GovernanceError::Unauthorized("No access_token cookie or Authorization: Bearer".to_owned())
        })?;

    if token.is_empty() {
        return Err(GovernanceError::Unauthorized(
            "Empty access_token cookie".to_owned(),
        ));
    }
    Ok(token.to_owned())
}
