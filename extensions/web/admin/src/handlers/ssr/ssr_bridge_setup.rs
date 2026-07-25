//! SSR page walking a user through bridge installation.

use axum::extract::Extension;
use axum::http::HeaderMap;
use axum::response::Response;
use serde::Serialize;

use crate::error::AdminHtmlResult;
use crate::templates::AdminTemplateEngine;
use crate::types::{MarketplaceContext, UserContext};

use super::ssr_helpers::render_typed_page;

use super::bridge_downloads;

#[derive(Debug, Serialize)]
struct SetupPageData {
    gateway_url: String,
    user_email: String,
    download_mac_arm_url: &'static str,
    download_mac_intel_url: &'static str,
    download_windows_url: &'static str,
    download_linux_url: &'static str,
    release_page_url: &'static str,
}

pub(crate) async fn bridge_setup_page(
    Extension(user_ctx): Extension<UserContext>,
    Extension(mkt_ctx): Extension<MarketplaceContext>,
    Extension(engine): Extension<AdminTemplateEngine>,
    headers: HeaderMap,
) -> AdminHtmlResult<Response> {
    let data = SetupPageData {
        gateway_url: derive_gateway_url(&headers),
        user_email: user_ctx.email.to_string(),
        download_mac_arm_url: bridge_downloads::MAC_ARM,
        download_mac_intel_url: bridge_downloads::MAC_INTEL,
        download_windows_url: bridge_downloads::WINDOWS,
        download_linux_url: bridge_downloads::LINUX,
        release_page_url: bridge_downloads::RELEASE_PAGE,
    };
    Ok(render_typed_page(
        &engine,
        "bridge-setup",
        &data,
        &user_ctx,
        &mkt_ctx,
    ))
}

fn derive_gateway_url(headers: &HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:8080");
    format!("{scheme}://{host}")
}
