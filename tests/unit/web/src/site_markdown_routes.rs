//! The page-markdown endpoint's path grammar, driven through `parse_md_path`.
//!
//! This function is the whole input surface of `/md/{*path}`: what it rejects,
//! the site 404s, and the MCP hub's `fetch_site_page` composes URLs of exactly
//! this shape — so this grammar is what makes the terminal's live-site bridge
//! unsteerable.

use systemprompt_web_admin::test_support::parse_md_path;

#[test]
fn known_sections_with_valid_slugs_parse() {
    assert_eq!(
        parse_md_path("documentation/services/ai.md"),
        Some(("documentation", "services/ai"))
    );
    assert_eq!(
        parse_md_path("blog/launch-post.md"),
        Some(("blog", "launch-post"))
    );
    assert_eq!(
        parse_md_path("documentation/a1.md"),
        Some(("documentation", "a1"))
    );
}

#[test]
fn the_md_suffix_is_required() {
    assert_eq!(parse_md_path("documentation/services/ai"), None);
    assert_eq!(parse_md_path("documentation/services/ai.MD"), None);
}

#[test]
fn unknown_sections_are_refused() {
    assert_eq!(parse_md_path("admin/users.md"), None);
    assert_eq!(parse_md_path("features/gateway.md"), None);
    assert_eq!(parse_md_path("Documentation/intro.md"), None);
}

#[test]
fn traversal_and_malformed_slugs_are_refused() {
    for path in [
        "documentation/../secrets.md",
        "documentation/a/../b.md",
        "documentation//double.md",
        "documentation/.md",
        "documentation/UPPER.md",
        "documentation/space here.md",
        "documentation/dot.file.md",
        "blog/.md",
        ".md",
        "documentation.md",
    ] {
        assert_eq!(parse_md_path(path), None, "{path:?} should not parse");
    }
}

#[test]
fn over_length_slugs_are_refused() {
    let long = format!("documentation/{}.md", "a".repeat(201));
    assert_eq!(parse_md_path(&long), None);
    let ok = format!("documentation/{}.md", "a".repeat(200));
    assert!(parse_md_path(&ok).is_some());
}
