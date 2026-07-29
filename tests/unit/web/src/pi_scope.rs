//! Workspace confinement, applied before a human is ever asked.
//!
//! pi's `read` applies no path containment of its own — an absolute path goes
//! straight to `readFile`. This is the check that turns that into a legible
//! `workspace_scope` denial rather than a bare `EACCES`, and it has to reject
//! every way out, not the obvious one.

use systemprompt_web_pi::test_support::escape_reason;

use std::path::Path;

fn input(path: &str) -> serde_json::Value {
    serde_json::json!({ "path": path })
}

#[test]
fn allows_paths_inside_the_workspace() {
    let ws = Path::new("/tmp/pi/abc");
    assert!(escape_reason(ws, Some(&input("README.md"))).is_none());
    assert!(escape_reason(ws, Some(&input("./sub/dir/x.txt"))).is_none());
    assert!(escape_reason(ws, Some(&input("/tmp/pi/abc/home/x"))).is_none());
    assert!(escape_reason(ws, Some(&input("sub/../README.md"))).is_none());
}

/// The four shapes that motivated this: the secrets file, the child's own
/// gateway credential under `~`, a traversal, and a sibling session.
#[test]
fn rejects_every_way_out() {
    let ws = Path::new("/tmp/pi/abc");
    for path in [
        "/var/www/html/systemprompt-demo/.systemprompt/profiles/local/secrets.json",
        "~/.pi/agent/models.json",
        "../../../etc/passwd",
        "../def/home/.pi/agent/models.json",
        "/proc/self/environ",
    ] {
        assert!(
            escape_reason(ws, Some(&input(path))).is_some(),
            "{path} should have been refused"
        );
    }
}

/// A prefix match on the string alone would let a sibling directory whose
/// name merely starts with the workspace's through.
#[test]
fn does_not_confuse_a_sibling_prefix_for_containment() {
    assert!(escape_reason(Path::new("/tmp/pi/abc"), Some(&input("/tmp/pi/abcdef/x"))).is_some());
}

/// pi 0.82's read/write/edit/ls/find/grep all name the argument `path`
/// (`dist/core/tools/*.d.ts`), so `path` is the key the allowed tool set
/// actually sends; the rest are MCP-side aliases. On a host without Landlock
/// this check is the only path confinement there is, so a pi upgrade that
/// renamed the argument would silently remove it — which is what
/// `version_check: required` in pi.yaml exists to catch.
#[test]
fn covers_every_alias_a_path_may_arrive_under() {
    let ws = Path::new("/tmp/pi/abc");
    for key in ["path", "file_path", "filePath", "file"] {
        let call = serde_json::json!({ key: "/etc/passwd" });
        assert!(
            escape_reason(ws, Some(&call)).is_some(),
            "`{key}` should have been checked"
        );
    }
}

/// Judging the first key found would let the second through unchecked.
#[test]
fn rejects_an_escape_under_any_key_when_several_are_present() {
    let ws = Path::new("/tmp/pi/abc");
    let call = serde_json::json!({ "path": "README.md", "file_path": "/etc/passwd" });
    assert!(escape_reason(ws, Some(&call)).is_some());
}

#[test]
fn ignores_calls_with_no_path_argument() {
    let ws = Path::new("/tmp/pi/abc");
    assert!(escape_reason(ws, None).is_none());
    assert!(escape_reason(ws, Some(&serde_json::json!({ "pattern": "TODO" }))).is_none());
}
