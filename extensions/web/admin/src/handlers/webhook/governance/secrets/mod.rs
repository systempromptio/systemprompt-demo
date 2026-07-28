//! Built-in plaintext secret-pattern registry.
//!
//! Each pattern has three pieces:
//! - `id`: a stable kebab-case identifier — the persistent referent written
//!   into `governance_decisions.evaluated_rules.pattern_id` and used by
//!   dashboards / SQL aggregations; it must never change.
//! - `name`: human-readable label rendered in deny messages.
//! - `expr`: an anchored regular expression that triggers the match. Shapes are
//!   seeded from the gitleaks (MIT) ruleset: a bare vendor prefix in prose
//!   ("keys start with sk-ant-") passes, a full-length credential denies.
//!
//! Patterns are backstopped by [`find_high_entropy_token`]: a credential with
//! no recognisable vendor prefix — a random base64 blob pasted into a prompt —
//! matches no pattern but still reads as machine-generated key material, and is
//! reported under the pseudo-pattern id `high-entropy-token`.

mod patterns;

use std::sync::LazyLock;

use regex::Regex;

pub(super) use patterns::SecretPattern;
use patterns::{HIGH_ENTROPY_PATTERN, SECRET_PATTERNS};

static COMPILED: LazyLock<Vec<(usize, Regex)>> = LazyLock::new(|| {
    SECRET_PATTERNS
        .iter()
        .enumerate()
        .filter_map(|(i, p)| match Regex::new(p.expr) {
            Ok(re) => Some((i, re)),
            // Why: a test pins COMPILED.len() == SECRET_PATTERNS.len(), so this
            // arm is a per-pattern guard for the release binary only.
            Err(e) => {
                tracing::error!(pattern_id = %p.id, error = %e, "secret pattern disabled: regex failed to compile");
                None
            },
        })
        .collect()
});

const ENTROPY_MIN_LEN: usize = 32;

// Why: bits per character. Random base64 of 32+ chars sits around 4.4-4.8;
// English-ish identifiers stay under 4.0.
const ENTROPY_THRESHOLD: f64 = 4.0;

fn shannon_entropy(s: &str) -> f64 {
    let mut counts = [0u32; 256];
    let bytes = s.as_bytes();
    for &b in bytes {
        counts[usize::from(b)] += 1;
    }
    let len = bytes.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = f64::from(c) / len;
            -p * p.log2()
        })
        .sum()
}

// Why: the mixed-class requirement (upper AND lower AND digit) is what keeps
// git SHAs, UUIDs and hex digests out; relaxing it reintroduces those false
// positives.
pub(crate) fn find_high_entropy_token(text: &str) -> Option<&str> {
    text.split(|c: char| c.is_whitespace() || "\"'`()[]{}<>,;:".contains(c))
        .find(|token| {
            token.len() >= ENTROPY_MIN_LEN
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+/=_-".contains(c))
                && token.chars().any(|c| c.is_ascii_uppercase())
                && token.chars().any(|c| c.is_ascii_lowercase())
                && token.chars().any(|c| c.is_ascii_digit())
                && shannon_entropy(token) >= ENTROPY_THRESHOLD
        })
}

// JSON: the scanner walks arbitrary tool input — generic by nature
fn collect_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(arr) => {
            for v in arr {
                collect_strings(v, out);
            }
        },
        serde_json::Value::Object(map) => {
            for v in map.values() {
                collect_strings(v, out);
            }
        },
        _ => {},
    }
}

fn redacted_snippet(s: &str, match_start: usize) -> String {
    let mut snippet_end = (match_start + 12).min(s.len());
    while !s.is_char_boundary(snippet_end) {
        snippet_end -= 1;
    }
    format!("{}...[REDACTED]", &s[match_start..snippet_end])
}

fn scan_str(s: &str) -> Option<(&'static SecretPattern, String)> {
    for (i, re) in COMPILED.iter() {
        if let Some(m) = re.find(s) {
            return Some((&SECRET_PATTERNS[*i], redacted_snippet(s, m.start())));
        }
    }
    find_high_entropy_token(s).map(|token| {
        let start = token.as_ptr() as usize - s.as_ptr() as usize;
        (&HIGH_ENTROPY_PATTERN, redacted_snippet(s, start))
    })
}

// Why: Shares [`SECRET_PATTERNS`] with the governance webhook so the gateway
// safety scanner and the tool-use governor flag the same credentials.
pub fn scan_str_for_secret(text: &str) -> Option<String> {
    scan_str(text).map(|(_, redacted)| redacted)
}

pub(super) fn detect_secrets(
    tool_input: Option<&serde_json::Value>,
) -> Option<(&'static SecretPattern, String)> {
    let input = tool_input?;

    let mut strings = Vec::new();
    collect_strings(input, &mut strings);

    for s in &strings {
        if let Some(hit) = scan_str(s) {
            return Some(hit);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Why: the paste that got through in production — a 43-char base64 blob
    // with no vendor prefix.
    #[test]
    fn a_prefixless_base64_key_is_caught_by_the_entropy_detector() {
        let text = "PHL+ERIbxzlQOeiiRybQwgV7GvYmIclsJe1zsFIyuuM here is my api key";
        let hit = scan_str(text);
        assert!(
            matches!(
                &hit,
                Some((pattern, redacted))
                    if pattern.id == "high-entropy-token"
                        && redacted.ends_with("...[REDACTED]")
                        && !redacted.contains("FIyuuM")
            ),
            "entropy detector must fire with truncated evidence, got {hit:?}"
        );
    }

    #[test]
    fn every_builtin_pattern_compiles() {
        assert_eq!(COMPILED.len(), SECRET_PATTERNS.len());
    }

    // Why: fixtures are assembled at runtime so no credential-shaped literal
    // exists in the source — GitHub push protection scans this file too.
    #[test]
    fn full_length_vendor_keys_match_their_patterns() {
        let mailgun = format!("key-{}", "0123456789abcdef".repeat(2));
        let cases = [
            ("AKIAIOSFODNN7EXAMPLE", "aws-access-key"),
            (
                "ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789",
                "github-token-classic",
            ),
            ("sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv", "anthropic-api-key"),
            (mailgun.as_str(), "mailgun-api-key"),
            (
                "postgresql://admin:hunter2@db.internal:5432/prod",
                "postgres-url-with-password",
            ),
        ];
        for (text, id) in cases {
            let hit = scan_str(text);
            assert!(
                matches!(&hit, Some((pattern, _)) if pattern.id == id),
                "{id} should match {text}, got {hit:?}"
            );
        }
    }

    #[test]
    fn prose_fragments_and_benign_urls_do_not_match() {
        for text in [
            "keys start with sk-ant- and AKIA is the AWS marker",
            "connect to redis://localhost:6379 and mysql://db.local/app",
            "the SG. abbreviation, a key-value store, and password= as a concept",
            "postgresql://readonly@db.internal/metrics",
        ] {
            assert!(scan_str(text).is_none(), "false positive on: {text}");
        }
    }

    #[test]
    fn shas_uuids_and_identifiers_do_not_trip_the_entropy_detector() {
        for text in [
            "commit c0196f2a4b8d9e1f2a3b4c5d6e7f8091a2b3c4d5 on main",
            "trace 03f06137-5eb1-4ed9-9b0b-ee6899baa5fa completed",
            "call list_requests_paged_with_total_and_filters_applied please",
            "https://example.com/docs/getting-started/installation-guide-linux",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA1aB",
        ] {
            assert!(scan_str(text).is_none(), "false positive on: {text}");
        }
    }

    // Why: the demonstrate_governance asymmetry (services/governance/config.yaml)
    // needs the demo credential invisible here; its all-caps shape keeps it
    // under the entropy detector's mixed-class bar by construction.
    #[test]
    fn the_demo_credential_stays_invisible() {
        assert!(scan_str("SPDEMOKEY-0000000000000000").is_none());
    }
}
