//! The Handlebars helpers the admin pages render through.
//!
//! These run against a registry built by `register_helpers`, so a helper
//! renamed in Rust but not re-registered fails here rather than silently
//! rendering nothing on a dashboard. The formatting itself is worth pinning
//! because it is the only place several display decisions exist: thresholds
//! that switch a cost between three and five decimals, a truncation that
//! measures bytes to decide but characters to cut, and fallbacks that must
//! produce a dash rather than an empty cell.

use handlebars::Handlebars;
use serde_json::{Value, json};
use systemprompt_web_admin::templates::helpers::register_helpers;

fn render(template: &str, data: &Value) -> String {
    let mut hbs = Handlebars::new();
    register_helpers(&mut hbs);
    hbs.render_template(template, data)
        .unwrap_or_else(|e| panic!("render of {template:?} failed: {e}"))
}

fn call(helper: &str, args: &[Value]) -> String {
    let params: Vec<String> = (0..args.len()).map(|i| format!("a{i}")).collect();
    let data: Value = params
        .iter()
        .zip(args)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<serde_json::Map<_, _>>()
        .into();
    render(&format!("{{{{{helper} {}}}}}", params.join(" ")), &data)
}

#[test]
fn every_registered_helper_resolves() {
    // A bare `{{name}}` parses as a variable and would render empty either
    // way; giving it an argument forces resolution as a helper.
    let mut hbs = Handlebars::new();
    register_helpers(&mut hbs);
    for name in [
        "formatDate",
        "formatNumber",
        "relativeTime",
        "initials",
        "truncate",
        "json",
        "concat",
        "toLowerCase",
        "toUpperCase",
        "default",
        "governanceColor",
        "css_version",
        "eq",
        "gt",
        "not",
        "add",
        "sub",
        "formatUsd",
        "percent",
        "deltaPct",
        "shortId",
    ] {
        let rendered = hbs.render_template(&format!("{{{{{name} 1 2}}}}"), &json!({}));
        assert!(
            rendered.is_ok(),
            "helper {name} is not registered: {:?}",
            rendered.unwrap_err()
        );
    }
}

#[test]
fn format_number_groups_thousands_and_abbreviates_above_a_million() {
    assert_eq!(call("formatNumber", &[json!(0)]), "0");
    assert_eq!(call("formatNumber", &[json!(999)]), "999");
    assert_eq!(call("formatNumber", &[json!(1_000)]), "1,000");
    assert_eq!(call("formatNumber", &[json!(999_999)]), "999,999");
    assert_eq!(call("formatNumber", &[json!(1_500_000)]), "1.5M");
    assert_eq!(call("formatNumber", &[json!(2_500_000_000i64)]), "2.5B");
}

#[test]
fn format_number_keeps_the_sign_outside_the_grouping() {
    assert_eq!(call("formatNumber", &[json!(-1_234)]), "-1,234");
    assert_eq!(call("formatNumber", &[json!(-12_345_678)]), "-12.3M");
}

#[test]
fn format_number_reads_a_missing_or_non_numeric_value_as_zero() {
    assert_eq!(call("formatNumber", &[json!(null)]), "0");
    assert_eq!(call("formatNumber", &[json!("not a number")]), "0");
    assert_eq!(render("{{formatNumber}}", &json!({})), "0");
}

#[test]
fn format_usd_widens_the_precision_as_the_amount_shrinks() {
    // Micro-dollars in; the thresholds exist so a sub-cent per-call cost is
    // still legible next to a three-figure monthly total.
    assert_eq!(call("formatUsd", &[json!(250_000_000i64)]), "$250");
    assert_eq!(call("formatUsd", &[json!(12_340_000i64)]), "$12.34");
    assert_eq!(call("formatUsd", &[json!(123_400i64)]), "$0.123");
    assert_eq!(call("formatUsd", &[json!(1_234i64)]), "$0.00123");
    assert_eq!(call("formatUsd", &[json!(0)]), "$0.00000");
}

#[test]
fn format_usd_renders_an_absent_cost_as_a_dash() {
    // Distinguishes "no cost recorded" from "cost was zero".
    assert_eq!(call("formatUsd", &[json!(null)]), "—");
    assert_eq!(render("{{formatUsd}}", &json!({})), "—");
}

#[test]
fn percent_scales_a_fraction_to_one_decimal() {
    assert_eq!(call("percent", &[json!(0.0)]), "0.0%");
    assert_eq!(call("percent", &[json!(0.5)]), "50.0%");
    assert_eq!(call("percent", &[json!(1.0)]), "100.0%");
    assert_eq!(call("percent", &[json!(0.12345)]), "12.3%");
    assert_eq!(call("percent", &[json!(null)]), "0.0%");
}

#[test]
fn delta_pct_signs_a_rise_and_omits_an_undefined_comparison() {
    assert_eq!(call("deltaPct", &[json!(150), json!(100)]), "+50% vs prev");
    assert_eq!(call("deltaPct", &[json!(50), json!(100)]), "-50% vs prev");
    assert_eq!(call("deltaPct", &[json!(100), json!(100)]), "0% vs prev");
    // A zero baseline has no percentage; the cell stays blank rather than
    // showing an infinity.
    assert_eq!(call("deltaPct", &[json!(10), json!(0)]), "");
    assert_eq!(call("deltaPct", &[json!(10), json!(null)]), "");
}

#[test]
fn initials_take_the_first_two_word_starts() {
    assert_eq!(call("initials", &[json!("Ada Lovelace")]), "AL");
    assert_eq!(call("initials", &[json!("ada")]), "A");
    assert_eq!(call("initials", &[json!("Ada Byron King Lovelace")]), "AB");
}

#[test]
fn initials_split_on_email_and_handle_punctuation() {
    // Users are often only known by an email or a login, so the separators
    // that matter are not just whitespace.
    assert_eq!(call("initials", &[json!("ada.lovelace@example.com")]), "AL");
    assert_eq!(call("initials", &[json!("ada_byron")]), "AB");
    assert_eq!(call("initials", &[json!("ada-byron")]), "AB");
}

#[test]
fn initials_fall_back_to_a_question_mark() {
    assert_eq!(call("initials", &[json!("")]), "?");
    assert_eq!(call("initials", &[json!("...")]), "?");
    assert_eq!(call("initials", &[json!(null)]), "?");
}

#[test]
fn truncate_appends_an_ellipsis_only_when_it_cuts() {
    assert_eq!(call("truncate", &[json!("short"), json!(10)]), "short");
    assert_eq!(call("truncate", &[json!("exactly10!"), json!(10)]), "exactly10!");
    assert_eq!(
        call("truncate", &[json!("this one is far too long"), json!(8)]),
        "this one..."
    );
}

#[test]
fn truncate_never_splits_a_character() {
    // The length test is in bytes but the cut is in characters, so a
    // multibyte string can be shortened further than the limit suggests —
    // never into invalid UTF-8.
    let out = call("truncate", &[json!("ααααα"), json!(4)]);
    assert_eq!(out, "αααα...");
    assert!(out.chars().count() <= 8);
}

#[test]
fn short_id_clips_to_a_character_count() {
    let trace = "0193f2a1-6c4d-7e8f-9a0b-1c2d3e4f5a6b";
    assert_eq!(call("shortId", &[json!(trace), json!(8)]), "0193f2a1");
    assert_eq!(call("shortId", &[json!(trace)]), "0193f2a1-6c4");
    assert_eq!(call("shortId", &[json!("abc"), json!(99)]), "abc");
    assert_eq!(call("shortId", &[json!(null)]), "");
}

#[test]
fn case_helpers_leave_non_strings_empty() {
    assert_eq!(call("toUpperCase", &[json!("deny")]), "DENY");
    assert_eq!(call("toLowerCase", &[json!("DENY")]), "deny");
    assert_eq!(call("toUpperCase", &[json!(42)]), "");
    assert_eq!(call("toLowerCase", &[json!(null)]), "");
}

#[test]
fn concat_joins_mixed_scalars_and_skips_nulls() {
    assert_eq!(
        call("concat", &[json!("run-"), json!(7), json!("-"), json!(true)]),
        "run-7-true"
    );
    assert_eq!(call("concat", &[json!("a"), json!(null), json!("b")]), "ab");
    assert_eq!(render("{{concat}}", &json!({})), "");
}

#[test]
fn governance_color_maps_every_decision_synonym() {
    for allow in ["allow", "pass", "ok", "ALLOW"] {
        assert_eq!(call("governanceColor", &[json!(allow)]), "success", "{allow}");
    }
    for warn in ["flag", "warn", "warning", "review"] {
        assert_eq!(call("governanceColor", &[json!(warn)]), "warning", "{warn}");
    }
    for deny in ["deny", "block", "denied", "fail", "error", "DENIED"] {
        assert_eq!(call("governanceColor", &[json!(deny)]), "danger", "{deny}");
    }
}

#[test]
fn an_unrecognised_decision_is_neutral_not_a_denial() {
    // Colouring an unknown decision red would report a denial that never
    // happened.
    assert_eq!(call("governanceColor", &[json!("escalated")]), "neutral");
    assert_eq!(call("governanceColor", &[json!("")]), "neutral");
    assert_eq!(call("governanceColor", &[json!(null)]), "neutral");
}

#[test]
fn default_substitutes_only_for_empty_and_false_values() {
    assert_eq!(call("default", &[json!("set"), json!("—")]), "set");
    assert_eq!(call("default", &[json!(""), json!("—")]), "—");
    assert_eq!(call("default", &[json!(null), json!("—")]), "—");
    assert_eq!(call("default", &[json!(false), json!("—")]), "—");
    // Zero is a real measurement, not a missing one.
    assert_eq!(call("default", &[json!(0), json!("—")]), "0");
}

#[test]
fn json_escapes_markup_so_embedded_data_cannot_close_the_script_tag() {
    let out = call("json", &[json!({ "note": "</script><img src=x>" })]);
    assert!(!out.contains("</script>"), "{out}");
    assert!(out.contains("&lt;/script&gt;"), "{out}");
    assert!(out.contains("&lt;img src=x&gt;"), "{out}");
}

#[test]
fn json_renders_an_absent_value_as_null() {
    assert_eq!(render("{{json}}", &json!({})), "null");
}

#[test]
fn format_date_passes_through_what_it_cannot_parse() {
    assert_eq!(call("formatDate", &[json!("")]), "-");
    assert_eq!(call("formatDate", &[json!("-")]), "-");
    assert_eq!(call("formatDate", &[json!(null)]), "-");
    assert_eq!(call("formatDate", &[json!("not a date")]), "not a date");
}

#[test]
fn format_date_accepts_both_stored_timestamp_shapes() {
    // Postgres hands back a naive timestamp; the API hands back RFC 3339.
    // Both must render, and to the same instant.
    let from_rfc = call("formatDate", &[json!("2024-03-05T14:30:00Z")]);
    let from_naive = call("formatDate", &[json!("2024-03-05T14:30:00")]);
    assert_eq!(from_rfc, from_naive);
    assert!(from_rfc.contains("2024"), "{from_rfc}");
    assert_ne!(from_rfc, "2024-03-05T14:30:00Z");
}

#[test]
fn relative_time_buckets_by_magnitude() {
    let now = chrono::Utc::now();
    let ago = |mins: i64| {
        call(
            "relativeTime",
            &[json!((now - chrono::Duration::minutes(mins)).to_rfc3339())],
        )
    };
    assert_eq!(ago(0), "just now");
    assert_eq!(ago(5), "5m ago");
    assert_eq!(ago(60 * 3), "3h ago");
    assert_eq!(ago(60 * 24 * 2), "2d ago");
}

#[test]
fn relative_time_falls_back_to_an_absolute_date_past_a_month() {
    let old = chrono::Utc::now() - chrono::Duration::days(400);
    let out = call("relativeTime", &[json!(old.to_rfc3339())]);
    assert!(!out.ends_with("d ago"), "{out}");
    assert!(out.contains(&old.format("%Y").to_string()), "{out}");
}

#[test]
fn relative_time_passes_through_what_it_cannot_parse() {
    assert_eq!(call("relativeTime", &[json!("")]), "-");
    assert_eq!(call("relativeTime", &[json!(null)]), "-");
    assert_eq!(call("relativeTime", &[json!("whenever")]), "whenever");
}
