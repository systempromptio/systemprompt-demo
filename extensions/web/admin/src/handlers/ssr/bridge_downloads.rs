//! Download URLs for the bridge desktop app, shared by every page that offers
//! it.
//!
//! Bump the `bridge-v*` tag in every URL below on each bridge release. The
//! bridge ships under its own tag alongside the gateway's `v*` tags, so
//! `releases/latest/download/...` is not safe here — it resolves to whichever
//! release published last and 404s.

pub const MAC_ARM: &str = "https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-aarch64-apple-darwin-app.zip";
pub const MAC_INTEL: &str = "https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-x86_64-apple-darwin-app.zip";
pub const WINDOWS: &str = "https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-x86_64-pc-windows-msvc.exe";
pub const LINUX: &str = "https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-x86_64-unknown-linux-gnu";

pub const RELEASE_PAGE: &str =
    "https://github.com/systempromptio/systemprompt-demo/releases/tag/bridge-v0.18.4";
