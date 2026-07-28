//! What is left of the server-rendered `/admin` surface.
//!
//! There is no admin console. The interactive site is one page — a governed
//! terminal and the visitor's own telemetry pane; analytics, governance,
//! catalog, and access live in the CLI, which reads the same tables. The
//! repositories behind them also feed `GET /api/public/pi/stats/{id}`.
//!
//! What survives is only what a browser still genuinely needs: the sign-in and
//! registration pages (the passkey ceremony has to be served from somewhere),
//! the holding page for an account under review, and the device endpoints the
//! Bridge calls.

use std::sync::Arc;

use axum::routing::{get, post};
use axum::{Extension, Router, middleware as axum_middleware};
use sqlx::PgPool;
use tower_http::normalize_path::NormalizePathLayer;

use super::super::templates::AdminTemplateEngine;
use super::super::{handlers, middleware};

pub fn admin_ssr_router(pool: Arc<PgPool>, engine: AdminTemplateEngine) -> Router {
    let inner = account_routes()
        .merge(device_routes())
        .merge(api_routes())
        .layer(Extension(engine.clone()))
        .layer(axum_middleware::from_fn(
            middleware::require_approved_middleware,
        ))
        .layer(axum_middleware::from_fn(
            middleware::require_user_middleware,
        ))
        .layer(axum_middleware::from_fn_with_state(
            Arc::clone(&pool),
            middleware::user_context_middleware,
        ))
        .with_state(Arc::clone(&pool));

    let combined = public_routes()
        .layer(Extension(engine))
        .with_state(pool)
        .fallback_service(inner);

    Router::new().fallback_service(
        tower::ServiceBuilder::new()
            .layer(NormalizePathLayer::trim_trailing_slash())
            .service(combined),
    )
}

/// The shareable audit-trail page, mounted at the site root.
///
/// Not under `/admin`, because the whole point of the report is that the
/// operator can hand the link to someone who was never in the session. The
/// conversation id in the path is the capability; see
/// `handlers::ssr::demo_trace_page`.
pub fn trace_ssr_router(pool: Arc<PgPool>, engine: AdminTemplateEngine) -> Router {
    Router::new()
        .route(
            "/trace/{conversation_id}",
            get(handlers::ssr::demo_trace_page),
        )
        .layer(Extension(engine))
        .with_state(pool)
}

fn public_routes() -> Router<Arc<PgPool>> {
    Router::new()
        .route("/login", get(handlers::ssr::login_page))
        .route("/register", get(handlers::ssr::register_page))
        .route("/add-passkey", get(handlers::ssr::add_passkey_page))
        .route("/verify-pending", get(handlers::ssr::verify_pending_page))
        .route(
            "/api/magic-link/request",
            post(handlers::magic_link::request_magic_link),
        )
        .route(
            "/api/magic-link/validate",
            post(handlers::magic_link::validate_magic_link),
        )
        .route(
            "/api/register",
            post(handlers::public_register::public_register_handler),
        )
}

fn account_routes() -> Router<Arc<PgPool>> {
    Router::new()
        .route("/pending", get(handlers::ssr::pending_page))
        .route("/continue", get(handlers::onboarding::post_login_redirect))
}

fn device_routes() -> Router<Arc<PgPool>> {
    Router::new()
        .route("/devices/pats", post(handlers::devices::issue_pat))
        .route(
            "/devices/bridge-code",
            post(handlers::devices::issue_bridge_code),
        )
        .route(
            "/devices/pats/{id}",
            axum::routing::delete(handlers::devices::revoke_pat),
        )
        .route(
            "/devices/certs/{id}",
            axum::routing::delete(handlers::devices::revoke_cert),
        )
}

fn api_routes() -> Router<Arc<PgPool>> {
    Router::new().route("/auth/me", get(middleware::auth_me_handler))
}
