//! The version-pin handshake's parsing half.
//!
//! The comparison against `expected_version` is string equality; what varies
//! in the wild is the banner around it, so `extract_version` is what gets
//! pinned: a bare number, a `v` prefix, and a wordy banner all resolve to the
//! same token, and output with no version in it resolves to none.

use systemprompt_web_pi::test_support::extract_version;

#[test]
fn a_bare_version_is_itself() {
    assert_eq!(extract_version("0.82.0\n"), Some("0.82.0"));
}

#[test]
fn a_v_prefix_is_stripped() {
    assert_eq!(extract_version("v0.82.0"), Some("0.82.0"));
}

#[test]
fn a_banner_yields_its_first_versionish_token() {
    assert_eq!(extract_version("pi 0.82.0 (node 22.19.0)"), Some("0.82.0"));
}

#[test]
fn output_without_a_version_is_none() {
    assert_eq!(extract_version("command not found"), None);
    assert_eq!(extract_version(""), None);
}
