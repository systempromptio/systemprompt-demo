//! SSR setup page: the final funnel step. Download the bridge, get a sign-in
//! code (or PAT), and device-link Claude Desktop / Cowork.

use crate::error::AdminHtmlResult;
use crate::templates::AdminTemplateEngine;
use crate::types::{MarketplaceContext, UserContext};
use axum::extract::{Extension, Query};
use axum::http::HeaderMap;
use axum::response::Response;
use serde::{Deserialize, Serialize};

const DOWNLOAD_MAC_URL: &str = "https://github.com/systempromptio/systemprompt-demo/releases/latest/download/systemprompt-bridge-aarch64-apple-darwin";
const DOWNLOAD_WINDOWS_URL: &str = "https://github.com/systempromptio/systemprompt-demo/releases/latest/download/systemprompt-bridge-x86_64-pc-windows-msvc.exe";
const RELEASES_URL: &str = "https://github.com/systempromptio/systemprompt-demo/releases/latest";

#[derive(Debug, Serialize)]
struct SetupPageContext {
    page: &'static str,
    title: &'static str,
    user_email: String,
    gateway_url: String,
    download_mac_url: &'static str,
    download_windows_url: &'static str,
    releases_url: &'static str,
    welcome: bool,
}

#[derive(Deserialize, Debug)]
pub(crate) struct SetupQuery {
    #[serde(default)]
    welcome: Option<String>,
}

pub(crate) async fn setup_page(
    Extension(user_ctx): Extension<UserContext>,
    Extension(mkt_ctx): Extension<MarketplaceContext>,
    Extension(engine): Extension<AdminTemplateEngine>,
    headers: HeaderMap,
    Query(query): Query<SetupQuery>,
) -> AdminHtmlResult<Response> {
    let ctx = SetupPageContext {
        page: "setup",
        title: "Connect Claude",
        user_email: user_ctx.email.to_string(),
        gateway_url: derive_gateway_url(&headers),
        download_mac_url: DOWNLOAD_MAC_URL,
        download_windows_url: DOWNLOAD_WINDOWS_URL,
        releases_url: RELEASES_URL,
        welcome: query.welcome.is_some(),
    };

    Ok(super::render_typed_page(
        &engine, "setup", &ctx, &user_ctx, &mkt_ctx,
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
