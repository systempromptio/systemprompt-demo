//! Parsing skill front-matter, and escaping what goes back out.

use systemprompt_web_admin::test_support::{escape, scalar};

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
