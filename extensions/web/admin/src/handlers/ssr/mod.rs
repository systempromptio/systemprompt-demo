//! Server-rendered admin pages.
//!
//! Each module owns one page: it builds a typed template context and renders a
//! `.hbs` template from `storage/files/admin/templates/` at request time.

use crate::error::{AdminHtmlError, AdminHtmlResult};
use crate::handlers::extract_user_from_cookie;
use crate::templates::AdminTemplateEngine;
use axum::Extension;
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect, Response};


pub(crate) mod bridge_downloads;
mod ssr_add_passkey;
mod ssr_bridge_device_link;
mod ssr_bridge_setup;
mod ssr_demo_help;
mod ssr_demo_trace;
pub(crate) mod ssr_helpers;

pub(crate) use ssr_add_passkey::add_passkey_page;
pub(crate) use ssr_bridge_device_link::{device_link_approve, device_link_deny, device_link_page};
pub(crate) use ssr_bridge_setup::bridge_setup_page;
pub(crate) use ssr_demo_trace::demo_trace_page;
pub(crate) use ssr_helpers::branding_context;

pub(crate) async fn login_page(
    Extension(engine): Extension<AdminTemplateEngine>,
) -> AdminHtmlResult<Response> {
    render_unauthenticated(&engine, "login")
}

pub(crate) async fn verify_pending_page(
    Extension(engine): Extension<AdminTemplateEngine>,
) -> AdminHtmlResult<Response> {
    render_unauthenticated(&engine, "verify-pending")
}

pub(crate) async fn pending_page(
    Extension(user_ctx): Extension<crate::types::UserContext>,
    Extension(engine): Extension<AdminTemplateEngine>,
    axum::extract::State(pool): axum::extract::State<std::sync::Arc<sqlx::PgPool>>,
) -> AdminHtmlResult<Response> {
    if user_ctx.is_approved || user_ctx.is_admin {
        return Ok(Redirect::to("/").into_response());
    }

    let applicant =
        crate::repositories::users::approvals::find_applicant(&pool, &user_ctx.user_id).await;

    let mut ctx = branding_context(&engine);
    if let Some(obj) = ctx.as_object_mut() {
        obj.insert("email".to_owned(), user_ctx.email.as_str().into());
        obj.insert(
            "display_name".to_owned(),
            applicant
                .as_ref()
                .map_or_else(|| user_ctx.username.clone(), |a| a.display_name.clone())
                .into(),
        );
        obj.insert(
            "company".to_owned(),
            applicant.map(|a| a.company).unwrap_or_default().into(),
        );
        obj.insert(
            "support_email".to_owned(),
            systemprompt_email_extension::configured_admin_email().into(),
        );
    }

    let html = engine
        .render("pending", &ctx)
        // lint-ok: http-error — every template render failure is a 500.
        .map_err(|e| AdminHtmlError::internal(format!("pending page render failed: {e:?}")))?;
    Ok(Html(html).into_response())
}

pub(crate) async fn register_page(
    headers: HeaderMap,
    Extension(engine): Extension<AdminTemplateEngine>,
) -> AdminHtmlResult<Response> {
    if extract_user_from_cookie(&headers).is_ok() {
        return Ok(Redirect::to("/admin/continue").into_response());
    }
    render_unauthenticated(&engine, "register")
}

fn render_unauthenticated(
    engine: &AdminTemplateEngine,
    template: &str,
) -> AdminHtmlResult<Response> {
    let html = engine
        .render(template, &branding_context(engine))
        // lint-ok: http-error — every template render failure is a 500.
        .map_err(|e| AdminHtmlError::internal(format!("{template} page render failed: {e:?}")))?;
    Ok(Html(html).into_response())
}
