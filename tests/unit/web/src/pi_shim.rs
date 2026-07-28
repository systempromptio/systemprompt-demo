//! The governance shim decides nothing — it suspends the call and asks.
//!
//! If policy ever leaked into the shim, a decision would be being made inside
//! the least trusted process in the system. These read the shipped TypeScript
//! to confirm it stays a relay, and that it fails closed.

use systemprompt_web_pi::SHIM_SOURCE;

/// Executable lines only. The shim's own comments discuss the things these
/// tests forbid — a naive substring search over the whole file would match
/// the prose explaining why the code avoids them.
fn shim_code() -> String {
    let mut out = String::with_capacity(SHIM_SOURCE.len());
    let mut rest = SHIM_SOURCE;
    // Block comments first, so a `//` inside one cannot confuse the line pass.
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..]
            .find("*/")
            .map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    out.push_str(rest);
    out.lines()
        .map(|l| l.split_once("//").map_or(l, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The shim must decide nothing. A policy name or an HTTP call in here
/// would mean a second place where a rule lives — and the one nobody
/// reviews.
#[test]
fn shim_holds_no_policy() {
    let code = shim_code();
    for forbidden in [
        "FAIL_OPEN",
        "fetch(",
        "blocklist",
        "secret_scan",
        "XMLHttpRequest",
    ] {
        assert!(
            !code.contains(forbidden),
            "shim code should not contain {forbidden}"
        );
    }
}

/// Every path that is not an explicit approval must block.
#[test]
fn shim_denies_by_default() {
    let code = shim_code();
    assert!(code.contains("block: true"), "no block path in the shim");
    assert!(
        code.contains("catch"),
        "a channel failure must be caught and denied"
    );
    assert!(
        code.contains("return false"),
        "the catch arm must deny rather than rethrow"
    );
}

/// The comment stripper has to survive the shapes the shim actually uses,
/// or the tests above quietly stop checking anything.
#[test]
fn comment_stripper_removes_both_comment_forms() {
    assert!(shim_code().contains("ExtensionAPI"));
    assert!(
        !shim_code().contains("pi runs its tools in-process"),
        "block comment survived stripping"
    );
    assert!(
        !shim_code().contains("Title the proxy matches on"),
        "line comment survived stripping"
    );
}
