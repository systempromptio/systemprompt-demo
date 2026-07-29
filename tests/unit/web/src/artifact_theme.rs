//! The registered artifact theme reaches a rendered artifact.
//!
//! Core styles its artifact renderers entirely through `--mcpui-*` tokens and
//! picks up a deployment's overrides by `inventory`. That linkage is invisible
//! at the call site — nothing references `artifact_theme` — so a dead-code
//! sweep or a lost `#[used]` would silently return every artifact to core's
//! unbranded defaults with no compile error. These tests are the alarm.

use systemprompt::mcp::services::ui_renderer::templates::DashboardRenderer;
use systemprompt::mcp::services::ui_renderer::{UiRenderer, active_theme};
use systemprompt::models::artifacts::dashboard::{
    DashboardArtifact, DashboardSection, MetricCard, MetricsCardsData, SectionType,
};
use systemprompt::models::{A2aArtifact as Artifact, ArtifactMetadata, DataPart, Part};

const BRAND_ACCENT: &str = "--mcpui-accent: oklch(0.72 0.17 52)";
const NOTCHED_CORNER: &str = "--mcpui-radius-card: 1.125rem 0.375rem 1.125rem 1.125rem";

fn spine_dashboard() -> Artifact {
    let metrics = DashboardSection::new(
        "spine-metrics",
        "Session at a glance",
        SectionType::MetricsCards,
    )
    .with_data(MetricsCardsData::new(vec![
        MetricCard::new("Verdicts allowed", "15"),
        MetricCard::new("Verdicts denied", "0"),
    ]))
    .expect("metrics section data is the type it declares");

    let dashboard = DashboardArtifact::new("Governance dashboard").with_sections(vec![metrics]);
    let data = match serde_json::to_value(&dashboard) {
        Ok(serde_json::Value::Object(map)) => map,
        other => panic!("a dashboard serializes to a JSON object, got {other:?}"),
    };

    Artifact {
        id: systemprompt::identifiers::ArtifactId::generate(),
        title: Some("Governance dashboard".to_owned()),
        description: None,
        parts: vec![Part::Data(DataPart { data })],
        extensions: vec![],
        metadata: ArtifactMetadata::new(
            "dashboard".to_owned(),
            systemprompt::identifiers::ContextId::generate(),
            systemprompt::identifiers::TaskId::generate(),
        ),
    }
}

// Why: linking is what registers the theme, and a dependency edge alone does
// not pull an rlib's object files in. Touching `WebExtension` is what the real
// binary does, so it is what these tests must do to observe the same registry.
fn link_web_extension() {
    let _ = systemprompt_web_extension::extension::WebExtension::new();
}

#[test]
fn theme_is_registered() {
    link_web_extension();
    let theme = active_theme().expect("the systemprompt.io artifact theme registers via inventory");
    assert!(theme.tokens.contains(BRAND_ACCENT));
}

#[tokio::test]
async fn rendered_dashboard_carries_the_brand_tokens() {
    link_web_extension();
    let result = DashboardRenderer::new()
        .render(&spine_dashboard())
        .await
        .expect("the demo's own dashboard shape renders");

    assert!(result.html.contains(BRAND_ACCENT));
    assert!(result.html.contains(NOTCHED_CORNER));
    // The metric section renders its cards rather than falling through to an
    // empty box — the failure that made this dashboard look blank.
    assert!(result.html.contains("Verdicts allowed"));
    assert_eq!(result.html.matches("class=\"metric-card").count(), 2);
}

#[tokio::test]
async fn rendered_dashboard_reaches_no_cdn() {
    let renderer = DashboardRenderer::new();
    let result = renderer
        .render(&spine_dashboard())
        .await
        .expect("the demo's own dashboard shape renders");

    assert!(!result.html.contains("jsdelivr"));
    assert!(!result.html.contains("<canvas"));
    assert!(
        !renderer
            .csp_policy()
            .script_src
            .iter()
            .any(|s| s.contains("://"))
    );
}
