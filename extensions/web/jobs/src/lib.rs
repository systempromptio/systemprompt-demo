//! Background jobs for the web extension.
//!
//! Every job in this crate implements the core `Job` trait and is registered
//! with the scheduler at extension boot. They split into three families:
//!
//! - **Publish pipeline** ([`PublishPipelineJob`]) — runs at server startup and
//!   orchestrates ACL/profile/config bootstrap, asset copy, content ingestion,
//!   prerender, sitemap/robots/llms.txt generation, and secret migration.
//!   Sub-jobs are individually addressable via the CLI for targeted re-runs.
//! - **Build helpers** ([`BundleAdminCssJob`], [`BundlePublicCssJob`],
//!   [`CopyExtensionAssetsJob`], [`ContentPrerenderJob`]) — emit the static
//!   surface under `web/dist/` consumed by the SSR layer.
//! - **Analytics / housekeeping** ([`ContentAnalyticsAggregationJob`],
//!   [`SecretMigrationJob`], [`RetentionJob`]) — periodic rollups, one-shot
//!   migrations, and data retention.
//!
//! Errors normalise on [`JobError`]; the scheduler logs and surfaces them
//! through `infra logs trace`.

mod error;

mod bundle_admin_css;
mod bundle_public_css;
mod content_analytics;
mod copy_assets;
mod governance_bootstrap;
mod ingestion;
mod llms_txt;
mod prerender;
mod publish;
mod retention;
mod robots;
mod secret_migration;
mod sitemap;

pub use error::JobError;

pub use bundle_admin_css::BundleAdminCssJob;
pub use bundle_public_css::BundlePublicCssJob;
pub use content_analytics::ContentAnalyticsAggregationJob;
pub use copy_assets::CopyExtensionAssetsJob;
pub use governance_bootstrap::GovernanceBootstrapJob;
pub use ingestion::ContentIngestionJob;
pub use llms_txt::LlmsTxtGenerationJob;
pub use prerender::ContentPrerenderJob;
pub use publish::PublishPipelineJob;
pub use retention::RetentionJob;
pub use robots::RobotsTxtGenerationJob;
pub use secret_migration::SecretMigrationJob;
pub use sitemap::SitemapGenerationJob;
