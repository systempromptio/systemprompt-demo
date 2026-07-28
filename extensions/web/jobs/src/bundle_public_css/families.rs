//! Per-page-family stylesheet lists for the public site.
//!
//! Order within each list is the cascade: it mirrors, exactly, the `<link>`
//! order the templates used before bundling. Reordering an entry changes which
//! rule wins, so entries are appended, never sorted.

pub(super) struct Family {
    pub(super) name: &'static str,
    pub(super) files: &'static [&'static str],
}

pub(super) const FAMILIES: &[Family] = &[
    Family {
        name: "core",
        files: CORE,
    },
    Family {
        name: "blog",
        files: BLOG,
    },
    Family {
        name: "blog-list",
        files: BLOG_LIST,
    },
    Family {
        name: "docs",
        files: DOCS,
    },
    Family {
        name: "homepage",
        files: HOMEPAGE,
    },
    Family {
        name: "resources",
        files: RESOURCES,
    },
];

const CORE: &[&str] = &[
    "core/tokens-primitives.css",
    "core/tokens.css",
    "core/page-tokens.css",
    "core/fonts.css",
    "core/reset.css",
    "components/header-core.css",
    "components/header-dropdown.css",
    "components/footer.css",
    "components/mobile-menu.css",
    "components/header-upgrade.css",
];

const BLOG: &[&str] = &[
    "blog/utilities.css",
    "blog/background.css",
    "blog/base.css",
    "blog/post-header.css",
    "blog/social-bar.css",
    "blog/featured-image.css",
    "blog/post-content.css",
    "blog/breadcrumb.css",
    "blog/page-header.css",
    "blog/list-controls.css",
    "blog/cards.css",
    "blog/footer.css",
    "blog/references.css",
    "blog/related.css",
    "blog/banner.css",
    "blog/chat-cta.css",
    "blog/social-content.css",
    "blog/hero.css",
    "blog/homepage.css",
    "blog/platforms.css",
    "blog/ai-badges.css",
    "blog/content-sections.css",
    "blog/content-cards.css",
    "blog/provenance-panel.css",
    "blog/provenance-sections.css",
    "blog/provenance-header.css",
    "blog/workflow.css",
    "blog/provenance-details.css",
    "blog/responsive.css",
    "syntax-highlight.css",
];

const BLOG_LIST: &[&str] = &[
    "blog/utilities.css",
    "blog/background.css",
    "blog/base.css",
    "blog/post-header.css",
    "blog/social-bar.css",
    "blog/featured-image.css",
    "blog/post-content.css",
    "blog/breadcrumb.css",
    "blog/page-header.css",
    "blog/list-controls.css",
    "blog/cards.css",
    "blog/footer.css",
    "blog/references.css",
    "blog/related.css",
    "blog/banner.css",
    "blog/chat-cta.css",
    "blog/social-content.css",
    "blog/hero.css",
    "blog/homepage.css",
    "blog/platforms.css",
    "blog/ai-badges.css",
    "blog/content-sections.css",
    "blog/content-cards.css",
    "blog/provenance-panel.css",
    "blog/provenance-sections.css",
    "blog/provenance-header.css",
    "blog/workflow.css",
    "blog/provenance-details.css",
    "blog/responsive.css",
    "content/cards-base.css",
    "content/cards-categories.css",
    "content/cards-list.css",
];

const DOCS: &[&str] = &[
    "docs/layout.css",
    "docs/header.css",
    "docs/content.css",
    "docs/pagination.css",
    "docs/toc.css",
    "docs/sidebar-links.css",
    "docs/responsive.css",
    "syntax-highlight.css",
];

const HOMEPAGE: &[&str] = &[
    "components/pi-terminal-shell.css",
    "components/pi-terminal-header.css",
    "components/pi-terminal-transcript.css",
    "components/pi-terminal-prose.css",
    "components/pi-terminal-chain.css",
    "components/pi-terminal-approval.css",
    "components/pi-terminal-record.css",
    "components/pi-terminal-composer.css",
    "components/pi-terminal-tools.css",
    "components/pi-terminal-motion.css",
    "components/pi-artifact.css",
    "components/home-split.css",
    "components/auth-pane-core.css",
    "components/auth-pane-offer.css",
    "components/auth-pane-telemetry.css",
    "components/auth-pane-charts.css",
    "components/analytics-pane.css",
    "components/conversation-list.css",
    "components/home-stage.css",
    "components/home-scene-rail.css",
    "components/home-scene-lanes.css",
    "components/home-scene-offer.css",
    "components/home-marquee.css",
    "components/video-modal.css",
];

const RESOURCES: &[&str] = &[
    "homepage/hero.css",
    "homepage/showreel.css",
    "homepage/sections-titles.css",
    "homepage/getting-started.css",
];
