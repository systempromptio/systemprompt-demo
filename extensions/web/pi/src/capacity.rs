//! Live slot occupancy and the wait line, for the header meter.
//!
//! Polling this endpoint IS the waitlist heartbeat: a queued browser calls it
//! with its embed token and `join=1`, which refreshes (or re-creates) its
//! entry; a browser that stops polling falls out of line after
//! [`super::registry`]'s TTL. The unauthenticated shape — `used`, `max`,
//! `queue_len` — is public on purpose, same exposure class as `pulse`: the
//! meter renders for anonymous visitors too.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use super::auth::authenticate;
use super::registry::PiRegistry;

#[derive(Debug, Deserialize)]
pub(super) struct CapacityQuery {
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    join: bool,
}

#[derive(Debug, Serialize)]
struct CapacityOut {
    used: usize,
    max: usize,
    queue_len: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<usize>,
    /// Whether a `POST /session` from this caller would be admitted right
    /// now — the client's cue to retry, so it never has to guess at the
    /// server's FIFO arithmetic.
    admissible: bool,
}

pub(super) async fn capacity(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Query(q): Query<CapacityQuery>,
) -> Response {
    // lint-ok: http-error — always 200; there is no failure path to classify
    let user_id = match q.token.as_deref() {
        Some(token) => authenticate(&pool, token).await,
        None => None,
    };
    let (used, max) = registry.occupancy();
    let (queue_len, position) = registry.waitlist_status(user_id.as_ref(), q.join);
    let free = max.saturating_sub(used);
    let admissible = user_id.is_some() && position.map_or(queue_len < free, |p| p < free);
    (
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(CapacityOut {
            used,
            max,
            queue_len,
            position,
            admissible,
        }),
    )
        .into_response()
}
