//! The four places a hub tool has to be named, kept in agreement.
//!
//! A tool reaches the model only if every one of these lists carries it:
//!
//! * `extensions/mcp/systemprompt/src/tools.rs` — declared to MCP
//! * `handlers/pi/shim/mcp-client.ts` — registered with pi
//! * `handlers/pi/mcp/mod.rs` `FORWARDABLE` — accepted by the proxy
//! * `services/config/pi.yaml` `tools:` — allowed by pi itself
//!
//! Each omission fails differently and none of them says so. Missing from
//! `pi.yaml` and the tool is dropped before the model sees it, which looks
//! exactly like the extension having failed to load. Missing from `FORWARDABLE`
//! and the call 400s as an unknown tool — which, for the two tools that exist
//! to be *refused by policy*, is a refusal that demonstrates nothing.
//!
//! Four hand-maintained lists is three too many to keep in step by eye.

use systemprompt_web_admin::test_support::FORWARDABLE;

fn repo_file(rel: &str) -> String {
    let path = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../..")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()))
}

/// The `mcp__systemprompt__*` entries under `tools:` in
/// `services/config/pi.yaml`.
fn pi_yaml_hub_tools() -> Vec<String> {
    repo_file("services/config/pi.yaml")
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- mcp__systemprompt__"))
        .map(str::to_owned)
        .collect()
}

/// Tool names declared in the hub's `list_tools`, read from its source.
///
/// Matched off the `name:` field of each `ToolDef`, which is the same string
/// the proxy and the shim concatenate onto the server prefix.
fn declared_tools() -> Vec<String> {
    repo_file("extensions/mcp/systemprompt/src/tools.rs")
        .lines()
        .filter_map(|l| l.trim().strip_prefix("name: \""))
        .filter_map(|r| r.split_once('"').map(|(n, _)| n.to_owned()))
        .collect()
}

/// Tool names the pi extension registers, read from the shipped TypeScript.
fn shim_tools() -> Vec<String> {
    repo_file("extensions/web/pi/src/shim/mcp-client.ts")
        .lines()
        .filter_map(|l| l.trim().strip_prefix("name: \""))
        .filter_map(|r| r.split_once('"').map(|(n, _)| n.to_owned()))
        .collect()
}

#[test]
fn the_four_hub_tool_lists_agree() {
    let mut expected = pi_yaml_hub_tools();
    assert!(
        !expected.is_empty(),
        "pi.yaml lists no hub tools; the terminal has nothing to call"
    );
    expected.sort_unstable();

    for (label, mut actual) in [
        ("tools.rs", declared_tools()),
        ("mcp-client.ts", shim_tools()),
        (
            "FORWARDABLE",
            FORWARDABLE.iter().map(|s| (*s).to_owned()).collect(),
        ),
    ] {
        actual.sort_unstable();
        assert_eq!(
            actual, expected,
            "{label} disagrees with services/config/pi.yaml about which hub tools exist"
        );
    }
}

/// The two tools that exist to be refused must still be reachable.
///
/// Their whole value is that the refusal comes from a policy rather than from a
/// missing route, and the tempting "cleanup" is to delete the tool that keeps
/// getting denied. That removes the demonstration, not the risk it stands for.
#[test]
fn the_tools_that_exist_to_be_refused_are_still_registered() {
    let listed = pi_yaml_hub_tools();
    for tool in ["admin_audit_dump", "fetch_remote_docs"] {
        assert!(
            listed.iter().any(|t| t == tool),
            "{tool} is no longer registered; the policy that refuses it now has \
             nothing to refuse and the demonstration silently passes"
        );
    }
}
