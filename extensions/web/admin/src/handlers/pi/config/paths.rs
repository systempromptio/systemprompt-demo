//! Where the config file lives and what the environment supplies when a key
//! is absent.
//!
//! Every function here is total: it reports a problem and returns a usable
//! value rather than failing. That is what keeps [`super::PiConfig::validate`]
//! total, which is in turn what [`super::PiConfig::load_or_defaults`] rests on.

use std::path::PathBuf;

use systemprompt::config::ProfileBootstrap;
use systemprompt::models::AppPaths;

pub(super) fn config_path() -> PathBuf {
    ProfileBootstrap::get()
        .map_err(|e| e.to_string())
        .and_then(|profile| AppPaths::from_profile(&profile.paths).map_err(|e| e.to_string()))
        .map_or_else(
            |_| PathBuf::from("./services/config/pi.yaml"),
            |paths| paths.system().services().join("config/pi.yaml"),
        )
}

pub(super) fn profile_base_url() -> String {
    ProfileBootstrap::get().map_or_else(
        |e| {
            tracing::warn!(error = %e, "No profile loaded; pi sessions will call back on localhost");
            "http://127.0.0.1:8080".to_owned()
        },
        |profile| profile.server.api_internal_url.clone(),
    )
}

pub(super) fn default_jail_binary() -> PathBuf {
    std::env::current_exe()
        .inspect_err(|e| {
            tracing::warn!(error = %e, "could not resolve the current exe; finding sp-pi-jail on PATH");
        })
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("sp-pi-jail")))
        .unwrap_or_else(|| PathBuf::from("sp-pi-jail"))
}
