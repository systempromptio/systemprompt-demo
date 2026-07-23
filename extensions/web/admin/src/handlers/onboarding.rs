//! The onboarding form: a short "tell us who you are" step shown once, right
//! after a user creates their passkey.
//!
//! `GET /onboarding` renders the form prefilled from the session identity.
//! `POST /api/onboarding` persists it, fires the [`onboarding_completed`] hook
//! (credit grant + welcome email land here in the integration pass), and
//! redirects to `/admin/setup` with a success flag.

use std::sync::Arc;

use axum::Form;
use axum::extract::{Extension, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::{AdminError, AdminHtmlResult, AdminResult};
use crate::repositories;
use crate::services::onboarding::{OnboardingProfile, onboarding_completed};
use crate::templates::AdminTemplateEngine;
use crate::types::{MarketplaceContext, UserContext};

#[derive(Debug, Serialize)]
struct OnboardingPageContext {
    page: &'static str,
    title: &'static str,
    username: String,
    email: String,
}

pub(crate) async fn onboarding_page(
    Extension(user_ctx): Extension<UserContext>,
    Extension(mkt_ctx): Extension<MarketplaceContext>,
    Extension(engine): Extension<AdminTemplateEngine>,
) -> AdminHtmlResult<Response> {
    let ctx = OnboardingPageContext {
        page: "onboarding",
        title: "Tell us who you are",
        username: user_ctx.username.clone(),
        email: user_ctx.email.as_str().to_owned(),
    };
    Ok(crate::handlers::ssr::render_typed_page(
        &engine,
        "onboarding",
        &ctx,
        &user_ctx,
        &mkt_ctx,
    ))
}

#[derive(Debug, Deserialize)]
pub(crate) struct OnboardingForm {
    pub full_name: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub use_case: Option<String>,
}

pub(crate) async fn onboarding_submit(
    Extension(user_ctx): Extension<UserContext>,
    State(pool): State<Arc<PgPool>>,
    Form(form): Form<OnboardingForm>,
) -> AdminResult<Response> {
    let full_name = form.full_name.trim().to_owned();
    if full_name.is_empty() {
        return Err(AdminError::BadRequest("Full name is required".to_owned()));
    }
    let display_name = form
        .username
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    repositories::users::registration::mark_onboarded(
        &pool,
        &user_ctx.user_id,
        &full_name,
        display_name,
    )
    .await?;

    let profile = OnboardingProfile {
        company: form
            .company
            .map(|c| c.trim().to_owned())
            .filter(|s| !s.is_empty()),
        use_case: form
            .use_case
            .map(|u| u.trim().to_owned())
            .filter(|s| !s.is_empty()),
    };
    onboarding_completed(
        &pool,
        &user_ctx.user_id,
        user_ctx.email.as_str(),
        &full_name,
        &profile,
    )
    .await;

    Ok(Redirect::to("/admin/setup?welcome=1").into_response())
}

/// Post-login gateway: sends onboarded users to setup and everyone else to the
/// onboarding form. Kept server-side because only the DB knows whether the
/// user has finished onboarding.
pub(crate) async fn post_login_redirect(
    Extension(user_ctx): Extension<UserContext>,
    State(pool): State<Arc<PgPool>>,
) -> Response {
    // lint-ok: http-error
    if repositories::users::registration::is_onboarded(&pool, &user_ctx.user_id).await {
        Redirect::to("/admin/setup").into_response()
    } else {
        Redirect::to("/admin/onboarding").into_response()
    }
}
