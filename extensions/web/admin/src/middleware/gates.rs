//! Authentication and authorisation layers for the admin plane.
//!
//! These sit above `user_context_middleware`, which is what populates the
//! [`UserContext`] they read. They are separated from page context because
//! they answer a different question: context decides what a page renders,
//! these decide whether the request is allowed to reach one at all.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::handlers::shared::ErrorBody;
use crate::types::UserContext;

fn original_path(request: &Request) -> String {
    request
        .extensions()
        .get::<axum::extract::OriginalUri>()
        .map_or_else(
            || request.uri().path().to_owned(),
            |o| o.0.path().to_owned(),
        )
}

fn original_path_and_query(request: &Request) -> String {
    let from_uri = |uri: &axum::http::Uri| {
        uri.path_and_query()
            .map_or_else(|| uri.path().to_owned(), |pq| pq.as_str().to_owned())
    };
    request
        .extensions()
        .get::<axum::extract::OriginalUri>()
        .map_or_else(|| from_uri(request.uri()), |o| from_uri(&o.0))
}

pub(crate) async fn require_user_middleware(request: Request, next: Next) -> Response {
    let user_ctx = request.extensions().get::<UserContext>().cloned();
    match user_ctx {
        Some(ctx) if !ctx.user_id.as_str().is_empty() => next.run(request).await,
        _ => {
            let target = original_path_and_query(&request);
            let redirect_url = format!("/admin/login?redirect={}", urlencoding::encode(&target));
            axum::response::Redirect::temporary(&redirect_url).into_response()
        },
    }
}

pub(crate) async fn require_auth_middleware(request: Request, next: Next) -> Response {
    let user_ctx = request.extensions().get::<UserContext>().cloned();
    match user_ctx {
        Some(ctx) if !ctx.user_id.as_str().is_empty() => next.run(request).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            axum::Json(ErrorBody {
                error: "Authentication required".to_owned(),
            }),
        )
            .into_response(),
    }
}

pub(crate) async fn require_admin_middleware(request: Request, next: Next) -> Response {
    let user_ctx = request.extensions().get::<UserContext>().cloned();
    match user_ctx {
        Some(ctx) if ctx.is_admin => next.run(request).await,
        _ => (
            StatusCode::FORBIDDEN,
            axum::Json(ErrorBody {
                error: "Admin access required".to_owned(),
            }),
        )
            .into_response(),
    }
}

pub(crate) async fn require_approved_middleware(request: Request, next: Next) -> Response {
    let path = original_path(&request);
    let Some(ctx) = request.extensions().get::<UserContext>().cloned() else {
        return next.run(request).await;
    };

    if may_pass_pending_gate(ctx.is_admin, ctx.is_approved, path.as_str()) {
        return next.run(request).await;
    }

    axum::response::Redirect::to("/admin/pending").into_response()
}

pub fn may_pass_pending_gate(is_admin: bool, is_approved: bool, path: &str) -> bool {
    is_admin || is_approved || is_pending_allowed_path(path)
}

/// What an account under review may still reach.
///
/// The pending page itself, the sign-in and sign-out round trip, and the JSON
/// API. The rest of `/admin` is the Bridge's device endpoints, which wait on
/// approval by design.
///
/// `/admin/auth/` is here because the homepage pane's whoami lives there, not
/// under `/admin/api/`. Without it a visitor who has just registered — and so
/// is pending by definition — gets a redirect where they expect their identity,
/// and the pane flips straight back to the sign-in form they just completed.
pub fn is_pending_allowed_path(path: &str) -> bool {
    path.starts_with("/admin/api/")
        || path.starts_with("/admin/auth/")
        || path == "/admin/pending"
        || path == "/admin/continue"
        || path == "/admin/logout"
        || path == "/admin/login"
        || path == "/admin/register"
        || path == "/admin/add-passkey"
        || path == "/admin/verify-pending"
}
