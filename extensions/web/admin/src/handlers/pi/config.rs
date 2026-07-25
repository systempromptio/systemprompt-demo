//! Runtime configuration for the pi web terminal.
//!
//! Read from the environment once at router construction. The widget is off by
//! default: without an explicit gateway credential there is nothing to spawn
//! against, and silently starting a half-configured agent service is worse than
//! not offering one.

use std::path::PathBuf;
use std::time::Duration;

/// Tools a session may use. Read-only on purpose — see the sandboxing note in
/// the module docs: with no container per session, `bash`/`write`/`edit` would
/// make this a remote code execution service rather than a governed demo.
const DEFAULT_TOOLS: &[&str] = &["read"];

#[derive(Debug, Clone)]
pub(crate) struct PiConfig {
    pub(super) binary: String,
    pub(super) workspace_root: PathBuf,
    pub(super) base_url: String,
    /// The gateway credential pi authenticates to `/v1/messages` with.
    ///
    /// One credential for all sessions, with per-session attribution coming
    /// from the attested `x-session-id` header instead. A PAT per conversation
    /// would be better and is deferred.
    pub(super) gateway_key: String,
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
    pub(super) max_sessions_per_user: usize,
    pub(super) max_sessions_total: usize,
}

impl PiConfig {
    /// Build from the environment, or `None` when the widget is not configured.
    pub(crate) fn from_env() -> Option<Self> {
        let gateway_key = std::env::var("SP_PI_GATEWAY_KEY").ok()?;
        if gateway_key.trim().is_empty() {
            return None;
        }
        Some(Self {
            binary: env_or("SP_PI_BINARY", "pi"),
            workspace_root: PathBuf::from(env_or(
                "SP_PI_WORKSPACE_ROOT",
                "/tmp/systemprompt-pi-sessions",
            )),
            base_url: env_or("SP_PI_BASE_URL", "http://127.0.0.1:8080"),
            gateway_key,
            provider: env_or("SP_PI_PROVIDER", "systemprompt"),
            model: env_or("SP_PI_MODEL", "claude-sonnet-4-6"),
            tools: std::env::var("SP_PI_TOOLS").map_or_else(
                |_| DEFAULT_TOOLS.iter().map(|s| (*s).to_owned()).collect(),
                |raw| {
                    raw.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned)
                        .collect()
                },
            ),
            child_path: env_or("SP_PI_CHILD_PATH", "/usr/local/bin:/usr/bin:/bin"),
            approval_timeout: Duration::from_secs(secs("SP_PI_APPROVAL_TIMEOUT_SECS", 120)),
            approve_all: env_or("SP_PI_APPROVE_ALL", "1") == "1",
            idle_timeout: Duration::from_secs(secs("SP_PI_IDLE_SECS", 600)),
            max_lifetime: Duration::from_secs(secs("SP_PI_MAX_LIFETIME_SECS", 3_600)),
            max_sessions_per_user: secs("SP_PI_MAX_PER_USER", 1) as usize,
            max_sessions_total: secs("SP_PI_MAX_TOTAL", 8) as usize,
        })
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn secs(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
