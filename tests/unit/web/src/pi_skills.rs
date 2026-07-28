//! Parsing skill front-matter, escaping what goes back out, and keeping the
//! shipped skill bodies inert to the gateway's secret scanner.

use systemprompt_web_governance::test_support::scan_str_for_secret;
use systemprompt_web_pi::test_support::{escape, scalar};

/// The credential-shaped literal `demonstrate_governance` sends to trigger a
/// `secret_scan` deny. It matches an operator `extra_pattern`, never a
/// built-in.
const DEMO_CREDENTIAL: &str = "SPDEMOKEY-0000000000000000";

/// The governed name of the tool `demonstrate_scope_rejection` reaches for.
const SCOPE_DEMO_TOOL: &str = "mcp__systemprompt__admin_audit_dump";

fn skills_root() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../services/skills"
    ))
}

/// Every shipped `SKILL.md`, as `(id, body)`.
fn shipped_skill_bodies() -> Vec<(String, String)> {
    let root = skills_root();
    let entries = std::fs::read_dir(&root).expect("services/skills is readable");
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        let config = dir.join("config.yaml");
        if !config.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&config).expect("config.yaml is readable");
        let id = scalar(&raw, "id").expect("every skill has an id");
        let file = scalar(&raw, "file").unwrap_or_else(|| "SKILL.md".to_owned());
        let body = std::fs::read_to_string(dir.join(&file)).expect("skill body is readable");
        out.push((id, body));
    }
    assert!(!out.is_empty(), "no skills found under {}", root.display());
    out
}

#[test]
fn reads_quoted_and_bare_scalars() {
    let raw = "id: demonstrate_governance\ndescription: \"Exercise the pipeline\"\n";
    assert_eq!(scalar(raw, "id").as_deref(), Some("demonstrate_governance"));
    assert_eq!(
        scalar(raw, "description").as_deref(),
        Some("Exercise the pipeline")
    );
    assert_eq!(scalar(raw, "missing"), None);
}

/// A key that only appears nested must not be mistaken for a top-level one
/// — `tags:` entries are indented, and a naive `contains` would match.
#[test]
fn ignores_indented_keys() {
    assert_eq!(scalar("tags:\n  id: nope\n", "id"), None);
}

/// An unescaped quote in a description would produce frontmatter pi cannot
/// parse, and pi drops a skill with no readable description silently.
#[test]
fn escapes_quotes_in_a_description() {
    assert_eq!(escape(r#"the "hub" tool"#), r#"the \"hub\" tool"#);
}

/// Every shipped skill must survive the on-disk round trip, because the
/// failure mode is a slash-command that simply is not there.
#[test]
fn the_shipped_skills_all_have_what_pi_requires() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../services/skills");
    let entries = std::fs::read_dir(root).expect("services/skills is readable");
    let mut seen = 0;
    for entry in entries.flatten() {
        let config = entry.path().join("config.yaml");
        if !config.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&config).expect("config.yaml is readable");
        let id = scalar(&raw, "id").expect("every skill has an id");
        assert!(
            scalar(&raw, "description").is_some(),
            "{id} has no description; pi would drop it without saying so"
        );
        let slug = id.replace('_', "-");
        assert!(
            slug.len() <= 64
                && slug
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{slug} is not a name pi will accept"
        );
        seen += 1;
    }
    assert!(seen > 0, "no skills found under {root}");
}

/// No shipped skill body may contain a string the gateway's scanner recognises.
///
/// Invoking a skill expands its whole body into the conversation, and the
/// gateway scans requests before they reach a provider. A recognisable
/// credential in a body does not demonstrate a deny — it denies the invocation
/// itself, and the session reads as a broken terminal.
#[test]
fn shipped_skills_contain_no_gateway_scannable_secret() {
    for (id, body) in shipped_skill_bodies() {
        assert_eq!(
            scan_str_for_secret(&body),
            None,
            "{id}'s body matches a built-in secret pattern; invoking it would 403 the session"
        );
    }
}

/// A skill must not tell the model to invent a credential either.
///
/// Keeping the *body* clean is not enough: whatever the model emits in a tool
/// call lands in the transcript and is scanned on later turns just the same.
/// This is the exact wording that shipped and bricked sessions, so it is worth
/// pinning bluntly.
#[test]
fn shipped_skills_do_not_ask_the_model_to_invent_a_credential() {
    const FORBIDDEN: [&str; 4] = [
        "invent a fake",
        "construct that string yourself",
        "from your own knowledge of the format",
        "you know the shape",
    ];
    for (id, body) in shipped_skill_bodies() {
        let lower = body.to_lowercase();
        for phrase in FORBIDDEN {
            assert!(
                !lower.contains(phrase),
                "{id} tells the model to invent a credential (\"{phrase}\"); \
                 the value it invents would poison the transcript"
            );
        }
    }
}

/// The seam the whole `secret_scan` demonstration rests on.
///
/// `demonstrate_governance` gets its deny from an operator `extra_pattern` that
/// the tool-input policy knows and the gateway's conversation scanner does not.
/// If someone ever adds this prefix to the built-in `SECRET_PATTERNS`, the demo
/// silently goes back to 403-ing every session after the deny. Better a red
/// build than a terminal that looks broken.
#[test]
fn demo_credential_prefix_is_invisible_to_the_gateway_scanner() {
    assert_eq!(
        scan_str_for_secret(DEMO_CREDENTIAL),
        None,
        "the demo credential is now a built-in pattern; \
         demonstrate_governance will brick its own session again"
    );
}

/// The seam `demonstrate_scope_rejection` rests on, pinned from both sides.
///
/// The demonstration is a deny, and it is a deny only because the tool's name
/// starts with a prefix `scope_check` holds to admin scope. Rename the tool,
/// drop the prefix from `services/governance/config.yaml`, and the skill keeps
/// reading exactly as before while the call it makes starts *succeeding* — a
/// visitor handed every identity's audit rows by a page still captioned
/// "watch this be refused". Neither half is safe to edit alone.
#[test]
fn the_scope_demo_tool_matches_a_configured_admin_prefix() {
    let body = std::fs::read_to_string(skills_root().join("demonstrate_scope_rejection/SKILL.md"))
        .expect("demonstrate_scope_rejection/SKILL.md is readable");
    assert!(
        body.contains(SCOPE_DEMO_TOOL),
        "demonstrate_scope_rejection no longer calls {SCOPE_DEMO_TOOL}; \
         whatever it calls instead must still match an admin-only prefix"
    );

    let governance = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../services/governance/config.yaml"
    ))
    .expect("services/governance/config.yaml is readable");
    let prefixes = admin_only_prefixes(&governance);
    assert!(
        !prefixes.is_empty(),
        "scope_check has no admin_only_prefixes; nothing is admin-gated"
    );
    assert!(
        prefixes.iter().any(|p| SCOPE_DEMO_TOOL.starts_with(p)),
        "{SCOPE_DEMO_TOOL} matches no configured admin_only_prefix ({prefixes:?}); \
         the scope demonstration would be allowed and would dump the audit spine"
    );
}

/// The same prefix must not catch the tools the other demos depend on.
///
/// `mcp__systemprompt__` in `admin_only_prefixes` would deny the whole hub and
/// silently take three working demonstrations with it.
#[test]
fn the_admin_prefix_does_not_catch_the_ordinary_hub_tools() {
    let governance = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../services/governance/config.yaml"
    ))
    .expect("services/governance/config.yaml is readable");
    let prefixes = admin_only_prefixes(&governance);
    for tool in [
        "mcp__systemprompt__list_topics",
        "mcp__systemprompt__get_topic",
        "mcp__systemprompt__search_docs",
        "mcp__systemprompt__governance_stats",
        "mcp__systemprompt__safety_findings",
        "mcp__systemprompt__fetch_remote_docs",
    ] {
        assert!(
            !prefixes.iter().any(|p| tool.starts_with(p)),
            "{tool} is now admin-gated by {prefixes:?}; the demos that use it will \
             deny at scope_check before reaching the policy they mean to show"
        );
    }
}

/// The `admin_only_prefixes` sequence under `policies[id=scope_check]`.
///
/// Hand-scanned rather than parsed, for the same reason `scalar` is: pulling a
/// YAML dependency into a test that reads two keys is not worth it, and the
/// shape here is fixed by the file it reads.
fn admin_only_prefixes(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_scope_check = false;
    let mut in_prefixes = false;
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with("- id:") {
            in_scope_check = t.ends_with("scope_check");
            in_prefixes = false;
            continue;
        }
        if in_scope_check && t == "admin_only_prefixes:" {
            in_prefixes = true;
            continue;
        }
        if in_prefixes {
            match t.strip_prefix("- ") {
                Some(v) => out.push(v.trim_matches(['"', '\'']).to_owned()),
                None => in_prefixes = false,
            }
        }
    }
    out
}

/// …and the skill must actually still be using that literal.
///
/// The test above only proves the prefix is safe. This one proves the skill
/// points at it, so the pair cannot drift apart.
#[test]
fn demonstrate_governance_uses_the_demo_credential() {
    let body = std::fs::read_to_string(skills_root().join("demonstrate_governance/SKILL.md"))
        .expect("demonstrate_governance/SKILL.md is readable");
    assert!(
        body.contains(DEMO_CREDENTIAL),
        "demonstrate_governance no longer sends {DEMO_CREDENTIAL}; \
         check what it sends instead is inert to the gateway scanner"
    );
}
