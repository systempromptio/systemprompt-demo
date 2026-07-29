//! Bootstrap-time loading of the `services/web/` YAML tree.
//!
//! Runs once at extension construction, before any request is served, so the
//! file-system reads here are not on a hot path.

use std::sync::{Arc, OnceLock};

use systemprompt::config::ProfileBootstrap;
use systemprompt::models::AppPaths;
use thiserror::Error;

static BRANDING_CONFIG: OnceLock<Result<Option<BrandingConfig>, String>> = OnceLock::new();

fn load_app_paths() -> Result<AppPaths, ConfigError> {
    let profile =
        ProfileBootstrap::get().map_err(|e| ConfigError::PathsUnavailable(e.to_string()))?;
    AppPaths::from_profile(&profile.paths).map_err(|e| ConfigError::PathsUnavailable(e.to_string()))
}

use crate::navigation::{BrandingConfig, NavigationConfig};

#[derive(Debug, Clone, Error)]
pub(crate) enum ConfigError {
    #[error("Failed to parse {config_name}: {message}")]
    Parse {
        config_name: String,
        message: String,
    },

    #[error("Application paths unavailable: {0}")]
    PathsUnavailable(String),
}

pub(crate) fn load_navigation_config() -> Result<Option<Arc<NavigationConfig>>, ConfigError> {
    let Some(nav_value) = load_config_section("navigation.yaml")? else {
        return Ok(None);
    };

    let nav_config: NavigationConfig =
        serde_yaml::from_value(nav_value).map_err(|e| ConfigError::Parse {
            config_name: "navigation.yaml".to_owned(),
            message: e.to_string(),
        })?;

    tracing::info!("Loaded navigation config from config/navigation.yaml");

    Ok(Some(Arc::new(nav_config)))
}

pub(crate) fn load_branding_config() -> Result<Option<BrandingConfig>, ConfigError> {
    let Some(theme_value) = load_config_section("theme.yaml")? else {
        return Ok(None);
    };

    let Some(branding_value) = theme_value.get("branding") else {
        return Ok(None);
    };

    let branding_config: BrandingConfig =
        serde_yaml::from_value(branding_value.clone()).map_err(|e| ConfigError::Parse {
            config_name: "theme.yaml (branding section)".to_owned(),
            message: e.to_string(),
        })?;

    tracing::info!("Loaded branding config from config/theme.yaml");

    Ok(Some(branding_config))
}
pub fn branding_config() -> Option<BrandingConfig> {
    crate::extension::log_and_discard_err(
        &BRANDING_CONFIG,
        load_branding_config,
        "Branding config error",
    )
}

fn load_config_section(filename: &str) -> Result<Option<serde_yaml::Value>, ConfigError> {
    let paths = match load_app_paths() {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!(error = %e, "AppPaths not available for config section");
            return Ok(None);
        },
    };

    let config_path = paths
        .system()
        .services()
        .join(format!("web/config/{filename}"));

    let yaml_content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!(
                path = %config_path.display(),
                "Config file does not exist"
            );
            return Ok(None);
        },
        Err(e) => {
            return Err(ConfigError::Parse {
                config_name: filename.to_owned(),
                message: format!("Failed to read file: {e}"),
            });
        },
    };

    serde_yaml::from_str(&yaml_content)
        .map(Some)
        .map_err(|e| ConfigError::Parse {
            config_name: filename.to_owned(),
            message: e.to_string(),
        })
}
