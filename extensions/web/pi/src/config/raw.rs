//! The shape of `services/config/pi.yaml`, and the defaults a missing key
//! takes.
//!
//! Nothing here is reachable outside [`super`]: these types exist only long
//! enough for [`super::PiConfig::validate`] to check them and freeze them into
//! the runtime type.

use std::path::PathBuf;

use serde::Deserialize;

pub(super) const DEFAULT_TOOLS: &[&str] = &[
    "read",
    "mcp__systemprompt__list_topics",
    "mcp__systemprompt__get_topic",
    "mcp__systemprompt__search_docs",
    "mcp__systemprompt__governance_stats",
    "mcp__systemprompt__safety_findings",
    "mcp__systemprompt__admin_audit_dump",
    "mcp__systemprompt__fetch_remote_docs",
];

pub(super) const DEFAULT_PERSONA: &str =
    include_str!("../../../../../services/config/pi-persona.md");

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
    Required,
    Off,
}

/// What a `pi --version` mismatch does to session creation.
///
/// The pinned version is load-bearing security, not compatibility hygiene:
/// the skills story depends on `--skill` staying additive under
/// `--no-skills`, and the RPC frame shapes this module parses are unversioned
/// on the wire. An operator upgrading the global npm install changes both
/// silently — `required` (the default) refuses to spawn rather than find out
/// mid-conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionCheckMode {
    Required,
    Warn,
    Off,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PiConfigRaw {
    pub(super) binary: String,
    pub(super) child_path: String,
    pub(super) expected_version: Option<String>,
    pub(super) version_check: VersionCheckMode,
    pub(super) workspace_root: PathBuf,
    pub(super) base_url: Option<String>,
    pub(super) provider: String,
    pub(super) model: String,
    pub(super) models: Vec<String>,
    pub(super) persona: String,
    pub(super) tools: Vec<String>,
    pub(super) sandbox: SandboxMode,
    pub(super) approve_all: bool,
    pub(super) timeouts: TimeoutsRaw,
    pub(super) sessions: SessionsRaw,
    pub(super) throttle: ThrottleRaw,
    pub(super) limits: LimitsRaw,
    pub(super) jail_binary: Option<PathBuf>,
    pub(super) jail_read_paths: Option<Vec<PathBuf>>,
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
pub(super) struct ThrottleRaw {
    pub(super) session_per_ip: usize,
    pub(super) embed_token_per_ip: usize,
    pub(super) window_secs: u64,
}

impl Default for ThrottleRaw {
    fn default() -> Self {
        Self {
            session_per_ip: 5,
            embed_token_per_ip: 10,
            window_secs: 60,
        }
    }
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
            expected_version: None,
            version_check: VersionCheckMode::Required,
            workspace_root: PathBuf::from("/tmp/systemprompt-pi-sessions"),
            base_url: None,
            provider: "systemprompt".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            models: Vec::new(),
            persona: DEFAULT_PERSONA.to_owned(),
            tools: DEFAULT_TOOLS.iter().map(|s| (*s).to_owned()).collect(),
            sandbox: SandboxMode::Required,
            approve_all: true,
            timeouts: TimeoutsRaw::default(),
            sessions: SessionsRaw::default(),
            throttle: ThrottleRaw::default(),
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
