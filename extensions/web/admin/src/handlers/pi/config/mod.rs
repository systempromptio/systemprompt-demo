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
pub use raw::SandboxMode;

// ---------------------------------------------------------------------------
// Validated config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PiConfig {
    pub(super) binary: String,
    pub(super) workspace_root: PathBuf,
    pub(super) base_url: String,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) tools: Vec<String>,
    pub(super) child_path: String,
    /// How long a tool call waits on a human before failing closed.
    pub(super) approval_timeout: Duration,
    /// Ask a human about every tool call, not just flagged ones. With a
    /// read-only tool set there is nothing on the flagged list, so this is what
    /// makes the approval UI visible at all in V1.
    pub(super) approve_all: bool,
    pub(super) idle_timeout: Duration,
    pub(super) max_lifetime: Duration,
    /// Conversations one account may have at once. A request past this closes
    /// the account's oldest rather than being refused — see
    /// [`super::registry::PiRegistry::create`].
    pub(super) max_sessions_per_user: usize,
    pub(super) max_sessions_total: usize,
    pub(super) limits: ChildLimits,
    pub(super) sandbox: SandboxMode,
    /// `sp-pi-jail`. Defaults to a sibling of this executable, which is right
    /// in both layouts: `target/debug/` in development, `/app/bin/` in the
    /// image.
    pub(super) jail_binary: PathBuf,
    pub(super) jail_read_paths: Vec<PathBuf>,
    /// The `systemprompt` MCP hub's endpoint, called server-side by the proxy
    /// in [`super::mcp`]. Deliberately not reachable by the child: the jail
    /// grants outbound TCP to the gateway's port alone, and the hub trusts
    /// whatever identity headers it is handed.
    pub(super) mcp_url: String,
}

/// Resource ceilings the child starts under.
///
/// These bound the blast radius of a tool that gets through, which is a
/// different job from deciding whether it should run. They are not a sandbox:
/// with `--tools read` the thing they actually buy is that a runaway or
/// hostile read cannot exhaust the host the server shares.
#[derive(Debug, Clone, Copy)]
pub(super) struct ChildLimits {
    /// Processes. **Zero means unset, and that is the default**, for two
    /// measured reasons: `RLIMIT_NPROC` counts every process the *uid* owns,
    /// not the child's descendants, so any value below the server user's
    /// existing process count stops the child forking at all; and `/bin/sh` is
    /// dash on Debian, whose `ulimit` has no `-u`. Useful once the child has a
    /// dedicated uid — the same prerequisite as enabling `bash`.
    pub(super) nproc: u64,
    /// Bytes any single file the child writes may reach. The one limit that is
    /// on by default: per-file, per-process, and supported by every `ulimit`.
    pub(super) fsize: u64,
    /// Virtual address space. **Zero means unset, and that is the default.**
    /// V8 reserves address space far in excess of what it commits, so a figure
    /// chosen to look prudent kills `pi` at startup rather than bounding it.
    /// Set it deliberately, against a measured value, or leave it alone.
    pub(super) address_space: u64,
}
// A dedicated low-privilege uid is deliberately not here. Dropping privilege
// in-process needs `setuid` between fork and exec, and this workspace denies
// `unsafe_code`. It belongs to whatever supervises the server — `User=` in a
// systemd unit, or the container's own user — where it also covers the parent.

impl PiConfig {
    /// Load and validate `services/config/pi.yaml`.
    ///
    /// A missing file is `Ok` on defaults — the terminal is always available,
    /// and the shipped defaults are the ones the demo runs on. `Err` is
    /// reserved for a file that exists but cannot be read, parsed, or
    /// validated.
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

    /// Parse a config body and validate it.
    ///
    /// Split out of [`Self::load`] so the same path can be driven from a
    /// string. A YAML error surfaces as a `_parse` entry rather than a
    /// separate error type, which keeps one result shape for callers that
    /// only need to know the config was rejected.
    pub fn parse(yaml: &str) -> Result<Self, ExtensionConfigErrors> {
        let raw: PiConfigRaw = serde_yaml::from_str(yaml).map_err(|e| {
            let mut errors = ExtensionConfigErrors::new("pi");
            errors.push("_parse", format!("Failed to parse config YAML: {e}"));
            errors
        })?;
        Self::validate(raw)
    }

    /// [`Self::load`], reporting a broken file at ERROR and continuing on the
    /// shipped defaults.
    ///
    /// The terminal stays mounted because it is the site's primary demo and a
    /// dead one is the failure this config path exists to remove. Running on
    /// defaults is the fail-*closed* direction on the key that matters:
    /// `sandbox: required`, a read-only tool set, and every ceiling in place —
    /// so a typo cannot widen the boundary, only lose a deliberate widening.
    pub(crate) fn load_or_defaults() -> Self {
        Self::load().unwrap_or_else(|errors| {
            tracing::error!(
                "{errors}\nFalling back to built-in pi defaults. The terminal is serving, but \
                 not with the settings in services/config/pi.yaml."
            );
            Self::from_raw(PiConfigRaw::default())
        })
    }

    /// Reject the settings that are wrong rather than merely unusual, then
    /// hand off to [`Self::from_raw`].
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

        check_nonzero("timeouts.approval_secs", raw.timeouts.approval_secs, &mut errors);
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

    /// Freeze a raw config into the runtime types (`Duration`, `PathBuf`) the
    /// rest of the module uses.
    ///
    /// Infallible on purpose: it is what makes `PiConfigRaw::default()` always
    /// constructible, which is the guarantee [`Self::load_or_defaults`] rests
    /// on. Rejection belongs in [`Self::validate`], which calls this once its
    /// checks pass.
    fn from_raw(raw: PiConfigRaw) -> Self {
        Self {
            binary: raw.binary,
            workspace_root: raw.workspace_root,
            base_url: raw.base_url.unwrap_or_else(profile_base_url),
            provider: raw.provider,
            model: raw.model,
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
            limits: ChildLimits {
                nproc: raw.limits.nproc,
                fsize: raw.limits.fsize,
                address_space: raw.limits.address_space,
            },
            sandbox: raw.sandbox,
            jail_binary: raw.jail_binary.unwrap_or_else(default_jail_binary),
            jail_read_paths: raw.jail_read_paths.unwrap_or_else(|| {
                DEFAULT_JAIL_READ_PATHS.iter().map(PathBuf::from).collect()
            }),
            mcp_url: raw.mcp_url,
        }
    }

    /// The model a session will run on, for the startup log.
    pub(crate) fn model_name(&self) -> &str {
        &self.model
    }

    /// The origin sessions call back on.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The tool set a session is bounded to.
    pub fn tools(&self) -> &[String] {
        &self.tools
    }

    pub const fn sandbox(&self) -> SandboxMode {
        self.sandbox
    }

    pub const fn approve_all(&self) -> bool {
        self.approve_all
    }

    pub fn jail_read_paths(&self) -> &[PathBuf] {
        &self.jail_read_paths
    }

    /// Say once, loudly, when the widget is serving unsandboxed children.
    /// Called at router construction so it lands in the startup log rather
    /// than being buried in a per-session line nobody greps for.
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
