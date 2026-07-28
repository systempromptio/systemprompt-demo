//! Admin extension for the Enterprise Demo template.
//!
//! Wires the admin dashboard, governance webhooks, bridge plane, and
//! supporting services onto a shared `PgPool`. Public surface is grouped by
//! concern:
//!
//! - [`admin_router`] — the SSR dashboard (auth-gated; admin-only and
//!   authenticated-read routes are layered together).
//! - [`hooks_webhook_router`] — the four governance webhooks called by gateway
//!   / MCP / Claude Code (`/hooks/track`, `/hooks/govern`, `/govern/authz`,
//!   statusline/transcript ingest).
//! - [`secrets_router`], [`share_manifest_router`] — per-plugin secret
//!   resolution and public manifest sharing.
//!
//! [`repositories`] owns every `sqlx` call; handlers/services never touch
//! the DB directly. Errors normalise on `error::MarketplaceError` via the
//! `MarketplaceError` re-export in [`systemprompt_web_shared`].

pub mod activity;
pub mod audit_event_bus;
pub mod authz;
pub mod error;
pub mod event_hub;
pub mod gateway_safety;
pub(crate) mod handlers;
pub mod marketplace_filter;
pub(crate) mod middleware;
pub mod numeric;
pub mod repositories;
mod routes;
pub(crate) mod services;
pub mod templates;
pub mod types;
pub mod util;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::{Extension, Router, middleware as axum_middleware};
use sqlx::PgPool;

pub use routes::{admin_ssr_router, bridge_auth_ssr_router, trace_ssr_router};
pub use types::{CreateUserRequest, MarketplaceContext, UserContext, UserSummary, UserUsageEvent};

/// Crate-private items that out-of-crate unit tests drive directly.
///
/// The tests for these live in `tests/unit/web` rather than beside the code,
/// so anything they touch needs a public name. Re-exporting here keeps that
/// list explicit and reviewable instead of widening each item in place.
pub mod test_support {
    pub use crate::handlers::pi::SHIM_SOURCE;
    pub use crate::handlers::pi::config::{PiConfig, SandboxMode, VersionCheckMode};
    pub use crate::handlers::pi::conversations::collapse_duplicate_errors;
    pub use crate::handlers::pi::events::{
        CREDIT_EXHAUSTED_CODE, CREDIT_EXHAUSTED_NEEDLE, ErrorDeduper, ErrorKind, PiEvent,
        PiEventBody, readable_provider_error, translate, upgrade_legacy_error,
    };
    pub use crate::handlers::pi::format::{cost, cost_round, median};
    pub use crate::handlers::pi::jail::gateway_port;
    pub use crate::handlers::pi::ledger::CallLedger;
    pub use crate::handlers::pi::mcp::FORWARDABLE;
    pub use crate::handlers::pi::mcp::render::{McpCallResult, first_frame, render};
    pub use crate::handlers::pi::normalize::{
        MIN_PEOPLE, bucket, bucket_tokens, window_is_publishable,
    };
    pub use crate::handlers::pi::persist::Journal;
    pub use crate::handlers::pi::rpc::{
        GovernancePayload, PayloadKind, RpcCommand, RpcFrame, UiRequest, parse_frame,
    };
    pub use crate::handlers::pi::scope::escape_reason;
    pub use crate::handlers::pi::skills::{escape, scalar};
    pub use crate::handlers::pi::stage::PolicyStage;
    pub use crate::handlers::pi::token::{B64, Invalid, sign, verify};
    pub use crate::handlers::pi::transcript::{MAX_CHARS, clamp, section};
    pub use crate::handlers::pi::version::extract_version;
    pub use crate::handlers::resolve_principal;
    pub use crate::handlers::site_markdown::parse_md_path;
    pub use crate::handlers::ssr::bridge_downloads::{
        LINUX, MAC_ARM, MAC_INTEL, RELEASE_PAGE, WINDOWS,
    };
    pub use crate::handlers::webhook::governance::policies::rate_limit::RateLimit;
    pub use crate::handlers::webhook::governance::scope::cap_at;
    pub use crate::handlers::webhook::governance::secrets::scan_str_for_secret;
    pub use crate::middleware::gates::{is_pending_allowed_path, may_pass_pending_gate};
    pub use crate::repositories::pi::events::NewPiEvent;
    pub use crate::util::hmac;
}

pub fn hooks_webhook_router(
    pool: Arc<PgPool>,
    session_service: Arc<systemprompt::oauth::SessionCreationService>,
    analytics_provider: Arc<dyn systemprompt::traits::AnalyticsProvider>,
) -> Router {
    Router::new()
        .route(
            "/hooks/track",
            post(handlers::hooks_track::handle_hook_track),
        )
        .route("/hooks/govern", post(handlers::govern_tool_use))
        .route("/govern/authz", post(handlers::govern_authz))
        .route("/hooks/statusline", post(handlers::track_statusline_event))
        .route("/hooks/transcript", post(handlers::track_transcript_event))
        .layer(Extension(event_hub::EventHub::default()))
        .layer(Extension(None::<Arc<systemprompt::ai::AiService>>))
        .layer(Extension(handlers::GovernanceDeps {
            session_service: Arc::clone(&session_service),
            analytics: analytics_provider,
        }))
        .layer(Extension(session_service))
        .with_state(pool)
}

/// Routes for the governed pi web terminal.
///
/// Always mounted — the terminal is the site's primary demo, so there is
/// nothing to opt into. `services/config/pi.yaml` bounds a session rather than
/// deciding whether one exists, and a broken one is reported at ERROR and
/// replaced by the shipped defaults rather than taking the surface away. See
/// `handlers::pi` for the sandboxing posture — the tool set is read-only
/// unless deliberately widened.
pub fn pi_terminal_router(
    pool: Arc<PgPool>,
    session_service: Arc<systemprompt::oauth::SessionCreationService>,
    analytics_provider: Arc<dyn systemprompt::traits::AnalyticsProvider>,
) -> Router {
    let cfg = handlers::pi::PiConfig::load_or_defaults();
    tracing::info!(model = %cfg.model_name(), "pi web terminal mounted");
    let registry =
        handlers::pi::PiRegistry::new(cfg, Arc::clone(&pool), Arc::clone(&analytics_provider));
    handlers::pi::pi_router(pool, registry, session_service, analytics_provider)
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
