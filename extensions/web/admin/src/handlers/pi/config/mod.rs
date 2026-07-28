//! Runtime configuration for the pi web terminal.
//!
//! Read from `services/config/pi.yaml` once at router construction, resolved
//! through the active profile's [`AppPaths`] like every other configured
//! surface in this repo. There is no enable flag: the terminal is the site's
//! primary demo, so "configured" and "present" are the same state. What the
//! file controls is how a session is *bounded* — model, tool set, sandbox,
//! timeouts, ceilings.
//!
//! Raw deserialisation and validation are separate types, following
//! [`systemprompt_web_shared::config::BlogConfigValidated`]: a [`PiConfig`] can
//! only be produced by `validate`, so nothing downstream handles a
//! half-checked config. A missing file is the all-defaults state and is fine;
//! a file that exists but does not parse or validate is reported at ERROR and
//! replaced by those same defaults — see [`PiConfig::load_or_defaults`] for why
//! that is the fail-closed direction here.
mod paths;
mod raw;

use std::path::PathBuf;
use std::time::Duration;

use systemprompt_web_shared::config_errors::ExtensionConfigErrors;

use paths::{config_path, default_jail_binary, profile_base_url};
use raw::{DEFAULT_JAIL_READ_PATHS, PiConfigRaw};
pub use raw::{SandboxMode, VersionCheckMode};


#[derive(Debug, Clone)]
pub struct PiConfig {
    pub(super) binary: String,
    pub(super) expected_version: Option<String>,
    pub(super) version_check: VersionCheckMode,
    pub(super) workspace_root: PathBuf,
    pub(super) base_url: String,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) models: Vec<String>,
    pub(super) persona: String,
    pub(super) tools: Vec<String>,
    pub(super) child_path: String,
    pub(super) approval_timeout: Duration,
    pub(super) approve_all: bool,
    pub(super) idle_timeout: Duration,
    pub(super) max_lifetime: Duration,
    pub(super) max_sessions_per_user: usize,
    pub(super) max_sessions_total: usize,
    pub(super) throttle_session_per_ip: usize,
    pub(super) throttle_embed_token_per_ip: usize,
    pub(super) throttle_window: Duration,
    pub(super) limits: ChildLimits,
    pub(super) sandbox: SandboxMode,
    pub(super) jail_binary: PathBuf,
    pub(super) jail_read_paths: Vec<PathBuf>,
    pub(super) mcp_url: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ChildLimits {
    pub(super) nproc: u64,
    pub(super) fsize: u64,
    pub(super) address_space: u64,
}

impl PiConfig {
    pub(crate) fn load() -> Result<Self, ExtensionConfigErrors> {
        let path = config_path();
        if !path.exists() {
            return Self::validate(PiConfigRaw::default());
        }
        let content = std::fs::read_to_string(&path).map_err(|e| {
            let mut errors = ExtensionConfigErrors::new("pi");
            errors.push_with_path("_file", format!("Failed to read config file: {e}"), &path);
            errors
        })?;
        Self::parse(&content)
    }

    pub fn parse(yaml: &str) -> Result<Self, ExtensionConfigErrors> {
        let raw: PiConfigRaw = serde_yaml::from_str(yaml).map_err(|e| {
            let mut errors = ExtensionConfigErrors::new("pi");
            errors.push("_parse", format!("Failed to parse config YAML: {e}"));
            errors
        })?;
        Self::validate(raw)
    }

    pub(crate) fn load_or_defaults() -> Self {
        Self::load().unwrap_or_else(|errors| {
            tracing::error!(
                errors = %errors,
                "falling back to built-in pi defaults; the terminal is serving, but not with \
                 the settings in services/config/pi.yaml"
            );
            Self::from_raw(PiConfigRaw::default())
        })
    }

    fn validate(raw: PiConfigRaw) -> Result<Self, ExtensionConfigErrors> {
        let mut errors = ExtensionConfigErrors::new("pi");

        if raw.tools.iter().all(|t| t.trim().is_empty()) {
            errors.push_with_suggestion(
                "tools",
                "At least one tool is required; a session with no tools cannot do anything",
                "Use the default: tools: [read]",
            );
        }

        if raw.binary.trim().is_empty() {
            errors.push_with_suggestion(
                "binary",
                "The pi binary name cannot be empty",
                "Use `pi` and make sure it is on `child_path`",
            );
        }

        check_nonzero(
            "timeouts.approval_secs",
            raw.timeouts.approval_secs,
            &mut errors,
        );
        check_nonzero("timeouts.idle_secs", raw.timeouts.idle_secs, &mut errors);
        check_nonzero(
            "timeouts.max_lifetime_secs",
            raw.timeouts.max_lifetime_secs,
            &mut errors,
        );
        if raw.sessions.max_per_user == 0 {
            errors.push_with_suggestion(
                "sessions.max_per_user",
                "Zero would refuse every session",
                "Use 1, or raise it to allow concurrent conversations per account",
            );
        }
        if raw.sessions.max_total == 0 {
            errors.push_with_suggestion(
                "sessions.max_total",
                "Zero would refuse every session",
                "Use 8, or size it to what the host can carry",
            );
        }

        if errors.is_empty() {
            Ok(Self::from_raw(raw))
        } else {
            Err(errors)
        }
    }

    fn from_raw(raw: PiConfigRaw) -> Self {
        Self {
            binary: raw.binary,
            expected_version: raw
                .expected_version
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty()),
            version_check: raw.version_check,
            workspace_root: raw.workspace_root,
            base_url: raw.base_url.unwrap_or_else(profile_base_url),
            provider: raw.provider,
            model: raw.model,
            models: raw
                .models
                .iter()
                .map(|m| m.trim().to_owned())
                .filter(|m| !m.is_empty())
                .collect(),
            persona: raw.persona,
            tools: raw
                .tools
                .iter()
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
                .collect(),
            child_path: raw.child_path,
            approval_timeout: Duration::from_secs(raw.timeouts.approval_secs),
            approve_all: raw.approve_all,
            idle_timeout: Duration::from_secs(raw.timeouts.idle_secs),
            max_lifetime: Duration::from_secs(raw.timeouts.max_lifetime_secs),
            max_sessions_per_user: raw.sessions.max_per_user,
            max_sessions_total: raw.sessions.max_total,
            throttle_session_per_ip: raw.throttle.session_per_ip,
            throttle_embed_token_per_ip: raw.throttle.embed_token_per_ip,
            throttle_window: Duration::from_secs(raw.throttle.window_secs.max(1)),
            limits: ChildLimits {
                nproc: raw.limits.nproc,
                fsize: raw.limits.fsize,
                address_space: raw.limits.address_space,
            },
            sandbox: raw.sandbox,
            jail_binary: raw.jail_binary.unwrap_or_else(default_jail_binary),
            jail_read_paths: raw
                .jail_read_paths
                .unwrap_or_else(|| DEFAULT_JAIL_READ_PATHS.iter().map(PathBuf::from).collect()),
            mcp_url: raw.mcp_url,
        }
    }

    pub(crate) fn model_name(&self) -> &str {
        &self.model
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn tools(&self) -> &[String] {
        &self.tools
    }

    pub const fn sandbox(&self) -> SandboxMode {
        self.sandbox
    }

    pub fn expected_version(&self) -> Option<&str> {
        self.expected_version.as_deref()
    }

    pub const fn version_check(&self) -> VersionCheckMode {
        self.version_check
    }

    pub const fn approve_all(&self) -> bool {
        self.approve_all
    }

    pub fn jail_read_paths(&self) -> &[PathBuf] {
        &self.jail_read_paths
    }

    pub(crate) fn warn_if_unsandboxed(&self) {
        if self.sandbox == SandboxMode::Off {
            tracing::warn!(
                "services/config/pi.yaml sets sandbox: off — pi children run with this \
                 process's filesystem access. The `read` tool can reach any file this uid \
                 can, including provider keys and the database URL. Only correct on a host \
                 without Landlock (Linux 5.13+), and only for a deployment nobody untrusted \
                 can sign into."
            );
        }
    }
}

fn check_nonzero(field: &str, value: u64, errors: &mut ExtensionConfigErrors) {
    if value == 0 {
        errors.push_with_suggestion(
            field.to_owned(),
            "Zero is not a timeout; it would expire immediately",
            "Give a positive number of seconds, or omit the key to take the default",
        );
    }
}
