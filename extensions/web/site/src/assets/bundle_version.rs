//! Cache-busting token for the public CSS bundles.
//!
//! The `bundle_public_css` job writes a content hash of every bundle to
//! `storage/files/css/css-manifest.json`; public templates append it as
//! `?v=…` so a rebuilt bundle is never served from a stale browser cache.

use std::sync::OnceLock;

const MANIFEST: &str = "storage/files/css/css-manifest.json";

pub fn css_bundle_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        // Why: a missing or corrupt manifest degrades to an unbusted cache, not
        // a failed render — prerendering must not depend on the bundle job
        // having run first.
        std::fs::read_to_string(MANIFEST)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|j| j.get("version")?.as_str().map(String::from))
            .unwrap_or_else(|| "0".to_owned())
    })
}
