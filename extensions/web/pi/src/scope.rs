//! Workspace confinement, checked before a human is ever asked.
//!
//! The Landlock jail is the boundary; this is the second layer, and it exists
//! for two reasons the kernel cannot cover.
//!
//! First, legibility. Landlock denies with a bare `EACCES`, which reaches the
//! user as an unexplained tool error and leaves nothing in the governance
//! spine — the one place this deployment promises every decision is recorded.
//! Rejecting here produces a `ToolBlocked` card naming `workspace_scope` and an
//! audit row beside it.
//!
//! Second, independence. The jail depends on the kernel; this does not. A host
//! running `sandbox: off`, or one below Landlock's 5.13 floor, still gets
//! path confinement for the tools whose arguments are paths. Neither layer is
//! sufficient alone, and each must be shown to work with the other disabled.

use std::path::{Component, Path, PathBuf};

const PATH_KEYS: &[&str] = &["path", "file_path"];

/// Why a call was refused, or `None` when it stays inside the workspace.
///
/// A tool call carrying no path argument is not this check's business and
/// passes: confinement of *those* is the jail's job, not a string match's.
pub fn escape_reason(workspace: &Path, tool_input: Option<&serde_json::Value>) -> Option<String> {
    let raw = PATH_KEYS
        .iter()
        .find_map(|key| tool_input?.get(*key)?.as_str())?;

    if raw.starts_with('~') {
        return Some(format!(
            "path `{raw}` refers to a home directory outside the session workspace"
        ));
    }
    let candidate = Path::new(raw);
    if candidate.is_absolute() && !normalize(candidate).starts_with(workspace) {
        return Some(format!("path `{raw}` is outside the session workspace"));
    }
    if !candidate.is_absolute() && !normalize(&workspace.join(candidate)).starts_with(workspace) {
        return Some(format!(
            "path `{raw}` traverses out of the session workspace"
        ));
    }
    if let Ok(real) = std::fs::canonicalize(if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace.join(candidate)
    }) && !real.starts_with(workspace)
    {
        return Some(format!(
            "path `{raw}` resolves through a link to {}, outside the session workspace",
            real.display()
        ));
    }
    None
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {},
            Component::ParentDir => {
                out.pop();
            },
            other => out.push(other),
        }
    }
    out
}
