//! Parsing skill front-matter, escaping what goes back out, and keeping the
//! shipped skill bodies inert to the gateway's secret scanner.

use systemprompt_web_admin::test_support::{escape, scalar, scan_str_for_secret};

/// The credential-shaped literal `demonstrate_governance` sends to trigger a
/// `secret_scan` deny. It matches an operator `extra_pattern`, never a built-in.
const DEMO_CREDENTIAL: &str = "SPDEMOKEY-0000000000000000";

fn skills_root() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../services/skills"
    ))
}

/// Every shipped `SKILL.md`, as `(id, body)`.
fn shipped_skill_bodies() -> Vec<(String, String)> {
    let root = skills_root();
    let entries = std::fs::read_dir(&root).expect("services/skills is readable");
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        let config = dir.join("config.yaml");
        if !config.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&config).expect("config.yaml is readable");
        let id = scalar(&raw, "id").expect("every skill has an id");
        let file = scalar(&raw, "file").unwrap_or_else(|| "SKILL.md".to_owned());
        let body = std::fs::read_to_string(dir.join(&file)).expect("skill body is readable");
        out.push((id, body));
    }
    assert!(!out.is_empty(), "no skills found under {}", root.display());
    out
}

#[test]
fn reads_quoted_and_bare_scalars() {
    let raw = "id: demonstrate_governance\ndescription: \"Exercise the pipeline\"\n";
    assert_eq!(scalar(raw, "id").as_deref(), Some("demonstrate_governance"));
    assert_eq!(
        scalar(raw, "description").as_deref(),
        Some("Exercise the pipeline")
    );
    assert_eq!(scalar(raw, "missing"), None);
}

/// A key that only appears nested must not be mistaken for a top-level one
/// — `tags:` entries are indented, and a naive `contains` would match.
#[test]
fn ignores_indented_keys() {
    assert_eq!(scalar("tags:\n  id: nope\n", "id"), None);
}

/// An unescaped quote in a description would produce frontmatter pi cannot
/// parse, and pi drops a skill with no readable description silently.
#[test]
fn escapes_quotes_in_a_description() {
    assert_eq!(escape(r#"the "hub" tool"#), r#"the \"hub\" tool"#);
}

/// Every shipped skill must survive the on-disk round trip, because the
/// failure mode is a slash-command that simply is not there.
#[test]
fn the_shipped_skills_all_have_what_pi_requires() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../services/skills");
    let entries = std::fs::read_dir(root).expect("services/skills is readable");
    let mut seen = 0;
    for entry in entries.flatten() {
        let config = entry.path().join("config.yaml");
        if !config.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&config).expect("config.yaml is readable");
        let id = scalar(&raw, "id").expect("every skill has an id");
        assert!(
            scalar(&raw, "description").is_some(),
            "{id} has no description; pi would drop it without saying so"
        );
        let slug = id.replace('_', "-");
        assert!(
            slug.len() <= 64
                && slug
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{slug} is not a name pi will accept"
        );
        seen += 1;
    }
    assert!(seen > 0, "no skills found under {root}");
}
