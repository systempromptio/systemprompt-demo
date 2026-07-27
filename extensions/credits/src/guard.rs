//! Gateway guard that denies requests once a user's credit balance is
//! exhausted.
//!
//! Balances are cached in-process for a short window so the hot gateway path
//! does not hit the database on every request. A denial is never cached beyond
//! the window, so a fresh grant frees the user within [`CACHE_TTL`].

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use systemprompt::extension::{GatewayDenyReason, GatewayRequestGuard};

const CACHE_TTL: Duration = Duration::from_secs(30);

struct CachedBalance {
    microdollars: i64,
    fetched_at: Instant,
}

static BALANCE_CACHE: LazyLock<Mutex<HashMap<String, CachedBalance>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cached_balance(subject: &str) -> Option<i64> {
    let cache = BALANCE_CACHE.lock().ok()?;
    let entry = cache.get(subject)?;
    let fresh = entry.fetched_at.elapsed() < CACHE_TTL;
    let value = entry.microdollars;
    drop(cache);
    fresh.then_some(value)
}

fn store_balance(subject: &str, microdollars: i64) {
    if let Ok(mut cache) = BALANCE_CACHE.lock() {
        cache.insert(
            subject.to_owned(),
            CachedBalance {
                microdollars,
                fetched_at: Instant::now(),
            },
        );
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CreditBalanceGuard;

#[async_trait::async_trait]
impl GatewayRequestGuard for CreditBalanceGuard {
    async fn check(&self, pool: &sqlx::PgPool, subject: &str) -> Result<(), GatewayDenyReason> {
        let balance = match cached_balance(subject) {
            Some(balance) => balance,
            None => match crate::get_balance_microdollars(pool, subject).await {
                Ok(balance) => {
                    store_balance(subject, balance);
                    balance
                },
                Err(e) => {
                    // Why: fail open — a ledger read error must not take the gateway down.
                    tracing::error!(error = %e, subject = %subject, "credit balance check failed; allowing request");
                    return Ok(());
                },
            },
        };

        if balance > 0 {
            Ok(())
        } else {
            // Grants vary per user, so the message must not name an amount.
            Err(GatewayDenyReason::new(
                "Credit exhausted. Your systemprompt credit has been used up — add credit to continue.",
            ))
        }
    }
}
