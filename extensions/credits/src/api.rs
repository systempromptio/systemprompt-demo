//! `GET /api/credits/balance` — the caller's own credit position.
//!
//! Serves the Bridge profile card. Accepts either credential the bridge can
//! present: a `sp-live-` personal access token, or the short-lived gateway JWT
//! it trades that token for.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use systemprompt::database::Database;
use systemprompt::identifiers::UserId;
use systemprompt::users::{API_KEY_PREFIX, ApiKeyService};

use crate::CreditBalance;

#[derive(Clone)]
pub(crate) struct CreditsApiState {
    pub(crate) pool: Arc<sqlx::PgPool>,
    pub(crate) db: Arc<Database>,
}

pub(crate) fn router(state: CreditsApiState) -> axum::Router {
    axum::Router::new()
        .route("/balance", get(balance_handler))
        .with_state(state)
}

#[derive(serde::Serialize)]
struct BalanceResponse {
    balance_microdollars: i64,
    granted_microdollars: i64,
    spent_microdollars: i64,
    currency: &'static str,
}

impl From<CreditBalance> for BalanceResponse {
    fn from(b: CreditBalance) -> Self {
        Self {
            balance_microdollars: b.balance_microdollars,
            granted_microdollars: b.granted_microdollars,
            spent_microdollars: b.spent_microdollars,
            currency: "USD",
        }
    }
}

async fn balance_handler(
    State(state): State<CreditsApiState>,
    headers: HeaderMap,
) -> Result<Json<BalanceResponse>, (StatusCode, String)> {
    let user_id = authenticate(&state, &headers).await?;
    let balance = crate::get_balance(&state.pool, user_id.as_str())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "credit balance lookup failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read credit balance".to_owned(),
            )
        })?;
    Ok(Json(balance.into()))
}

/// Resolve the presented credential to a user.
///
/// This validates the JWT signature, issuer and expiry, but not the
/// user-exists / session-exists / JTI-revocation checks the gateway performs —
/// those live behind `JwtContextExtractor`, which is internal to the API
/// crate. Acceptable here: the endpoint is a read of the caller's own balance.
async fn authenticate(
    state: &CreditsApiState,
    headers: &HeaderMap,
) -> Result<UserId, (StatusCode, String)> {
    let credential = extract_credential(headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "Missing Authorization or x-api-key credential".to_owned(),
        )
    })?;

    if credential.starts_with(API_KEY_PREFIX) {
        let service = ApiKeyService::new(&state.db).map_err(|e| {
            tracing::error!(error = %e, "api key service init failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to verify credential".to_owned(),
            )
        })?;
        return match service.verify(credential).await {
            Ok(Some(record)) => Ok(record.user_id),
            Ok(None) => Err((StatusCode::UNAUTHORIZED, "Invalid API key".to_owned())),
            Err(e) => {
                tracing::error!(error = %e, "api key verification failed");
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to verify credential".to_owned(),
                ))
            },
        };
    }

    systemprompt::security::extract_user_context(credential)
        .map(|ctx| ctx.user_id)
        .map_err(|e| {
            tracing::debug!(error = %e, "credits endpoint token rejected");
            (
                StatusCode::UNAUTHORIZED,
                "Invalid or expired token".to_owned(),
            )
        })
}

fn extract_credential(headers: &HeaderMap) -> Option<&str> {
    if let Some(bearer) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(bearer);
    }
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}
