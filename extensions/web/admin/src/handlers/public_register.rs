//! Public self-registration: the company details, the one-shot setup token it
//! issues, and the auto-approved account it creates.
//!
//! Registration is open to anyone. It creates the account, records what the
//! applicant says they are evaluating, and approves it on the spot — the
//! signup credit is granted immediately. The manual-review machinery (gate
//! middleware, pending page, admin approve endpoint) stays wired but never
//! triggers while signups are auto-approved.
//!
//! Two limiters stand in for that missing review, and they are orthogonal: the
//! email-keyed one slows a single address retrying, and the IP-keyed one caps
//! how many accounts — and therefore how much credit — one network can mint in
//! a day. Neither is an authorisation boundary; both exist to make farming the
//! signup credit tedious rather than impossible.

use std::net::IpAddr;
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
use systemprompt::api::services::middleware::client_addr::ClientIp;
use systemprompt::identifiers::{Email, UserId};

use crate::error::{AdminError, AdminResult};
use crate::repositories;
use crate::repositories::users::approvals::APPROVAL_DENIED;
use crate::repositories::users::registration::RegistrationState;
use crate::services::onboarding::{OnboardingProfile, account_approved, registration_submitted};
use crate::types::CreateUserRequest;
use crate::util::client_address::is_private_range;

const TOKEN_PREFIX: &str = "sp_wst_";

const REGISTRATIONS_PER_IP_PER_DAY: i64 = 3;

const IP_RATE_LIMITED_MESSAGE: &str = "This network has reached the signup limit for today. Please try again tomorrow, or contact \
     ed@systemprompt.io if you need access sooner.";

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

fn parse_registration(
    body: &PublicRegisterRequest,
) -> AdminResult<(String, String, Email, OnboardingProfile)> {
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
    Ok((email_str, name, email, profile))
}

pub(crate) async fn public_register_handler(
    State(pool): State<Arc<PgPool>>,
    ClientIp(client_ip): ClientIp,
    Json(body): Json<PublicRegisterRequest>,
) -> AdminResult<Response> {
    let (email_str, name, email, profile) = parse_registration(&body)?;

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

    // Why: deliberately after the branch above. That branch writes nothing and
    // grants nothing, so people behind one office NAT re-submitting an address
    // that already has an account must not spend the network's quota on it.
    check_ip_rate_limit(&pool, client_ip).await?;

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

    // Why: outside the transaction above, and only once it has committed — a
    // registration that failed must not spend quota, and a failed write here
    // must not undo a signup that succeeded.
    if let Some(ip) = client_ip.filter(|ip| !is_private_range(*ip)) {
        repositories::users::registration::insert_registration_attempt(&pool, ip, &email_str).await;
    }

    registration_submitted(&user_id, &email_str, &name, &profile);

    // Why: signups are auto-approved, so the credit grant fires here rather
    // than from the admin approve endpoint; idempotent, never fails the signup
    account_approved(&pool, &user_id, &email_str, &name).await;

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

    repositories::users::approvals::approve_on_signup(&mut *tx, user_id).await?;

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

async fn check_ip_rate_limit(pool: &PgPool, ip: Option<IpAddr>) -> AdminResult<()> {
    let Some(ip) = ip else {
        // Why: `None` means the request carried no peer address, which the
        // served router always supplies — only an in-process caller reaches
        // this. Failing closed here would 429 every signup rather than the
        // handful this limit exists to slow down.
        tracing::warn!("registration ip limit skipped: client address unresolved");
        return Ok(());
    };

    if is_private_range(ip) {
        tracing::warn!(
            ip = %ip,
            "registration ip limit skipped: client resolved to a private address"
        );
        return Ok(());
    }

    let count =
        repositories::users::registration::count_recent_registration_attempts(pool, ip).await;

    if count >= REGISTRATIONS_PER_IP_PER_DAY {
        return Err(AdminError::RateLimited(IP_RATE_LIMITED_MESSAGE.to_owned()));
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
