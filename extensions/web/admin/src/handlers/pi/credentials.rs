//! The gateway credential one conversation runs on.
//!
//! A conversation authenticates to `/v1/messages` as the user who opened it,
//! not as the deployment. That is not a hygiene preference: the gateway attests
//! `x-session-id` against the identity that authenticated and rejects a
//! mismatch, so a credential shared across users works for exactly one of them.
//!
//! Everything here is therefore paired with a conversation's lifetime — minted
//! when it starts, revoked when it ends, and swept at boot for the conversations
//! a hard kill ended without telling anyone.

use chrono::TimeDelta;
use sqlx::PgPool;
use systemprompt::identifiers::UserId;

use crate::repositories::bridge::IssuedApiKey;
use crate::services::device_service;

/// Name prefix on every PAT this module mints. The boot sweep matches on it, so
/// it has to be one constant rather than two literals that can drift apart.
pub(super) const PI_PAT_PREFIX: &str = "pi-conversation ";

/// How far a conversation's PAT outlives the conversation itself.
///
/// Expiry is the only backstop when the process dies without running teardown,
/// so this is deliberately small — long enough that a turn in flight at the
/// lifetime ceiling still completes, not long enough to matter if it leaks.
const PAT_GRACE: TimeDelta = TimeDelta::minutes(5);

/// Mint the credential one conversation will run on.
///
/// The PAT carries no scopes and no roles — on the API-key arm the gateway
/// resolves neither — so it is bearer-equivalent to the user for `/v1/*`. That
/// is the whole reason for the bounded lifetime and for revoking on teardown
/// rather than letting it lapse.
pub(super) async fn issue(
    pool: &PgPool,
    user_id: &UserId,
    conversation_id: &str,
    max_lifetime: std::time::Duration,
) -> Result<IssuedApiKey, String> {
    let expires_at = chrono::Utc::now()
        + TimeDelta::from_std(max_lifetime).unwrap_or(PAT_GRACE)
        + PAT_GRACE;
    device_service::issue_pat(
        pool,
        user_id,
        &format!("{PI_PAT_PREFIX}{conversation_id}"),
        Some(expires_at),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Retire one conversation's PAT.
///
/// Best-effort and logged: a revoke that fails must not stop the child being
/// killed, but it must not vanish either — an unrevoked key is a live
/// credential, and the log line is the only thing that says so.
pub(super) async fn revoke(pool: &PgPool, user_id: &UserId, api_key_id: &str) {
    if let Err(e) = device_service::revoke_pat(pool, user_id, api_key_id).await {
        tracing::warn!(
            api_key_id = %api_key_id,
            error = %e,
            "could not revoke a pi conversation's gateway credential"
        );
    }
}

/// Retire credentials a previous process left live.
///
/// No pi conversation survives a restart — the children are gone with it — so
/// every unrevoked `pi-conversation ` key at boot is by definition orphaned.
/// Without this a `kill -9` leaves a bearer-equivalent credential valid until
/// its expiry, and the session row it pairs with never expires at all: the
/// gateway's lookup filters on `revoked_at`, not `expires_at`.
pub(super) async fn sweep_orphans(pool: &PgPool) {
    match device_service::revoke_pats_by_name_prefix(pool, PI_PAT_PREFIX).await {
        Ok(0) => {},
        Ok(n) => tracing::info!(revoked = n, "swept orphaned pi conversation credentials"),
        Err(e) => tracing::warn!(error = %e, "could not sweep orphaned pi credentials"),
    }
}
