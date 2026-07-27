//! Credit ledger extension for systemprompt.io.
//!
//! Grants a one-time $5 signup credit and enforces the resulting balance at the
//! gateway. Balance is `SUM(credit_grants) −
//! SUM(ai_requests.cost_microdollars)` for the user; when it reaches zero the
//! registered [gateway guard] denies further requests.
//!
//! [gateway guard]: systemprompt::extension::GatewayRequestGuard
mod api;
mod guard;

pub use guard::CreditBalanceGuard;

use systemprompt::extension::prelude::*;

/// The signup credit: $5, expressed in microdollars.
pub const SIGNUP_CREDIT_MICRODOLLARS: i64 = 5_000_000;
const SIGNUP_REASON: &str = "signup";

/// Microdollars per US dollar.
pub const MICRODOLLARS_PER_USD: i64 = 1_000_000;

#[derive(Debug, thiserror::Error)]
pub enum CreditError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// A user's credit position: what they have been granted, what their AI
/// requests have cost, and the difference.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct CreditBalance {
    /// `granted_microdollars - spent_microdollars`. Negative once a request's
    /// cost lands after the balance is already exhausted.
    pub balance_microdollars: i64,
    pub granted_microdollars: i64,
    pub spent_microdollars: i64,
}

/// One row of the grant ledger.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CreditGrant {
    pub microdollars: i64,
    pub reason: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Grant `microdollars` to `user_id` under `reason`.
///
/// Idempotent per `(user_id, reason)`: re-granting the same reason is a no-op
/// and returns `false`. Callers that want a repeatable top-up must vary the
/// reason.
pub async fn grant_credit(
    pool: &sqlx::PgPool,
    subject: &str,
    microdollars: i64,
    reason: &str,
) -> Result<bool, CreditError> {
    let result = sqlx::query!(
        "INSERT INTO credit_grants (user_id, microdollars, reason)
         VALUES ($1, $2, $3) ON CONFLICT (user_id, reason) DO NOTHING",
        subject,
        microdollars,
        reason,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Grant the one-time signup credit to `user_id`.
///
/// Idempotent: a second call for the same user is a no-op thanks to the
/// `UNIQUE (user_id, reason)` constraint. Returns `true` when a new grant row
/// was inserted.
pub async fn grant_signup_credit(pool: &sqlx::PgPool, subject: &str) -> Result<bool, CreditError> {
    grant_credit(pool, subject, SIGNUP_CREDIT_MICRODOLLARS, SIGNUP_REASON).await
}

/// The user's full credit position.
pub async fn get_balance(pool: &sqlx::PgPool, subject: &str) -> Result<CreditBalance, CreditError> {
    let row = sqlx::query!(
        r#"SELECT
             COALESCE((SELECT SUM(microdollars) FROM credit_grants WHERE user_id = $1), 0)::BIGINT
               AS "granted!",
             COALESCE((SELECT SUM(cost_microdollars) FROM ai_requests WHERE user_id = $1), 0)::BIGINT
               AS "spent!""#,
        subject,
    )
    .fetch_one(pool)
    .await?;
    Ok(CreditBalance {
        balance_microdollars: row.granted - row.spent,
        granted_microdollars: row.granted,
        spent_microdollars: row.spent,
    })
}

/// The user's remaining balance in microdollars: total grants minus total AI
/// request cost. May go negative if a request's cost is recorded after the
/// balance is exhausted.
pub async fn get_balance_microdollars(
    pool: &sqlx::PgPool,
    subject: &str,
) -> Result<i64, CreditError> {
    Ok(get_balance(pool, subject).await?.balance_microdollars)
}

/// Every grant made to `user_id`, newest first.
pub async fn list_grants(
    pool: &sqlx::PgPool,
    subject: &str,
) -> Result<Vec<CreditGrant>, CreditError> {
    let rows = sqlx::query!(
        r#"SELECT microdollars, reason, created_at
           FROM credit_grants WHERE user_id = $1 ORDER BY created_at DESC"#,
        subject,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| CreditGrant {
            microdollars: r.microdollars,
            reason: r.reason,
            created_at: r.created_at,
        })
        .collect())
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

    fn router(&self, ctx: &dyn ExtensionContext) -> Option<ExtensionRouter> {
        let handle = ctx.database();
        let database = handle
            .as_any()
            .downcast_ref::<systemprompt::database::Database>()?;
        let pool = database.pool()?;
        let db = std::sync::Arc::new(systemprompt::database::Database::from_pools(
            std::sync::Arc::clone(&pool),
            database.write_pool_arc().ok(),
        ));
        // Public mount: the handler authenticates the bearer credential itself,
        // because the bridge presents a PAT or gateway JWT rather than a
        // site session cookie.
        Some(ExtensionRouter::public(
            api::router(api::CreditsApiState { pool, db }),
            "/api/credits",
        ))
    }
}

register_extension!(CreditsExtension);
register_gateway_guard!(CreditBalanceGuard);
