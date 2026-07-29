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

/// pi 0.82 names the argument `path` on every built-in that takes one — read,
/// write, edit, ls, find, grep — so `path` alone covers the tool set this
/// deployment allows. The rest are aliases an MCP tool might use; matching a
/// key no allowlisted tool sends costs nothing, and missing one costs the
/// whole check on a host where Landlock is unavailable.
const PATH_KEYS: &[&str] = &["path", "file_path", "filePath", "file"];

/// Why a call was refused, or `None` when it stays inside the workspace.
///
/// A tool call carrying no path argument is not this check's business and
/// passes: confinement of *those* is the jail's job, not a string match's.
pub fn escape_reason(workspace: &Path, tool_input: Option<&serde_json::Value>) -> Option<String> {
    let input = tool_input?;
    // Why: every path-bearing key is checked, not the first that matches — a
    // call carrying both `path` and `file_path` would otherwise be judged on
    // one of them and smuggle the other past.
    PATH_KEYS
        .iter()
        .filter_map(|key| input.get(*key)?.as_str())
        .find_map(|raw| escapes(workspace, raw))
}

fn escapes(workspace: &Path, raw: &str) -> Option<String> {
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
