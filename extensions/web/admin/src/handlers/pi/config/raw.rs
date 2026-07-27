//! The shape of `services/config/pi.yaml`, and the defaults a missing key takes.
//!
//! Nothing here is reachable outside [`super`]: these types exist only long
//! enough for [`super::PiConfig::validate`] to check them and freeze them into
//! the runtime type.

use std::path::PathBuf;

use serde::Deserialize;

/// Tools a session may use. Read-only on purpose — see the sandboxing note in
/// the module docs: with no container per session, `bash`/`write`/`edit` would
/// make this a remote code execution service rather than a governed demo.
pub(super) const DEFAULT_TOOLS: &[&str] = &[
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
pub(super) const DEFAULT_JAIL_READ_PATHS: &[&str] = &[
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
pub enum SandboxMode {
    /// No child starts unless the jail confirms `FullyEnforced`.
    Required,
    Off,
}

/// `deny_unknown_fields` so a stale or misspelled key is a startup error
/// rather than a setting that silently does nothing. Every field defaults, so
/// the file only ever states what it changes.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PiConfigRaw {
    pub(super) binary: String,
    pub(super) child_path: String,
    pub(super) workspace_root: PathBuf,
    /// Absent means "wherever this server answers", taken from the profile's
    /// `server.api_internal_url`. Set it only to point sessions at a different
    /// origin than the one serving them.
    pub(super) base_url: Option<String>,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) tools: Vec<String>,
    pub(super) sandbox: SandboxMode,
    pub(super) approve_all: bool,
    pub(super) timeouts: TimeoutsRaw,
    pub(super) sessions: SessionsRaw,
    pub(super) limits: LimitsRaw,
    /// Defaults to `sp-pi-jail` beside this executable.
    pub(super) jail_binary: Option<PathBuf>,
    /// Replaces [`DEFAULT_JAIL_READ_PATHS`] wholesale — it does not extend it.
    pub(super) jail_read_paths: Option<Vec<PathBuf>>,
    /// Where the `systemprompt` MCP hub answers. Called server-side by
    /// [`super::super::mcp`], never by the child.
    pub(super) mcp_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[expect(
    clippy::struct_field_names,
    reason = "the unit belongs in the key: these are what a YAML author types"
)]
pub(super) struct TimeoutsRaw {
    pub(super) approval_secs: u64,
    pub(super) idle_secs: u64,
    pub(super) max_lifetime_secs: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct SessionsRaw {
    pub(super) max_per_user: usize,
    pub(super) max_total: usize,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct LimitsRaw {
    pub(super) nproc: u64,
    pub(super) fsize: u64,
    pub(super) address_space: u64,
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
