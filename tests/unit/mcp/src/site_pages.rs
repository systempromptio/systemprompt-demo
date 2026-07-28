//! Live-site page tools: URL composition is confined to section + slug, and
//! model-facing truncation reports only when it dropped something.

use systemprompt_mcp_agent::test_support::{site_page_url, truncate_for_model};
use systemprompt_mcp_agent::tool_inputs::SitePageSection;

#[test]
fn url_is_composed_from_section_and_slug_only() {
    assert_eq!(
        site_page_url(
            "https://systemprompt.io",
            SitePageSection::Documentation,
            "services/ai"
        )
        .unwrap(),
        "https://systemprompt.io/md/documentation/services/ai.md"
    );
    assert_eq!(
        site_page_url("https://systemprompt.io/", SitePageSection::Blog, "launch").unwrap(),
        "https://systemprompt.io/md/blog/launch.md"
    );
}

#[test]
fn slugs_that_could_steer_the_url_are_rejected() {
    for slug in [
        "",
        "../etc/passwd",
        "a/../b",
        "/absolute",
        "trailing/",
        "double//slash",
        "UPPER",
        "space here",
        "query?x=1",
        "frag#ment",
        "percent%2e%2e",
        "host.com",
        &"a".repeat(201),
    ] {
        assert!(
            site_page_url(
                "https://systemprompt.io",
                SitePageSection::Documentation,
                slug
            )
            .is_err(),
            "slug {slug:?} should be rejected"
        );
    }
}

#[test]
fn truncation_reports_only_when_it_dropped_something() {
    assert_eq!(truncate_for_model("abc", 3), ("abc".to_owned(), false));
    assert_eq!(truncate_for_model("abcd", 3), ("abc".to_owned(), true));
    assert_eq!(truncate_for_model("", 3), (String::new(), false));
    // Char-based, so a multibyte boundary cannot split a code point.
    assert_eq!(
        truncate_for_model("日本語テスト", 3),
        ("日本語".to_owned(), true)
    );
}
