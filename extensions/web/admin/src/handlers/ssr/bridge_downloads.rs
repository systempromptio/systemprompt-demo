//! Download URLs for the bridge desktop app, shared by every page that offers it.
//!
//! Bump the `bridge-v*` tag in every URL below on each bridge release. The bridge
//! ships under its own tag alongside the gateway's `v*` tags, so
//! `releases/latest/download/...` is not safe here — it resolves to whichever
//! release published last and 404s.

pub(crate) const MAC_ARM: &str = "https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-aarch64-apple-darwin-app.zip";
pub(crate) const MAC_INTEL: &str = "https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-x86_64-apple-darwin-app.zip";
pub(crate) const WINDOWS: &str = "https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-x86_64-pc-windows-msvc.exe";
pub(crate) const LINUX: &str = "https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-x86_64-unknown-linux-gnu";

/// Release page for the pinned tag — checksums, signatures, every target.
pub(crate) const RELEASE_PAGE: &str =
    "https://github.com/systempromptio/systemprompt-demo/releases/tag/bridge-v0.18.4";

#[cfg(test)]
mod tests {
    use super::*;

    /// A URL that lost its `bridge-v*` tag points at the gateway release and 404s.
    #[test]
    fn every_url_is_pinned_to_one_bridge_tag() {
        let tags: Vec<&str> = [MAC_ARM, MAC_INTEL, WINDOWS, LINUX, RELEASE_PAGE]
            .iter()
            .map(|url| {
                url.split('/')
                    .find(|seg| seg.starts_with("bridge-v"))
                    .unwrap_or_else(|| panic!("{url} is not pinned to a bridge tag"))
            })
            .collect();
        assert!(tags.windows(2).all(|w| w[0] == w[1]), "mixed tags: {tags:?}");
    }
}
