//! Where a user lands after signing in.
//!
//! The company details this module used to collect are now part of
//! registration itself (`public_register`), so what remains is the post-login
//! fork: an account still under review goes to `/admin/pending`, an approved
//! one to `/admin/setup`.

use axum::Extension;
use axum::response::{IntoResponse, Redirect, Response};

use crate::types::UserContext;

/// The onboarding form's old address.
///
/// Kept for one release so sessions that were mid-flow when the merged
/// registration shipped, and the root-level `/onboarding` redirect, land
/// somewhere real instead of 404ing.
// lint-ok: http-error — a redirect, not an error path
pub(crate) async fn onboarding_moved() -> Response {
    Redirect::permanent("/admin/continue").into_response()
}

/// Why: kept server-side because only the approval decision, not the session,
/// knows whether this account may see the admin plane yet.
// lint-ok: http-error — both arms are redirects
pub(crate) async fn post_login_redirect(Extension(user_ctx): Extension<UserContext>) -> Response {
    if user_ctx.is_approved || user_ctx.is_admin {
        Redirect::to("/admin/setup").into_response()
    } else {
        Redirect::to("/admin/pending").into_response()
    }
}
