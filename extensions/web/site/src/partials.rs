//! Shared header, footer, and asset partials for every public-site page.
//!
//! The template bodies are `include_str!`-compiled from
//! `services/web/templates/partials/`, so editing them requires `just build`
//! and a server restart — running `just publish` alone keeps serving the markup
//! baked into the old binary.

use async_trait::async_trait;
use systemprompt::template_provider::{
    ComponentContext, ComponentRenderer, PartialTemplate, RenderedComponent,
};
use systemprompt::traits::ProviderError;

pub(crate) const PRIORITY_CRITICAL: u32 = 5;
pub(crate) const PRIORITY_HIGH: u32 = 10;
pub(crate) const PRIORITY_MID: u32 = 50;
pub(crate) const PRIORITY_LOW: u32 = 90;
pub(crate) const PRIORITY_LAST: u32 = 95;

pub use super::partials_animations::{
    AgenticMeshAnimationPartialRenderer, ArchitectureDiagramPartialRenderer,
    CliRemoteAnimationPartialRenderer, MemoryLoopAnimationPartialRenderer,
    RustMeshAnimationPartialRenderer,
};

/// Generates a zero-state [`ComponentRenderer`] whose only job is to expose an
/// embedded partial template: the render step contributes an empty variable and
/// the engine substitutes the partial body at the declared name.
macro_rules! static_partial {
    ($ty:ident, $id:literal, $var:literal, $partial:literal, $template:literal, $priority:expr) => {
        #[derive(Debug, Clone, Copy)]
        pub struct $ty;

        impl $ty {
            const TEMPLATE: &str = include_str!(concat!(env!("OUT_DIR"), "/partials/", $template));
        }

        #[async_trait]
        impl ComponentRenderer for $ty {
            fn component_id(&self) -> &'static str {
                $id
            }

            fn variable_name(&self) -> &'static str {
                $var
            }

            fn applies_to(&self) -> Vec<String> {
                vec![]
            }

            fn partial_template(&self) -> Option<PartialTemplate> {
                Some(PartialTemplate::embedded($partial, Self::TEMPLATE))
            }

            async fn render(
                &self,
                _ctx: &ComponentContext<'_>,
            ) -> Result<RenderedComponent, ProviderError> {
                Ok(RenderedComponent::new(self.variable_name(), ""))
            }

            fn priority(&self) -> u32 {
                $priority
            }
        }
    };
}

pub(crate) use static_partial;

static_partial!(
    HeadAssetsPartialRenderer,
    "web:head-assets-partial",
    "HEAD_ASSETS",
    "head-assets",
    "head-assets.html",
    PRIORITY_CRITICAL
);

static_partial!(
    HeaderPartialRenderer,
    "web:header-partial",
    "HEADER",
    "header",
    "header.html",
    PRIORITY_HIGH
);

static_partial!(
    FooterPartialRenderer,
    "web:footer-partial",
    "FOOTER",
    "footer",
    "footer.html",
    PRIORITY_LOW
);

static_partial!(
    ScriptsPartialRenderer,
    "web:scripts-partial",
    "SCRIPTS",
    "scripts",
    "scripts.html",
    PRIORITY_LAST
);
