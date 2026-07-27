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

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use systemprompt::config::ProfileBootstrap;
use systemprompt::models::AppPaths;
use systemprompt_web_shared::config_errors::ExtensionConfigErrors;

/// Tools a session may use. Read-only on purpose — see the sandboxing note in
/// the module docs: with no container per session, `bash`/`write`/`edit` would
/// make this a remote code execution service rather than a governed demo.
const DEFAULT_TOOLS: &[&str] = &[
    "read",
    "mcp__systemprompt__list_topics",
    "mcp__systemprompt__get_topic",
    "mcp__systemprompt__search_docs",
    "mcp__systemprompt__governance_stats",
    "mcp__systemprompt__fetch_remote_docs",
];

/// Directories the jailed child may read and execute from, derived by running
/// `pi --version` under the jail and widening until it started: the interpreter
/// and its shebang chain, the shared libraries `ldd` reports, the CA bundle,
/// and the two device nodes node opens. pi's own package root is *not* here —
/// it lives under `$HOME` on an nvm host and is derived from the configured
/// `binary` at spawn time so it follows an upgrade instead of being pinned.
///
/// `/proc` is deliberately absent and must stay absent: `/proc/<server-pid>/`
/// `environ` is readable by this uid and holds `DATABASE_URL` and the provider
/// keys. Granting it would re-open the hole the jail closes. If a future node
/// needs it, grant `/proc/self` alone.
const DEFAULT_JAIL_READ_PATHS: &[&str] = &[
    "/usr/bin",
    "/usr/lib",
    "/usr/local/bin",
    "/usr/local/lib",
    "/bin",
    "/lib",
    "/lib64",
    "/etc/ssl",
    "/etc/ca-certificates",
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/nsswitch.conf",
    "/dev/null",
    "/dev/urandom",
];

/// Whether the child must start inside a Landlock jail.
///
/// There is deliberately no third "best effort" value. A boundary that
/// silently downgrades is the exact failure this change exists to remove: the
/// vulnerability it closes was masked for months by a credit guard nobody
/// thought of as a security control. `Off` is for kernels below 5.13 and for
/// non-Linux development hosts, and it announces itself at WARN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SandboxMode {
    /// No child starts unless the jail confirms `FullyEnforced`.
    Required,
    Off,
}

// ---------------------------------------------------------------------------
// Raw schema — the shape of services/config/pi.yaml
// ---------------------------------------------------------------------------

/// `deny_unknown_fields` so a stale or misspelled key is a startup error
/// rather than a setting that silently does nothing. Every field defaults, so
/// the file only ever states what it changes.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PiConfigRaw {
    binary: String,
    child_path: String,
    workspace_root: PathBuf,
    /// Absent means "wherever this server answers", taken from the profile's
    /// `server.api_internal_url`. Set it only to point sessions at a different
    /// origin than the one serving them.
    base_url: Option<String>,
    provider: String,
    model: String,
    tools: Vec<String>,
    sandbox: SandboxMode,
    approve_all: bool,
    timeouts: TimeoutsRaw,
    sessions: SessionsRaw,
    limits: LimitsRaw,
    /// Defaults to `sp-pi-jail` beside this executable.
    jail_binary: Option<PathBuf>,
    /// Replaces [`DEFAULT_JAIL_READ_PATHS`] wholesale — it does not extend it.
    jail_read_paths: Option<Vec<PathBuf>>,
    /// Where the `systemprompt` MCP hub answers. Called server-side by
    /// [`super::mcp`], never by the child.
    mcp_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "the unit belongs in the key: these are what a YAML author types"
)]
struct TimeoutsRaw {
    approval_secs: u64,
    idle_secs: u64,
    max_lifetime_secs: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SessionsRaw {
    max_per_user: usize,
    max_total: usize,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LimitsRaw {
    nproc: u64,
    fsize: u64,
    address_space: u64,
}

impl Default for PiConfigRaw {
    fn default() -> Self {
        Self {
            binary: "pi".to_owned(),
            child_path: "/usr/local/bin:/usr/bin:/bin".to_owned(),
            workspace_root: PathBuf::from("/tmp/systemprompt-pi-sessions"),
            base_url: None,
            provider: "systemprompt".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            tools: DEFAULT_TOOLS.iter().map(|s| (*s).to_owned()).collect(),
            sandbox: SandboxMode::Required,
            approve_all: true,
            timeouts: TimeoutsRaw::default(),
            sessions: SessionsRaw::default(),
            limits: LimitsRaw::default(),
            jail_binary: None,
            jail_read_paths: None,
            mcp_url: "http://127.0.0.1:5010/mcp".to_owned(),
        }
    }
}

impl Default for TimeoutsRaw {
    fn default() -> Self {
        Self {
            approval_secs: 120,
            idle_secs: 600,
            max_lifetime_secs: 3_600,
        }
    }
}

impl Default for SessionsRaw {
    fn default() -> Self {
        Self {
            max_per_user: 1,
            max_total: 8,
        }
    }
}

impl Default for LimitsRaw {
    fn default() -> Self {
        Self {
            nproc: 0,
            fsize: 64 * 1024 * 1024,
            address_space: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Validated config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct PiConfig {
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
        let raw = if path.exists() {
            let content = std::fs::read_to_string(&path).map_err(|e| {
                let mut errors = ExtensionConfigErrors::new("pi");
                errors.push_with_path("_file", format!("Failed to read config file: {e}"), &path);
                errors
            })?;
            serde_yaml::from_str(&content).map_err(|e| {
                let mut errors = ExtensionConfigErrors::new("pi");
                errors.push_with_path("_parse", format!("Failed to parse config YAML: {e}"), &path);
                errors
            })?
        } else {
            PiConfigRaw::default()
        };
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
    pub(crate) fn validate(raw: PiConfigRaw) -> Result<Self, ExtensionConfigErrors> {
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

/// `services/config/pi.yaml` under the active profile.
///
/// The profile is loaded once at startup, so a failure here means the process
/// has no profile at all — a condition the server would already have failed
/// on. Falling back to the repo-relative path keeps a `cargo test` run that
/// never bootstrapped a profile working.
fn config_path() -> PathBuf {
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
/// `validate` total so the default config is always constructible, which is
/// what [`PiConfig::load_or_defaults`] leans on.
fn profile_base_url() -> String {
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
fn default_jail_binary() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("sp-pi-jail")))
        .unwrap_or_else(|| PathBuf::from("sp-pi-jail"))
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "assertions in tests"
)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Result<PiConfig, ExtensionConfigErrors> {
        let raw: PiConfigRaw = serde_yaml::from_str(yaml).expect("test yaml parses");
        PiConfig::validate(raw)
    }

    #[test]
    fn empty_file_is_all_defaults() {
        let raw: PiConfigRaw = serde_yaml::from_str("{}").expect("empty map parses");
        assert_eq!(raw.tools, DEFAULT_TOOLS);
        assert_eq!(raw.sandbox, SandboxMode::Required);
        assert!(raw.approve_all);
        assert_eq!(raw.sessions.max_per_user, 1);
    }

    #[test]
    fn sandbox_typo_is_rejected_rather_than_read_as_off() {
        let err = serde_yaml::from_str::<PiConfigRaw>("sandbox: of").unwrap_err();
        assert!(err.to_string().contains("of"), "{err}");
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = serde_yaml::from_str::<PiConfigRaw>("aprove_all: true").unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn empty_tool_list_fails_validation() {
        let errors = parse("tools: []\nbase_url: http://127.0.0.1:8080").unwrap_err();
        assert!(errors.errors.iter().any(|e| e.field == "tools"));
    }

    #[test]
    fn zero_timeout_fails_validation() {
        let errors =
            parse("timeouts:\n  idle_secs: 0\nbase_url: http://127.0.0.1:8080").unwrap_err();
        assert!(
            errors
                .errors
                .iter()
                .any(|e| e.field == "timeouts.idle_secs")
        );
    }

    /// The shipped file has to satisfy `deny_unknown_fields` and every check
    /// in `validate`, or the deployment it configures silently runs on
    /// defaults instead. Nothing else exercises it until startup.
    #[test]
    fn the_checked_in_config_is_valid() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../services/config/pi.yaml");
        let yaml = std::fs::read_to_string(path).expect("services/config/pi.yaml is readable");
        let cfg = parse(&yaml).expect("services/config/pi.yaml validates");
        // The two settings that decide whether this is a demo or an exposure.
        assert_eq!(cfg.sandbox, SandboxMode::Required);
        assert!(!cfg.tools.iter().any(|t| matches!(t.as_str(), "bash" | "write" | "edit")));
        assert!(cfg.approve_all);
    }

    #[test]
    fn explicit_base_url_needs_no_profile() {
        let cfg = parse("base_url: https://example.test").expect("validates");
        assert_eq!(cfg.base_url, "https://example.test");
        assert_eq!(cfg.jail_read_paths.len(), DEFAULT_JAIL_READ_PATHS.len());
    }
}
