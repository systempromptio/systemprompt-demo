//! Admin extension for the Enterprise Demo template.
//!
//! Wires the admin dashboard, governance webhooks, bridge plane, and
//! supporting services onto a shared `PgPool`. Public surface is grouped by
//! concern:
//!
//! - [`admin_router`] — the SSR dashboard (auth-gated; admin-only and
//!   authenticated-read routes are layered together).
//! - [`hooks_webhook_router`] — re-exported from
//!   [`systemprompt_web_governance`], which owns the governance webhooks.
//! - [`secrets_router`], [`share_manifest_router`] — per-plugin secret
//!   resolution and public manifest sharing.
//!
//! [`repositories`] owns every `sqlx` call; handlers/services never touch
//! the DB directly. Errors normalise on `error::WebError` via the
//! `WebError` re-export in [`systemprompt_web_shared`].

pub mod error;
pub(crate) mod handlers;
pub mod marketplace_filter;
pub(crate) mod middleware;
pub mod repositories;
mod routes;
pub(crate) mod services;
pub mod templates;
pub mod types;
pub mod util;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::{Router, middleware as axum_middleware};
use sqlx::PgPool;

pub use systemprompt_web_governance::{
    activity, audit_event_bus, authz, event_hub, gateway_safety, hooks_webhook_router, numeric,
};

pub use routes::{admin_ssr_router, bridge_auth_ssr_router, trace_ssr_router};
pub use types::{CreateUserRequest, MarketplaceContext, UserContext, UserSummary, UserUsageEvent};

/// Crate-private items that out-of-crate unit tests drive directly.
///
/// The tests for these live in `tests/unit/web` rather than beside the code,
/// so anything they touch needs a public name. Re-exporting here keeps that
/// list explicit and reviewable instead of widening each item in place. Only
/// admin-owned items belong here — the governance and pi crates are depended
/// on directly by the test crate.
pub mod test_support {
    pub use crate::handlers::resolve_principal;
    pub use crate::handlers::site_markdown::parse_md_path;
    pub use crate::handlers::ssr::bridge_downloads::{
        LINUX, MAC_ARM, MAC_INTEL, RELEASE_PAGE, WINDOWS,
    };
    pub use crate::middleware::gates::{is_pending_allowed_path, may_pass_pending_gate};
}



/// The public page-markdown surface: `/index.md` and `/md/{section}/{slug}.md`.
///
/// Serves the site's `markdown_content` rows as raw markdown for agents — the
/// site half of the terminal's live-content bridge (see
/// `handlers::site_markdown`). Unauthenticated on purpose: it exposes nothing
/// the prerendered HTML pages don't already publish.
pub fn site_markdown_router(pool: Arc<PgPool>) -> Router {
    Router::new()
        .route("/index.md", get(handlers::site_markdown::index_handler))
        .route("/md/{*path}", get(handlers::site_markdown::page_handler))
        .with_state(pool)
}

pub fn share_manifest_router(pool: Arc<PgPool>) -> Router {
    Router::new()
        .route(
            "/share/manifest/{token}",
            get(handlers::share::public_manifest_handler),
        )
        .with_state(pool)
}

pub fn secrets_router(pool: Arc<PgPool>) -> Router {
    Router::new()
        .route(
            "/api/v1/secrets/{plugin_id}/token",
            post(handlers::secrets::create_resolution_token_handler),
        )
        .route(
            "/api/v1/secrets/{plugin_id}/resolve",
            get(handlers::secrets::resolve_secrets_handler),
        )
        .route(
            "/admin/api/secrets/{plugin_id}/audit",
            get(handlers::secrets::audit_log_handler),
        )
        .route(
            "/admin/api/secrets/{plugin_id}/rotate",
            post(handlers::secrets::rotate_handler),
        )
        .with_state(pool)
}

pub fn admin_router(read_pool: Arc<PgPool>) -> Router {
    let admin_only = routes::build_admin_only_routes(&read_pool, &read_pool);
    let auth_reads = routes::build_auth_read_routes(&read_pool);

    admin_only
        .merge(auth_reads)
        .layer(axum_middleware::from_fn(
            middleware::require_auth_middleware,
        ))
        .layer(axum_middleware::from_fn_with_state(
            read_pool,
            middleware::user_context_middleware,
        ))
}
