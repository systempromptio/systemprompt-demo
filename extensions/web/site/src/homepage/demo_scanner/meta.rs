//! Deserialisation model for `services/web/config/demo-scanner.yaml` — the
//! pillar and category copy the scanner joins on-disk demos against.
//!
//! Unknown keys are rejected so a typo in the YAML surfaces at load time
//! rather than as silently missing copy.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoScannerMeta {
    pub title: String,
    pub subtitle: String,
    pub pillars: Vec<PillarMeta>,
    pub categories: Vec<CategoryMeta>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryMeta {
    pub id: String,
    pub title: String,
    pub tagline: String,
    pub story: String,
    #[serde(default)]
    pub cost: String,
    #[serde(default)]
    pub feature_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PillarMeta {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub feature_url: String,
    pub category_ids: Vec<String>,
}
