//! The gateway credential one conversation runs on.
//!
//! A conversation authenticates to `/v1/messages` as the user who opened it,
//! not as the deployment. That is not a hygiene preference: the gateway attests
//! `x-session-id` against the identity that authenticated and rejects a
//! mismatch, so a credential shared across users works for exactly one of them.
//!
//! Everything here is therefore paired with a conversation's lifetime — minted
//! when it starts, revoked when it ends, and swept at boot for the
//! conversations a hard kill ended without telling anyone.

use chrono::TimeDelta;
use sqlx::PgPool;
use systemprompt::identifiers::{ContextId, UserId};

use crate::repositories::bridge::IssuedApiKey;
use crate::services::device_service;

pub(super) const PI_PAT_PREFIX: &str = "pi-conversation ";

const PAT_GRACE: TimeDelta = TimeDelta::minutes(5);

pub(super) async fn issue(
    pool: &PgPool,
    user_id: &UserId,
    conversation_id: &ContextId,
    max_lifetime: std::time::Duration,
) -> Result<IssuedApiKey, String> {
    let expires_at =
        chrono::Utc::now() + TimeDelta::from_std(max_lifetime).unwrap_or(PAT_GRACE) + PAT_GRACE;
    device_service::issue_pat(
        pool,
        user_id,
        &format!("{PI_PAT_PREFIX}{conversation_id}"),
        Some(expires_at),
    )
    .await
    .map_err(|e| e.to_string())
}

pub(super) async fn revoke(pool: &PgPool, user_id: &UserId, api_key_id: &str) {
    if let Err(e) = device_service::revoke_pat(pool, user_id, api_key_id).await {
        tracing::warn!(
            api_key_id = %api_key_id,
            error = %e,
            "could not revoke a pi conversation's gateway credential"
        );
    }
}

// Why: `live` is what makes the periodic sweep orphan-only — a running
// conversation's credential must survive it; boot passes an empty slice
// because nothing is live yet.
pub(super) async fn sweep_orphans(pool: &PgPool, live: &[ContextId]) {
    let keep: Vec<String> = live
        .iter()
        .map(|id| format!("{PI_PAT_PREFIX}{id}"))
        .collect();
    match device_service::revoke_pats_by_name_prefix(pool, PI_PAT_PREFIX, &keep).await {
        Ok(0) => {},
        Ok(n) => tracing::info!(revoked = n, "swept orphaned pi conversation credentials"),
        Err(e) => tracing::warn!(error = %e, "could not sweep orphaned pi credentials"),
    }
}
