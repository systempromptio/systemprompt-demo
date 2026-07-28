//! `PiConfig::parse` — the boundary between `services/config/pi.yaml` and a
//! bounded session.
//!
//! Two things are being pinned here. First, that a typo cannot quietly widen
//! the boundary: `deny_unknown_fields` and the `SandboxMode` enum both turn a
//! misspelling into a rejection rather than a default. Second, that the file
//! actually shipped in this repo still passes every check — nothing else
//! exercises it until startup, where a failure means the deployment runs on
//! defaults instead of on what the file says.

use systemprompt_web_pi::test_support::{PiConfig, SandboxMode, VersionCheckMode};

#[test]
fn empty_file_is_all_defaults() {
    let cfg = PiConfig::parse("{}").expect("an empty map is the all-defaults state");
    assert_eq!(cfg.sandbox(), SandboxMode::Required);
    assert!(cfg.approve_all());
    assert!(cfg.tools().iter().any(|t| t == "read"));
    assert!(!cfg.jail_read_paths().is_empty());
}

#[test]
fn sandbox_typo_is_rejected_rather_than_read_as_off() {
    let errors = PiConfig::parse("sandbox: of").expect_err("`of` is not a sandbox mode");
    assert!(
        errors.errors.iter().any(|e| e.field == "_parse"),
        "{errors}"
    );
}

#[test]
fn unknown_key_is_rejected() {
    let errors = PiConfig::parse("aprove_all: true").expect_err("misspelled key is rejected");
    assert!(
        errors
            .errors
            .iter()
            .any(|e| e.message.contains("unknown field")),
        "{errors}"
    );
}

#[test]
fn empty_tool_list_fails_validation() {
    let errors = PiConfig::parse("tools: []\nbase_url: http://127.0.0.1:8080")
        .expect_err("a session with no tools cannot do anything");
    assert!(errors.errors.iter().any(|e| e.field == "tools"));
}

#[test]
fn zero_timeout_fails_validation() {
    let errors = PiConfig::parse("timeouts:\n  idle_secs: 0\nbase_url: http://127.0.0.1:8080")
        .expect_err("zero would expire immediately");
    assert!(
        errors
            .errors
            .iter()
            .any(|e| e.field == "timeouts.idle_secs")
    );
}

/// The shipped file has to satisfy `deny_unknown_fields` and every check in
/// `validate`, or the deployment it configures silently runs on defaults
/// instead. Nothing else exercises it until startup.
#[test]
fn the_checked_in_config_is_valid() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../services/config/pi.yaml"
    );
    let yaml = std::fs::read_to_string(path).expect("services/config/pi.yaml is readable");
    let cfg = PiConfig::parse(&yaml).expect("services/config/pi.yaml validates");
    // The two settings that decide whether this is a demo or an exposure.
    assert_eq!(cfg.sandbox(), SandboxMode::Required);
    assert!(
        !cfg.tools()
            .iter()
            .any(|t| matches!(t.as_str(), "bash" | "write" | "edit"))
    );
    assert!(cfg.approve_all());
    // The version pin is load-bearing security (see version.rs): the shipped
    // file must carry one, and must fail closed on a mismatch.
    assert!(cfg.expected_version().is_some());
    assert_eq!(cfg.version_check(), VersionCheckMode::Required);
}

#[test]
fn version_check_defaults_to_required_with_no_pin() {
    let cfg = PiConfig::parse("base_url: http://127.0.0.1:8080").expect("validates");
    assert_eq!(cfg.expected_version(), None);
    assert_eq!(cfg.version_check(), VersionCheckMode::Required);
}

#[test]
fn a_blank_pin_reads_as_no_pin() {
    let cfg = PiConfig::parse("expected_version: \"  \"\nbase_url: http://127.0.0.1:8080")
        .expect("validates");
    assert_eq!(cfg.expected_version(), None);
}

#[test]
fn version_check_typo_is_rejected() {
    let errors =
        PiConfig::parse("version_check: sometimes").expect_err("`sometimes` is not a mode");
    assert!(errors.errors.iter().any(|e| e.field == "_parse"));
}

#[test]
fn throttle_typo_is_rejected() {
    let errors = PiConfig::parse("throttle:\n  sessions_per_ip: 5")
        .expect_err("misspelled throttle key is rejected");
    assert!(
        errors
            .errors
            .iter()
            .any(|e| e.message.contains("unknown field")),
        "{errors}"
    );
}

#[test]
fn explicit_base_url_needs_no_profile() {
    let cfg = PiConfig::parse("base_url: https://example.test").expect("validates");
    assert_eq!(cfg.base_url(), "https://example.test");
}
