#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
//! systemprompt.io desktop bridge.
//!
//! A thin wrapper over the core systemprompt bridge: it defines the
//! systemprompt.io [`Brand`] (chrome, on-disk paths, env prefix, default
//! gateway, and embedded GUI assets) and hands it to
//! [`systemprompt_bridge::run_with_brand`]. All behaviour lives in the shared
//! core library — this file is intentionally tiny so a white-label bridge is
//! "copy this crate, swap `assets/`, edit the const below". See `README.md`
//! for the recipe.

use std::process::ExitCode;

use systemprompt_bridge::brand::{Brand, BrandAssets};

// Mirrors core's `Brand::SYSTEMPROMPT` (same names, paths, env prefix, and
// scheduler ids) with two deliberate differences: `device_link_path` points at
// this gateway's /bridge-auth mount, and the assets are embedded from OUT_DIR
// with a brand theme.css.
static SYSTEMPROMPT_BRAND: Brand = Brand {
    app_name: "Systemprompt Bridge",
    binary_name: "systemprompt-bridge",
    vendor: "systemprompt.io",
    config_dir: "systemprompt",
    config_file: "systemprompt-bridge.toml",
    pat_file: "systemprompt-bridge.pat",
    working_dir_name: "systemprompt-bridge",
    // User-facing default Cowork workspace folder → ~/Systemprompt, pushed as a
    // pre-trusted allowedWorkspaceFolders entry so the agent has a writable
    // home without folder prompts. Consumed by core's MDM policy writer.
    workspace_dir_name: "Systemprompt",
    keyring_service: "systemprompt-bridge.oauth-client",
    env_prefix: "SP_BRIDGE",
    // Pre-fills the setup/settings gateway field with the local gateway so a
    // dev build talks to a `just start` server out of the box. Overridable at
    // runtime via SP_BRIDGE_GATEWAY_URL or
    // `systemprompt-bridge install --gateway <url>`.
    default_gateway_url: "http://localhost:8080",
    // This gateway mounts the device-link consent page under /bridge-auth
    // (see extensions/web/src/extension_impl.rs nest_service), not the
    // upstream default /bridge — keep these in lockstep.
    device_link_path: "/bridge-auth/device-link",
    tray_tooltip: "Systemprompt Bridge",
    window_title: "Systemprompt Bridge",
    app_menu_name: "Systemprompt Bridge",
    sign_in_label: "Sign in",
    sign_in_hint: "Opens your browser. This device is linked automatically once you approve.",
    // Scheduler identifiers for the periodic sync job.
    schedule_label: "io.systemprompt.bridge-sync",
    schedule_unit: "systemprompt-bridge-sync",
    schedule_task_name: "SystempromptBridgeSync",
    // Embedded from OUT_DIR (copied there by build.rs) rather than directly
    // from `assets/`, so regenerating an asset reliably re-embeds it even
    // under incremental/sccache builds. See build.rs.
    assets: BrandAssets {
        icon_svg: include_str!(concat!(env!("OUT_DIR"), "/icon.svg")),
        logo_svg: include_str!(concat!(env!("OUT_DIR"), "/logo.svg")),
        window_icon_png: include_bytes!(concat!(env!("OUT_DIR"), "/window-icon-1024.png")),
        tray_icon_png: include_bytes!(concat!(env!("OUT_DIR"), "/tray-icon.png")),
        theme_css: include_str!(concat!(env!("OUT_DIR"), "/theme.css")),
    },
};

fn main() -> ExitCode {
    systemprompt_bridge::run_with_brand(&SYSTEMPROMPT_BRAND)
}
