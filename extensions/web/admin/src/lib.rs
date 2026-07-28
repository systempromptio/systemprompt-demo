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
/// list explicit and reviewable instead of widening each item in place.
pub mod test_support {
    pub use systemprompt_web_pi::SHIM_SOURCE;
    pub use systemprompt_web_pi::config::{PiConfig, SandboxMode, VersionCheckMode};
    pub use systemprompt_web_pi::conversations::collapse_duplicate_errors;
    pub use systemprompt_web_pi::events::{
        CREDIT_EXHAUSTED_CODE, CREDIT_EXHAUSTED_NEEDLE, ErrorDeduper, ErrorKind, PiEvent,
        PiEventBody, readable_provider_error, translate, upgrade_legacy_error,
    };
    pub use systemprompt_web_pi::format::{cost, cost_round, median};
    pub use systemprompt_web_pi::jail::gateway_port;
    pub use systemprompt_web_pi::ledger::CallLedger;
    pub use systemprompt_web_pi::mcp::FORWARDABLE;
    pub use systemprompt_web_pi::mcp::render::{McpCallResult, first_frame, render};
    pub use systemprompt_web_pi::normalize::{
        MIN_PEOPLE, bucket, bucket_tokens, window_is_publishable,
    };
    pub use systemprompt_web_pi::persist::Journal;
    pub use systemprompt_web_pi::rpc::{
        GovernancePayload, PayloadKind, RpcCommand, RpcFrame, UiRequest, parse_frame,
    };
    pub use systemprompt_web_pi::scope::escape_reason;
    pub use systemprompt_web_pi::skills::{escape, scalar};
    pub use systemprompt_web_pi::stage::PolicyStage;
    pub use systemprompt_web_pi::token::{B64, Invalid, sign, verify};
    pub use systemprompt_web_pi::transcript::{MAX_CHARS, clamp, section};
    pub use systemprompt_web_pi::version::extract_version;
    pub use crate::handlers::resolve_principal;
    pub use crate::handlers::site_markdown::parse_md_path;
    pub use crate::handlers::ssr::bridge_downloads::{
        LINUX, MAC_ARM, MAC_INTEL, RELEASE_PAGE, WINDOWS,
    };
    pub use systemprompt_web_governance::webhook::governance::policies::rate_limit::RateLimit;
    pub use systemprompt_web_governance::webhook::governance::scope::cap_at;
    pub use systemprompt_web_governance::webhook::governance::secrets::scan_str_for_secret;
    pub use crate::middleware::gates::{is_pending_allowed_path, may_pass_pending_gate};
    pub use systemprompt_web_pi::repositories::events::NewPiEvent;
    pub use crate::util::hmac;
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
