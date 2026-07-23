//! Credit ledger extension for systemprompt.io.
//!
//! Grants a one-time $5 signup credit and enforces the resulting balance at the
//! gateway. Balance is `SUM(credit_grants) −
//! SUM(ai_requests.cost_microdollars)` for the user; when it reaches zero the
//! registered [gateway guard] denies further requests.
//!
//! [gateway guard]: systemprompt::extension::GatewayRequestGuard
mod guard;

pub use guard::CreditBalanceGuard;

use systemprompt::extension::prelude::*;

/// The signup credit: $5, expressed in microdollars.
pub const SIGNUP_CREDIT_MICRODOLLARS: i64 = 5_000_000;
const SIGNUP_REASON: &str = "signup";

#[derive(Debug, thiserror::Error)]
pub enum CreditError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Grant the one-time signup credit to `user_id`.
///
/// Idempotent: a second call for the same user is a no-op thanks to the
/// `UNIQUE (user_id, reason)` constraint. Returns `true` when a new grant row
/// was inserted.
pub async fn grant_signup_credit(pool: &sqlx::PgPool, subject: &str) -> Result<bool, CreditError> {
    let result = sqlx::query!(
        "INSERT INTO credit_grants (user_id, microdollars, reason)
         VALUES ($1, $2, $3) ON CONFLICT (user_id, reason) DO NOTHING",
        subject,
        SIGNUP_CREDIT_MICRODOLLARS,
        SIGNUP_REASON,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// The user's remaining balance in microdollars: total grants minus total AI
/// request cost. May go negative if a request's cost is recorded after the
/// balance is exhausted.
pub async fn get_balance_microdollars(
    pool: &sqlx::PgPool,
    subject: &str,
) -> Result<i64, CreditError> {
    let balance = sqlx::query_scalar!(
        r#"SELECT
             (COALESCE((SELECT SUM(microdollars) FROM credit_grants WHERE user_id = $1), 0)
              - COALESCE((SELECT SUM(cost_microdollars) FROM ai_requests WHERE user_id = $1), 0)
             )::BIGINT AS "balance!""#,
        subject,
    )
    .fetch_one(pool)
    .await?;
    Ok(balance)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CreditsExtension;

impl CreditsExtension {
    pub const PREFIX: &'static str = "credits";

    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Extension for CreditsExtension {
    fn metadata(&self) -> ExtensionMetadata {
        ExtensionMetadata {
            id: "credits",
            name: "Credits",
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    fn schemas(&self) -> Vec<SchemaDefinition> {
        vec![SchemaDefinition::new(
            "credit_grants",
            include_str!("../schema/001_credit_grants.sql"),
        )]
    }
}

register_extension!(CreditsExtension);
register_gateway_guard!(CreditBalanceGuard);
