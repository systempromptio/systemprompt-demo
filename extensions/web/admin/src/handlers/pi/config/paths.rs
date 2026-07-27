//! Where the config file lives and what the environment supplies when a key
//! is absent.
//!
//! Every function here is total: it reports a problem and returns a usable
//! value rather than failing. That is what keeps [`super::PiConfig::validate`]
//! total, which is in turn what [`super::PiConfig::load_or_defaults`] rests on.

use std::path::PathBuf;

use systemprompt::config::ProfileBootstrap;
use systemprompt::models::AppPaths;

/// `services/config/pi.yaml` under the active profile.
///
/// The profile is loaded once at startup, so a failure here means the process
/// has no profile at all — a condition the server would already have failed
/// on. Falling back to the repo-relative path keeps a `cargo test` run that
/// never bootstrapped a profile working.
pub(super) fn config_path() -> PathBuf {
    ProfileBootstrap::get()
        .map_err(|e| e.to_string())
        .and_then(|profile| AppPaths::from_profile(&profile.paths).map_err(|e| e.to_string()))
        .map_or_else(
            |_| PathBuf::from("./services/config/pi.yaml"),
            |paths| paths.system().services().join("config/pi.yaml"),
        )
}

/// The origin sessions call back on, taken from the profile so it follows the
/// deployment rather than being restated per environment.
///
/// A process with no profile cannot serve the terminal anyway — this keeps
/// `validate` total so the default config is always constructible.
pub(super) fn profile_base_url() -> String {
    ProfileBootstrap::get().map_or_else(
        |e| {
            tracing::warn!(error = %e, "No profile loaded; pi sessions will call back on localhost");
            "http://127.0.0.1:8080".to_owned()
        },
        |profile| profile.server.api_internal_url.clone(),
    )
}

/// `sp-pi-jail` beside our own executable. A failure to resolve `current_exe`
/// leaves a bare name, which `SpawnError` will report as a missing binary —
/// still fail-closed, and with a legible reason.
pub(super) fn default_jail_binary() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("sp-pi-jail")))
        .unwrap_or_else(|| PathBuf::from("sp-pi-jail"))
}
