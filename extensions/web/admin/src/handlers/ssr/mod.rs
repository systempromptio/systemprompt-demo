//! Server-rendered admin pages.
//!
//! Each module owns one page: it builds a typed template context and renders a
//! `.hbs` template from `storage/files/admin/templates/` at request time.

use crate::error::{AdminHtmlError, AdminHtmlResult, AdminResult};
use crate::handlers::extract_user_from_cookie;
use crate::templates::AdminTemplateEngine;
use axum::Extension;
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect, Response};


mod bridge_downloads;
mod ssr_access_control;
mod ssr_add_passkey;
pub(crate) mod ssr_analytics_requests;
mod ssr_bridge_device_link;
mod ssr_bridge_setup;
mod ssr_chain;
mod ssr_context_detail;
mod ssr_conversations_raw;
mod ssr_demo_help;
mod ssr_demo_register;
mod ssr_governance;
mod ssr_governance_audit_detail;
mod ssr_governance_hooks;
mod ssr_governance_policy_edit;
pub(crate) mod ssr_helpers;
mod ssr_management;
mod ssr_perf_trace_detail;
mod ssr_perf_traces;
mod ssr_profile;
mod ssr_search_resolve;
mod ssr_session_detail;
mod ssr_settings;
mod ssr_setup;
mod ssr_skills_contexts;
mod ssr_users;
mod ssr_users_sessions;
pub(crate) mod types;

pub(crate) use ssr_access_control::access_control_page;
pub(crate) use ssr_add_passkey::add_passkey_page;
pub(crate) use ssr_analytics_requests::analytics_requests_page;
pub(crate) use ssr_bridge_device_link::{device_link_approve, device_link_deny, device_link_page};
pub(crate) use ssr_bridge_setup::bridge_setup_page;
pub(crate) use ssr_chain::chain_envelope;
pub(crate) use ssr_context_detail::context_detail_page;
pub(crate) use ssr_conversations_raw::conversations_raw;
pub(crate) use ssr_demo_register::demo_register_page;
pub(crate) use ssr_governance::governance_page;
pub(crate) use ssr_governance_audit_detail::governance_audit_detail_page;
pub(crate) use ssr_governance_hooks::governance_hooks_page;
pub(crate) use ssr_governance_policy_edit::{
    governance_policy_edit_page, governance_policy_toggle,
};
pub(crate) use ssr_helpers::{branding_context, render_page, render_typed_page};
pub(crate) use ssr_management::{
    management_department_detail_page, management_departments_page, management_devices_page,
};
pub(crate) use ssr_perf_trace_detail::perf_trace_detail_page;
pub(crate) use ssr_perf_traces::perf_traces_page;
pub(crate) use ssr_profile::profile_page;
pub(crate) use ssr_search_resolve::search_resolve;
pub(crate) use ssr_session_detail::session_detail_page;
pub(crate) use ssr_settings::settings_page;
pub(crate) use ssr_setup::setup_page;
pub(crate) use ssr_skills_contexts::skills_contexts_page;
pub(crate) use ssr_users::{user_detail_page, users_page};
pub(crate) use ssr_users_sessions::users_sessions_page;

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

/// The holding page for an account awaiting manual review.
///
/// Rendered without the admin chrome on purpose: every nav target is behind the
/// approval gate, so a normal page render would fill the screen with links that
/// bounce straight back here.
pub(crate) async fn pending_page(
    Extension(user_ctx): Extension<crate::types::UserContext>,
    Extension(engine): Extension<AdminTemplateEngine>,
    axum::extract::State(pool): axum::extract::State<std::sync::Arc<sqlx::PgPool>>,
) -> AdminHtmlResult<Response> {
    if user_ctx.is_approved || user_ctx.is_admin {
        return Ok(Redirect::to("/admin/setup").into_response());
    }

    let applicant =
        crate::repositories::users::registration::find_applicant(&pool, &user_ctx.user_id).await;

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

/// The pages reachable before sign-in, which therefore have no user or
/// marketplace context to inject and cannot go through `render_page`.
fn render_unauthenticated(
    engine: &AdminTemplateEngine,
    template: &str,
) -> AdminHtmlResult<Response> {
    let html = engine
        .render(template, &branding_context(engine))
        .map_err(|e| AdminHtmlError::internal(format!("{template} page render failed: {e:?}")))?;
    Ok(Html(html).into_response())
}

pub(crate) fn get_services_path() -> AdminResult<std::path::PathBuf> {
    super::shared::get_services_path()
}
