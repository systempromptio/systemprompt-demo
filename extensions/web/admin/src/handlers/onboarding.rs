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
    pub company: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub team_size: String,
    #[serde(default)]
    pub why_assessing: String,
    #[serde(default)]
    pub credit_plans: Option<String>,
    /// Local path to resume after onboarding (e.g. the bridge device-link
    /// consent page). Only same-site paths are honored.
    #[serde(default)]
    pub redirect: Option<String>,
}

pub(crate) async fn onboarding_submit(
    Extension(user_ctx): Extension<UserContext>,
    State(pool): State<Arc<PgPool>>,
    Form(form): Form<OnboardingForm>,
) -> AdminResult<Response> {
    // Validate every required field before any side effect. The credit grant and
    // welcome email fire from `onboarding_completed`, so a missing field must
    // short-circuit here and never reach that hook.
    let full_name = required_field(&form.full_name, "Full name")?;
    let company = required_field(&form.company, "Company name")?;
    let role = required_field(&form.role, "Role or title")?;
    let team_size = required_field(&form.team_size, "Team size")?;
    let why_assessing = required_field(&form.why_assessing, "Why you are assessing systemprompt")?;

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
        company,
        role,
        team_size,
        why_assessing,
        credit_plans: form
            .credit_plans
            .map(|c| c.trim().to_owned())
            .filter(|s| !s.is_empty()),
    };

    repositories::users::registration::insert_onboarding_profile(
        &pool,
        &user_ctx.user_id,
        &repositories::users::registration::NewOnboardingProfile {
            company: &profile.company,
            role: &profile.role,
            team_size: &profile.team_size,
            why_assessing: &profile.why_assessing,
            credit_plans: profile.credit_plans.as_deref(),
        },
    )
    .await?;

    onboarding_completed(
        &pool,
        &user_ctx.user_id,
        user_ctx.email.as_str(),
        &full_name,
        &profile,
    )
    .await;

    let target = form
        .redirect
        .as_deref()
        .filter(|r| is_safe_local_redirect(r))
        .unwrap_or("/admin/setup?welcome=1");
    Ok(Redirect::to(target).into_response())
}

/// Same-site path check: a single leading `/` (not `//host` or a full URL),
/// so the post-onboarding bounce cannot leave the gateway.
fn is_safe_local_redirect(redirect: &str) -> bool {
    redirect.starts_with('/') && !redirect.starts_with("//")
}

fn required_field(value: &str, label: &str) -> AdminResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AdminError::BadRequest(format!("{label} is required")));
    }
    Ok(trimmed.to_owned())
}

// Why: kept server-side because only the DB knows whether the user has
// finished onboarding.
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
