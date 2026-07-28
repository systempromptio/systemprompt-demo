//! `bundle_public_css` job: one stylesheet per public page family.
//!
//! Public pages linked 30-40 individual sheets each. This concatenates them
//! into `css/bundles/<family>-bundle.css` in the declared cascade order and
//! records a content hash per family in `css/css-manifest.json`. Templates link
//! the stable bundle name and cache-bust with `?v={{CSS_BUNDLE_VERSION}}`,
//! which the navigation page-data provider reads back out of that manifest.
//!
//! Must run before `copy_extension_assets`, which ships the bundles to
//! `web/dist/`.

mod families;

use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use systemprompt::models::AppPaths;
use systemprompt::traits::{Job, JobContext, JobResult};

use self::families::{FAMILIES, Family};
use crate::error::JobError;

pub const BUNDLE_DIR: &str = "bundles";
pub const MANIFEST_NAME: &str = "css-manifest.json";

#[derive(Debug, Clone, Copy, Default)]
pub struct BundlePublicCssJob;

impl BundlePublicCssJob {
    pub async fn execute_bundle(paths: &AppPaths) -> Result<JobResult, JobError> {
        let start_time = std::time::Instant::now();

        tracing::info!("Bundle public CSS job started");

        let css_dir = paths.storage().files().join("css");
        let bundle_dir = css_dir.join(BUNDLE_DIR);
        tokio::fs::create_dir_all(&bundle_dir).await?;

        let mut entries = serde_json::Map::new();
        let mut site_hasher = Sha256::new();
        let mut bundled = 0u64;

        for family in FAMILIES {
            let bundle = concatenate(&css_dir, family).await?;
            let hash = short_hash(bundle.as_bytes());
            site_hasher.update(bundle.as_bytes());

            let filename = format!("{}-bundle.css", family.name);
            tokio::fs::write(bundle_dir.join(&filename), &bundle).await?;

            entries.insert(
                family.name.to_owned(),
                serde_json::json!({ "file": format!("{BUNDLE_DIR}/{filename}"), "hash": hash }),
            );
            bundled += 1;

            tracing::debug!(
                family = family.name,
                sources = family.files.len(),
                bytes = bundle.len(),
                "Public CSS family bundled"
            );
        }

        let version = hex12(&site_hasher.finalize());
        write_manifest(&css_dir, version, entries).await?;

        let duration_ms = u64::try_from(start_time.elapsed().as_millis()).unwrap_or(u64::MAX);

        tracing::info!(bundled, duration_ms, "Bundle public CSS job completed");

        Ok(JobResult::success()
            .with_stats(bundled, 0)
            .with_duration(duration_ms))
    }
}

async fn concatenate(css_dir: &Path, family: &Family) -> Result<String, JobError> {
    let mut bundle = String::new();

    for name in family.files {
        let path = css_dir.join(name);
        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            JobError::other(format!(
                "Public CSS family '{}' lists {name}, which could not be read: {e}",
                family.name
            ))
        })?;
        if !bundle.is_empty() {
            bundle.push('\n');
        }
        bundle.push_str(&content);
    }

    Ok(bundle)
}

async fn write_manifest(
    css_dir: &Path,
    version: String,
    bundles: serde_json::Map<String, serde_json::Value>,
) -> Result<(), JobError> {
    let manifest = serde_json::json!({ "version": version, "bundles": bundles });
    let body = serde_json::to_string_pretty(&manifest)
        .map_err(|e| JobError::other(format!("CSS manifest serialization failed: {e}")))?;
    tokio::fs::write(css_dir.join(MANIFEST_NAME), body).await?;
    Ok(())
}

fn short_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex12(&hasher.finalize())
}

fn hex12(digest: &[u8]) -> String {
    digest
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

#[async_trait::async_trait]
impl Job for BundlePublicCssJob {
    fn name(&self) -> &'static str {
        "bundle_public_css"
    }

    fn description(&self) -> &'static str {
        "Concatenates public-site CSS into one bundle per page family"
    }

    fn schedule(&self) -> &'static str {
        "0 */15 * * * *"
    }

    async fn execute(
        &self,
        ctx: &JobContext,
    ) -> Result<JobResult, systemprompt::traits::ProviderError> {
        let paths = ctx
            .app_paths::<Arc<AppPaths>>()
            .ok_or_else(|| JobError::other("AppPaths unavailable in job context"))?;
        Ok(Self::execute_bundle(paths).await?)
    }
}

systemprompt::traits::submit_job!(&BundlePublicCssJob);
