//! Who the caller is, and the two ways they get a token to prove it.
//!
//! Everything here answers the same question — *is this request allowed to
//! touch this conversation* — and answers it opaquely. A caller learns that
//! they were refused, never which of the conflated cases refused them.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Serialize;
use sqlx::PgPool;
use systemprompt::config::SecretsBootstrap;
use systemprompt::identifiers::UserId;

use super::registry::PiRegistry;
use super::{session, token};
use crate::error::{AdminError, AdminResult};
use crate::handlers::extract_user_from_cookie;
use crate::repositories;
use crate::types::UserContext;

#[derive(Debug, Serialize)]
struct IssuedToken {
    token: String,
    expires_at: i64,
}

/// Admin-only issuance for an arbitrary user, mounted on the authenticated
/// admin router rather than on the public pi router.
pub(crate) async fn issue_embed_token_handler(
    Extension(user_ctx): Extension<UserContext>,
    State(pool): State<Arc<PgPool>>,
    Path(target_user_id): Path<String>,
) -> AdminResult<Response> {
    if !user_ctx.is_admin {
        return Err(AdminError::Forbidden("Admin access required".to_owned()));
    }
    let target_user_id = UserId::new(target_user_id);
    mint_for(&pool, &target_user_id).await
}

/// Self-service issuance for whoever owns the session cookie.
///
/// Deliberately gated on *registration only*, not on approval: the human review
/// and the signup credit gate the Bridge, and holding the terminal behind them
/// too would mean a visitor who just signed up sees a dead widget. A pending
/// account still cannot reach the admin plane — that gate is unchanged and
/// lives in `middleware::gates`.
///
/// Mounted on the public pi router, so it must not assume any middleware has
/// run: the cookie is read and validated here.
pub(super) async fn issue_own_embed_token(
    State(pool): State<Arc<PgPool>>,
    headers: axum::http::HeaderMap,
) -> Response { // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Ok(session) = extract_user_from_cookie(&headers) else {
        return unauthorized();
    };
    mint_for(&pool, &session.user_id)
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

/// The shared mint. `find_share_token_version` returning `None` is what makes a
/// deleted account's cookie useless even while the JWT is still in date.
async fn mint_for(pool: &Arc<PgPool>, user_id: &UserId) -> AdminResult<Response> {
    let secret = SecretsBootstrap::manifest_signing_secret_seed().map_err(AdminError::internal)?;
    let version = repositories::users::find_share_token_version(pool, user_id)
        .await
        .map_err(AdminError::internal)?
        .ok_or_else(|| AdminError::NotFound("user not found".to_owned()))?;
    let exp = now_secs() + token::TTL_SECS;
    Ok(Json(IssuedToken {
        token: token::sign(&secret, user_id, version, exp),
        expires_at: exp,
    })
    .into_response())
}

/// Verify the embed token and recheck the revocation version against the DB.
pub(super) async fn authenticate(pool: &Arc<PgPool>, raw: &str) -> Option<UserId> {
    let secret = SecretsBootstrap::manifest_signing_secret_seed().ok()?;
    let (user_id, version) = match token::verify(&secret, raw, now_secs()) {
        Ok(v) => v,
        Err(reason) => {
            tracing::debug!(?reason, "pi embed token rejected");
            return None;
        },
    };
    // Revocation: bumping `share_token_version` invalidates every token issued
    // against the old one, so the signature alone is not sufficient.
    let current = repositories::users::find_share_token_version(pool, &user_id)
        .await
        .ok()??;
    (current == version).then_some(user_id)
}

pub(super) async fn authorize_session(
    pool: &Arc<PgPool>,
    registry: &PiRegistry,
    raw_token: &str,
    conversation_id: &str,
) -> Option<Arc<session::PiSession>> {
    let user_id = authenticate(pool, raw_token).await?;
    let session = registry.get(conversation_id)?;
    (session.user_id == user_id).then_some(session)
}

pub(super) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub(super) fn unauthorized() -> Response {
    // lint-ok: http-error — a widget-facing endpoint answers in its own shape
    problem(StatusCode::UNAUTHORIZED, "invalid or expired token")
}

pub(super) fn problem(status: StatusCode, message: &str) -> Response {
    // lint-ok: http-error — small JSON body the widget renders directly
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}
