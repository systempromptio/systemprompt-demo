//! Approving an account, and the credit that approval grants.
//!
//! Split from `users.rs`: these are the only handlers that move money rather
//! than user records, and they share the review workflow that gates the
//! Bridge. The pi web terminal deliberately does *not* wait on this — it is
//! gated on registration alone.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::response::{IntoResponse, Response};
use sqlx::PgPool;
use systemprompt::identifiers::UserId;

use crate::activity::{self, ActivityEntity, NewActivity};
use crate::error::{AdminError, AdminResult};
use crate::repositories;
use crate::types::UserContext;

pub(crate) async fn approve_user_handler(
    State(pool): State<Arc<PgPool>>,
    Extension(user_ctx): Extension<UserContext>,
    Path(user_id_raw): Path<String>,
) -> AdminResult<Response> {
    let user_id = UserId::new(user_id_raw);
    if !user_ctx.is_admin {
        return Err(AdminError::Forbidden("Admin access required".to_owned()));
    }

    let applicant = repositories::users::approvals::find_applicant(&pool, &user_id)
        .await
        .ok_or_else(|| AdminError::NotFound("No pending registration for user".to_owned()))?;

    repositories::users::approvals::set_approval_status(
        &pool,
        &user_id,
        repositories::users::approvals::APPROVAL_APPROVED,
        user_ctx.user_id.as_str(),
    )
    .await?;

    crate::services::onboarding::account_approved(
        &pool,
        &user_id,
        &applicant.email,
        &applicant.display_name,
    )
    .await;

    let p = Arc::clone(&pool);
    let uid = user_ctx.user_id.clone();
    let name = applicant.display_name.clone();
    let target = user_id.clone();
    tokio::spawn(async move {
        activity::record(
            &p,
            NewActivity::entity_updated(&uid, ActivityEntity::User, target.as_str(), &name),
        )
        .await;
    });

    Ok(Json(serde_json::json!({ "ok": true, "user_id": user_id })).into_response())
}

#[derive(serde::Deserialize)]
pub(crate) struct GrantCreditRequest {
    usd: f64,
    #[serde(default)]
    reason: Option<String>,
}

pub(crate) async fn grant_credit_handler(
    State(pool): State<Arc<PgPool>>,
    Extension(user_ctx): Extension<UserContext>,
    Path(user_id_raw): Path<String>,
    Json(body): Json<GrantCreditRequest>,
) -> AdminResult<Response> {
    let user_id = UserId::new(user_id_raw);
    if !user_ctx.is_admin {
        return Err(AdminError::Forbidden("Admin access required".to_owned()));
    }
    if !body.usd.is_finite() || body.usd <= 0.0 {
        return Err(AdminError::BadRequest(
            "Credit amount must be a positive number of dollars".to_owned(),
        ));
    }

    let microdollars =
        (body.usd * systemprompt_credits::MICRODOLLARS_PER_USD as f64).round() as i64;
    let reason = body
        .reason
        .map(|r| r.trim().to_owned())
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| format!("manual-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ")));

    let granted = systemprompt_credits::grant_credit(
        &pool,
        user_id.as_str(),
        microdollars,
        &reason,
    )
    .await
    .map_err(AdminError::internal)?;
    let balance = systemprompt_credits::get_balance(&pool, user_id.as_str())
        .await
        .map_err(AdminError::internal)?;

    if granted {
        let p = Arc::clone(&pool);
        let uid = user_ctx.user_id.clone();
        let target = user_id.clone();
        let label = format!("credit +${:.2} ({reason})", body.usd);
        tokio::spawn(async move {
            activity::record(
                &p,
                NewActivity::entity_updated(&uid, ActivityEntity::User, target.as_str(), &label),
            )
            .await;
        });
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "granted": granted,
        "reason": reason,
        "balance_microdollars": balance.balance_microdollars,
    }))
    .into_response())
}
