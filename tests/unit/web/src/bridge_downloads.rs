//! Every bridge download URL must point at the same release tag.
//!
//! A URL that lost its `bridge-v*` tag points at the gateway release and 404s.

use systemprompt_web_admin::test_support::{LINUX, MAC_ARM, MAC_INTEL, RELEASE_PAGE, WINDOWS};

/// A URL that lost its `bridge-v*` tag points at the gateway release and
/// 404s.
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
    assert!(
        tags.windows(2).all(|w| w[0] == w[1]),
        "mixed tags: {tags:?}"
    );
}
