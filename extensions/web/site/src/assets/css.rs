//! Stylesheet definitions, grouped by the page family that consumes them.

use std::path::Path;
use systemprompt::extension::AssetDefinition;

macro_rules! css {
    ($p:expr, $name:literal) => {
        AssetDefinition::css($p.join($name), concat!("css/", $name))
    };
}

pub(super) fn css_assets(storage_css: &Path) -> Vec<AssetDefinition> {
    let mut v = bundle_css(storage_css);
    v.extend(core_css(storage_css));
    v.extend(homepage_css(storage_css));
    v.extend(blog_css(storage_css));
    v.extend(docs_css(storage_css));
    v.extend(content_cards_css(storage_css));
    v.extend(syntax_css(storage_css));
    v
}

// Why: emitted by the bundle_public_css job, which runs immediately before the
// asset copy — the sources stay registered because the bundles are built from
// them on every publish, not checked in.
fn bundle_css(p: &Path) -> Vec<AssetDefinition> {
    vec![
        css!(p, "bundles/core-bundle.css"),
        css!(p, "bundles/blog-bundle.css"),
        css!(p, "bundles/blog-list-bundle.css"),
        css!(p, "bundles/docs-bundle.css"),
        css!(p, "bundles/homepage-bundle.css"),
        css!(p, "bundles/resources-bundle.css"),
    ]
}

fn core_css(p: &Path) -> Vec<AssetDefinition> {
    vec![
        css!(p, "core/tokens-primitives.css"),
        css!(p, "core/tokens.css"),
        css!(p, "core/page-tokens.css"),
        css!(p, "core/fonts.css"),
        css!(p, "core/reset.css"),
        css!(p, "components/header-core.css"),
        css!(p, "components/header-dropdown.css"),
        css!(p, "components/footer.css"),
        css!(p, "components/mobile-menu.css"),
        css!(p, "components/header-upgrade.css"),
    ]
}

fn homepage_css(p: &Path) -> Vec<AssetDefinition> {
    vec![
        css!(p, "homepage/hero.css"),
        css!(p, "homepage/showreel.css"),
        css!(p, "homepage/getting-started.css"),
        css!(p, "homepage/sections-titles.css"),
        css!(p, "components/pi-terminal-shell.css"),
        css!(p, "components/pi-terminal-header.css"),
        css!(p, "components/pi-terminal-capacity.css"),
        css!(p, "components/pi-terminal-transcript.css"),
        css!(p, "components/pi-terminal-prose.css"),
        css!(p, "components/pi-terminal-chain.css"),
        css!(p, "components/pi-terminal-approval.css"),
        css!(p, "components/pi-terminal-record.css"),
        css!(p, "components/pi-terminal-composer.css"),
        css!(p, "components/pi-terminal-tools.css"),
        css!(p, "components/pi-terminal-motion.css"),
        css!(p, "components/pi-artifact.css"),
        css!(p, "components/home-split.css"),
        css!(p, "components/home-stage.css"),
        css!(p, "components/home-scene-rail.css"),
        css!(p, "components/home-scene-lanes.css"),
        css!(p, "components/home-scene-offer.css"),
        css!(p, "components/home-marquee.css"),
        css!(p, "components/video-modal.css"),
        css!(p, "components/auth-pane-core.css"),
        css!(p, "components/auth-pane-offer.css"),
        css!(p, "components/auth-pane-telemetry.css"),
        css!(p, "components/auth-pane-charts.css"),
        css!(p, "components/analytics-pane.css"),
        css!(p, "components/conversation-list.css"),
    ]
}

fn blog_css(p: &Path) -> Vec<AssetDefinition> {
    vec![
        css!(p, "blog/utilities.css"),
        css!(p, "blog/background.css"),
        css!(p, "blog/base.css"),
        css!(p, "blog/post-header.css"),
        css!(p, "blog/social-bar.css"),
        css!(p, "blog/featured-image.css"),
        css!(p, "blog/post-content.css"),
        css!(p, "blog/breadcrumb.css"),
        css!(p, "blog/page-header.css"),
        css!(p, "blog/list-controls.css"),
        css!(p, "blog/cards.css"),
        css!(p, "blog/footer.css"),
        css!(p, "blog/references.css"),
        css!(p, "blog/related.css"),
        css!(p, "blog/banner.css"),
        css!(p, "blog/chat-cta.css"),
        css!(p, "blog/social-content.css"),
        css!(p, "blog/hero.css"),
        css!(p, "blog/homepage.css"),
        css!(p, "blog/platforms.css"),
        css!(p, "blog/ai-badges.css"),
        css!(p, "blog/content-sections.css"),
        css!(p, "blog/content-cards.css"),
        css!(p, "blog/provenance-panel.css"),
        css!(p, "blog/provenance-sections.css"),
        css!(p, "blog/provenance-header.css"),
        css!(p, "blog/workflow.css"),
        css!(p, "blog/provenance-details.css"),
        css!(p, "blog/responsive.css"),
    ]
}

fn docs_css(p: &Path) -> Vec<AssetDefinition> {
    vec![
        css!(p, "docs/layout.css"),
        css!(p, "docs/header.css"),
        css!(p, "docs/content.css"),
        css!(p, "docs/pagination.css"),
        css!(p, "docs/toc.css"),
        css!(p, "docs/responsive.css"),
        css!(p, "docs/sidebar-links.css"),
    ]
}

fn syntax_css(p: &Path) -> Vec<AssetDefinition> {
    vec![css!(p, "syntax-highlight.css")]
}

fn content_cards_css(p: &Path) -> Vec<AssetDefinition> {
    vec![
        css!(p, "content/cards-base.css"),
        css!(p, "content/cards-categories.css"),
        css!(p, "content/cards-list.css"),
    ]
}
