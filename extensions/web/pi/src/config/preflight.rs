//! Startup warnings for containment that a deployment has relaxed.
//!
//! Both boundaries below can be weakened by the host rather than by
//! `services/config/pi.yaml`, which is exactly what makes them worth logging:
//! the file that ships looks identical on a deployment where the sandbox
//! applies and one where it cannot, so a reader of the config alone cannot
//! tell which they are running. These warnings put the difference in the log.

use std::path::Path;

use super::{PiConfig, SandboxMode};

pub(super) fn warn(cfg: &PiConfig) {
    if cfg.sandbox == SandboxMode::Off {
        tracing::warn!(
            "pi sandbox is off (services/config/pi.yaml or the SP_PI_SANDBOX \
             override) — pi children run with this \
             process's filesystem access. The `read` tool can reach any file this uid \
             can, including provider keys and the database URL. Only correct on a host \
             without Landlock (Linux 5.13+), and only for a deployment nobody untrusted \
             can sign into."
        );
    }
    if let Some(fstype) = backing_fstype(&cfg.workspace_root)
        && fstype != "tmpfs"
    {
        tracing::warn!(
            workspace_root = %cfg.workspace_root.display(),
            fstype = %fstype,
            "pi session workspaces are not on a tmpfs — `limits.fsize` caps each \
             file a session writes, nothing caps the total, so session churn is \
             bounded by the host's disk rather than by a size-capped mount"
        );
    }
}

// Why: the longest matching mount point wins, since `/tmp` on its own mount
// must beat the `/` entry that also prefixes it.
fn backing_fstype(path: &Path) -> Option<String> {
    let mounts = std::fs::read_to_string("/proc/self/mounts").ok()?;
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace().skip(1);
            let point = fields.next()?;
            let fstype = fields.next()?;
            path.starts_with(point)
                .then(|| (point.len(), fstype.to_owned()))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, fstype)| fstype)
}
