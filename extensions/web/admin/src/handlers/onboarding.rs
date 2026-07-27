//! Where a user lands after signing in.
//!
//! The company details this module used to collect are now part of
//! registration itself (`public_register`), so what remains is the post-login
//! fork: an account still under review goes to `/admin/pending`, an approved
//! one to the homepage.

use axum::Extension;
use axum::response::{IntoResponse, Redirect, Response};

use crate::types::UserContext;

/// Why: kept server-side because only the approval decision, not the session,
/// knows whether this account has anything waiting on review.
///
/// An approved account goes to `/`, which is now the whole product — the
/// terminal and the pane. There is no console left to land on.
// lint-ok: http-error — both arms are redirects
pub(crate) async fn post_login_redirect(Extension(user_ctx): Extension<UserContext>) -> Response {
    if user_ctx.is_approved || user_ctx.is_admin {
        Redirect::to("/").into_response()
    } else {
        Redirect::to("/admin/pending").into_response()
    }
}
