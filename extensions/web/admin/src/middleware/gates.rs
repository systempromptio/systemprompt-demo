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

/// The request path as the client sent it.
///
/// `nest_service` strips its prefix from `request.uri()`, so a layer inside
/// the admin SSR router sees `/profile` where the caller asked for
/// `/admin/profile`. Anything matching against user-facing paths has to read
/// through `OriginalUri` instead.
fn original_path(request: &Request) -> String {
    request
        .extensions()
        .get::<axum::extract::OriginalUri>()
        .map_or_else(
            || request.uri().path().to_owned(),
            |o| o.0.path().to_owned(),
        )
}

/// The request path and query as the client sent it.
///
/// The query string must survive the login bounce: the bridge device-link
/// carries its loopback callback in `?redirect=...`, so dropping the query
/// strands the post-login return on a page whose extractor then 400s.
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

/// Holds an account at the pending page until an admin has reviewed it.
///
/// Sits above `non_admin_gate_middleware` so an unapproved user is bounced
/// before the role-based allowlist gets a say. Admins bypass unconditionally:
/// accounts predating the review gate carry no approval row, and locking the
/// only account that can approve people out of the approval queue is
/// unrecoverable.
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

fn may_pass_pending_gate(is_admin: bool, is_approved: bool, path: &str) -> bool {
    is_admin || is_approved || is_pending_allowed_path(path)
}

/// What an account under review may still reach: the pending page itself, the
/// sign-in and sign-out round trip, and the JSON API the shared page chrome
/// calls. Deliberately excludes `/admin/profile` and `/admin/settings` — the
/// non-admin fallback targets — or the bounce would land on a denied page.
fn is_pending_allowed_path(path: &str) -> bool {
    path.starts_with("/admin/api/")
        || path == "/admin/pending"
        || path == "/admin/continue"
        || path == "/admin/logout"
        || path == "/admin/login"
        || path == "/admin/register"
        || path == "/admin/add-passkey"
        || path == "/admin/verify-pending"
}

// Why: The path comes from `OriginalUri`, as it does in
// `require_user_middleware`: this layer sits inside a `nest_service("/admin",
// …)`, which strips the prefix, and every arm of `is_non_admin_allowed_path`
// matches on it.
pub(crate) async fn non_admin_gate_middleware(request: Request, next: Next) -> Response {
    let path = original_path(&request);
    let path = path.as_str();
    let user_ctx = request.extensions().get::<UserContext>().cloned();

    let Some(ctx) = user_ctx else {
        return next.run(request).await;
    };
    if ctx.is_admin || ctx.user_id.as_str().is_empty() {
        return next.run(request).await;
    }

    if is_non_admin_allowed_path(path) {
        next.run(request).await
    } else {
        axum::response::Redirect::to("/admin/profile").into_response()
    }
}

fn is_non_admin_allowed_path(path: &str) -> bool {
    path.starts_with("/admin/profile")
        || path.starts_with("/admin/settings")
        || path.starts_with("/admin/auth/")
        || path.starts_with("/admin/api/")
        || path == "/admin/logout"
        || path == "/admin/login"
        || path == "/admin/register"
        || path == "/admin/add-passkey"
        || path == "/admin/verify-pending"
        || path == "/admin/setup"
        || path == "/admin/pending"
        || path == "/admin/continue"
        || path == "/admin/devices/bridge-code"
        || path == "/admin/devices/pats"
        || path == "/admin/demo-register"
        || path == "/admin/"
        || path == "/admin"
}

#[cfg(test)]
mod tests {
    use super::{is_pending_allowed_path, may_pass_pending_gate};

    #[test]
    fn unapproved_user_is_held_at_the_pending_page() {
        for path in [
            "/admin/profile",
            "/admin/settings",
            "/admin/access/users",
            "/admin/setup",
            "/bridge-auth/device-link",
        ] {
            assert!(
                !may_pass_pending_gate(false, false, path),
                "{path} must bounce an unapproved user"
            );
        }
    }

    #[test]
    fn sign_in_and_sign_out_survive_the_gate() {
        // A bounce target that is itself bounced is an infinite redirect, and
        // an account that cannot reach logout is stuck in the browser session.
        for path in [
            "/admin/pending",
            "/admin/login",
            "/admin/logout",
            "/admin/continue",
            "/admin/register",
            "/admin/api/auth/me",
        ] {
            assert!(
                is_pending_allowed_path(path),
                "{path} must stay reachable while under review"
            );
            assert!(may_pass_pending_gate(false, false, path));
        }
    }

    #[test]
    fn admins_bypass_even_without_an_approval_row() {
        // Accounts predating the review gate carry no approval row. Locking the
        // only role that can approve anyone out of the queue is unrecoverable.
        assert!(may_pass_pending_gate(true, false, "/admin/access/users"));
    }

    #[test]
    fn approved_user_reaches_the_admin_plane() {
        assert!(may_pass_pending_gate(false, true, "/admin/profile"));
    }
}
