//! Where a user lands after signing in.
//!
//! Registration (`public_register`) collects the company details, so all
//! that happens here is the post-login fork: an account still under review
//! goes to `/admin/pending`, an approved one to the homepage.

use axum::Extension;
use axum::response::{IntoResponse, Redirect, Response};

use crate::types::UserContext;

// lint-ok: http-error — both arms are redirects
pub(crate) async fn post_login_redirect(Extension(user_ctx): Extension<UserContext>) -> Response {
    if user_ctx.is_approved || user_ctx.is_admin {
        Redirect::to("/").into_response()
    } else {
        Redirect::to("/admin/pending").into_response()
    }
}
