//! Prerenders the resources page into a static artifact during
//! `publish_pipeline`.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;
use systemprompt::extension::prelude::*;
use systemprompt::models::WebConfig;

/// Template context for the prerendered resources page (`resources.html`): the
/// site-wide web config under `site.*`, consumed for branding and meta tags.
#[derive(Debug, Serialize)]
struct ResourcesContext<'a> {
    site: &'a WebConfig,
}

#[derive(Debug, Clone, Copy)]
pub struct ResourcesPrerenderer;

#[async_trait]
impl PagePrerenderer for ResourcesPrerenderer {
    fn page_type(&self) -> &'static str {
        "resources"
    }

    fn priority(&self) -> u32 {
        50
    }

    async fn prepare(
        &self,
        ctx: &PagePrepareContext<'_>,
    ) -> Result<Option<PageRenderSpec>, systemprompt::traits::ProviderError> {
        let base_data = serde_json::to_value(ResourcesContext {
            site: ctx.web_config,
        })?;

        let output_path = PathBuf::from("resources/index.html");

        Ok(Some(PageRenderSpec::new(
            "resources",
            base_data,
            output_path,
        )))
    }
}
