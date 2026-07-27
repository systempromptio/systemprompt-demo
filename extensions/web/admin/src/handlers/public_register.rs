//! Public self-registration: the company details, the one-shot setup token it
//! issues, and the review request it opens.
//!
//! Registration is open to anyone, but it grants nothing. It creates the
//! account, records what the applicant says they are evaluating, and opens a
//! `pending` review. Access to the admin plane and the signup credit both wait
//! on a human approving that review.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use systemprompt::identifiers::{Email, UserId};

use crate::error::{AdminError, AdminResult};
use crate::repositories;
use crate::repositories::users::registration::{APPROVAL_DENIED, RegistrationState};
use crate::services::onboarding::{OnboardingProfile, registration_submitted};
use crate::types::CreateUserRequest;

const TOKEN_PREFIX: &str = "sp_wst_";

#[derive(Deserialize, Debug)]
pub(crate) struct PublicRegisterRequest {
    pub name: String,
    pub email: String,
    pub company: String,
    pub role: String,
    pub team_size: String,
    pub why_assessing: String,
    #[serde(default)]
    pub credit_plans: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicRegisterResponse {
    pub ok: bool,
    /// Set when the email already owns an account. The client sends them to
    /// sign in instead of creating a second passkey.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub already_registered: bool,
    /// Absent whenever `already_registered` is set — there is no passkey step
    /// to run, so there is nothing to authorise it with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<UserId>,
    pub display_name: String,
}

pub(crate) async fn public_register_handler(
    State(pool): State<Arc<PgPool>>,
    Json(body): Json<PublicRegisterRequest>,
) -> AdminResult<Response> {
    let email_str = body.email.trim().to_lowercase();
    let name = required_field(&body.name, "Name")?;

    if email_str.is_empty() || !email_str.contains('@') {
        return Err(AdminError::BadRequest("Invalid email address".to_owned()));
    }
    let email = Email::try_new(email_str.clone())
        // lint-ok: http-error — adapting a foreign, variant-less parse error.
        .map_err(|e| AdminError::BadRequest(format!("Invalid email address: {e}")))?;

    let profile = OnboardingProfile {
        company: required_field(&body.company, "Company name")?,
        role: required_field(&body.role, "Role or title")?,
        team_size: required_field(&body.team_size, "Team size")?,
        why_assessing: required_field(&body.why_assessing, "What you are evaluating")?,
        credit_plans: body
            .credit_plans
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned),
    };

    check_rate_limit(&pool, &email_str).await?;

    // Why: every branch is decided from this one read, before anything is
    // written. `create_user` upserts on the email, so writing first would let an
    // unauthenticated caller rewrite an existing account's profile — and then be
    // handed a credential-link token for it.
    let existing =
        repositories::users::registration::find_registration_state(&pool, &email_str).await;

    if let Some(ref state) = existing
        && !may_issue_token(state)
    {
        // Why: deliberately the same shape whether the account owns a passkey or
        // was denied. Neither may proceed, and the difference is not the
        // caller's business.
        return Ok(already_registered_response(&email_str, &name));
    }

    let user_id = existing.map_or_else(
        || UserId::new(uuid::Uuid::new_v4().to_string()),
        |state| state.user_id,
    );

    let (raw_token, token_hash) = generate_setup_token();
    persist_registration(
        &pool,
        &user_id,
        RegistrationWrite {
            name: &name,
            email,
            profile: &profile,
            token_id: &uuid::Uuid::new_v4().to_string(),
            token_hash: &token_hash,
        },
    )
    .await?;

    registration_submitted(&user_id, &email_str, &name, &profile);

    Ok((
        StatusCode::OK,
        Json(PublicRegisterResponse {
            ok: true,
            already_registered: false,
            token: Some(raw_token),
            email: email_str,
            user_id: Some(user_id),
            display_name: name,
        }),
    )
        .into_response())
}

fn may_issue_token(state: &RegistrationState) -> bool {
    !state.has_credential && state.approval_status.as_deref() != Some(APPROVAL_DENIED)
}

// lint-ok: http-error — a 200 telling the client to go sign in, not an error
fn already_registered_response(email: &str, name: &str) -> Response {
    (
        StatusCode::OK,
        Json(PublicRegisterResponse {
            ok: true,
            already_registered: true,
            token: None,
            email: email.to_owned(),
            user_id: None,
            display_name: name.to_owned(),
        }),
    )
        .into_response()
}

struct RegistrationWrite<'a> {
    name: &'a str,
    email: Email,
    profile: &'a OnboardingProfile,
    token_id: &'a str,
    token_hash: &'a str,
}

async fn persist_registration(
    pool: &PgPool,
    user_id: &UserId,
    write: RegistrationWrite<'_>,
) -> AdminResult<()> {
    let mut tx = pool.begin().await?;

    let create_req = CreateUserRequest {
        user_id: user_id.clone(),
        display_name: write.name.to_owned(),
        email: write.email,
        roles: vec!["user".to_owned()],
        status: Some("active".to_owned()),
    };
    repositories::users::mutations::create_user(&mut *tx, &create_req).await?;

    repositories::users::registration::mark_onboarded(&mut *tx, user_id, write.name, None).await?;

    repositories::users::registration::insert_onboarding_profile(
        &mut *tx,
        user_id,
        &repositories::users::registration::NewOnboardingProfile {
            company: &write.profile.company,
            role: &write.profile.role,
            team_size: &write.profile.team_size,
            why_assessing: &write.profile.why_assessing,
            credit_plans: write.profile.credit_plans.as_deref(),
        },
    )
    .await?;

    repositories::users::registration::insert_pending_approval(&mut *tx, user_id).await?;

    repositories::users::registration::insert_setup_token(
        &mut *tx,
        write.token_id,
        user_id,
        write.token_hash,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

fn required_field(value: &str, label: &str) -> AdminResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AdminError::BadRequest(format!("{label} is required")));
    }
    Ok(trimmed.to_owned())
}

async fn check_rate_limit(pool: &PgPool, email_str: &str) -> AdminResult<()> {
    let rate_count =
        repositories::users::registration::count_recent_setup_tokens(pool, email_str).await;

    if rate_count >= 5 {
        return Err(AdminError::RateLimited(
            "Too many registration attempts. Please try again later.".to_owned(),
        ));
    }
    Ok(())
}

fn generate_setup_token() -> (String, String) {
    let bytes: [u8; 32] = rand::rng().random();
    let raw_token = format!("{}{}", TOKEN_PREFIX, URL_SAFE_NO_PAD.encode(bytes));
    let token_hash = {
        let mut hasher = Sha256::new();
        hasher.update(raw_token.as_bytes());
        URL_SAFE_NO_PAD.encode(hasher.finalize())
    };
    (raw_token, token_hash)
}
