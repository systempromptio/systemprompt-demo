//! The governance spine for the Enterprise Demo template.
//!
//! Everything that decides whether a call is allowed, and everything that
//! records what happened, lives here. It sits between
//! [`systemprompt_web_shared`] and the surfaces that consume it — the admin
//! dashboard and the pi terminal both depend on this crate; it depends on
//! neither.
//!
//! - [`hooks_webhook_router`] — the four webhooks called by gateway / MCP /
//!   Claude Code (`/hooks/track`, `/hooks/govern`, `/govern/authz`, plus
//!   statusline and transcript ingest).
//! - [`webhook::governance::inproc`] — the same four-stage pipeline (scope
//!   check, secret scan, blocklist, rate limit) invoked in-process, which is
//!   how the pi terminal governs its own tool calls without a network hop.
//! - [`authz`] — the attribute dimensions this deployment declares, registered
//!   with the core authorization engine.
//! - [`identity`] / [`device_service`] — who is calling, and the API keys and
//!   device certificates they call with.
//! - [`repositories`] owns every `sqlx` call; handlers never touch the DB
//!   directly. Errors normalise on [`error::GovernanceError`].

pub mod activity;
pub mod audit_event_bus;
pub mod authz;
pub mod device_service;
pub mod error;
pub mod event_hub;
pub mod gateway_safety;
pub mod hooks_track;
pub mod identity;
pub mod numeric;
pub mod repositories;
pub mod types;
pub mod webhook;

use std::sync::Arc;

use axum::routing::post;
use axum::{Extension, Router};
use sqlx::PgPool;

pub use error::{GovernanceError, GovernanceResult};

/// Crate-private items that out-of-crate unit tests drive directly.
///
/// The tests live in `tests/unit/web` rather than beside the code, so anything
/// they touch needs a public name. Re-exporting here keeps that list explicit
/// and reviewable instead of widening each item in place.
pub mod test_support {
    pub use crate::webhook::governance::policies::rate_limit::RateLimit;
    pub use crate::webhook::governance::scope::cap_at;
    pub use crate::webhook::governance::secrets::scan_str_for_secret;
}

pub fn hooks_webhook_router(
    pool: Arc<PgPool>,
    session_service: Arc<systemprompt::oauth::SessionCreationService>,
    analytics_provider: Arc<dyn systemprompt::traits::AnalyticsProvider>,
) -> Router {
    Router::new()
        .route("/hooks/track", post(hooks_track::handle_hook_track))
        .route("/hooks/govern", post(webhook::govern_tool_use))
        .route("/govern/authz", post(webhook::govern_authz))
        .route("/hooks/statusline", post(webhook::track_statusline_event))
        .route("/hooks/transcript", post(webhook::track_transcript_event))
        .layer(Extension(event_hub::EventHub::default()))
        .layer(Extension(None::<Arc<systemprompt::ai::AiService>>))
        .layer(Extension(webhook::GovernanceDeps {
            session_service: Arc::clone(&session_service),
            analytics: analytics_provider,
        }))
        .layer(Extension(session_service))
        .with_state(pool)
}
