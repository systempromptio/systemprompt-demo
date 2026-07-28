use systemprompt_web_admin::repositories::analytics::conversations::redact_text;

#[test]
fn redact_aws_key() {
    let (out, n) = redact_text("here is AKIAIOSFODNN7EXAMPLE in text");
    assert_eq!(n, 1);
    assert!(out.contains("[REDACTED:aws_access_key]"));
    assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn redact_anthropic_key() {
    let (out, n) = redact_text("call sk-ant-api03-abc and also AIzaSyAbCdEfG please");
    assert_eq!(n, 2);
    assert!(out.contains("[REDACTED:anthropic_api_key]"));
    assert!(out.contains("[REDACTED:google_api_key]"));
}

#[test]
fn redact_prefixless_high_entropy_key() {
    let (out, n) = redact_text("PHL+ERIbxzlQOeiiRybQwgV7GvYmIclsJe1zsFIyuuM here is my api key");
    assert_eq!(n, 1);
    assert!(out.contains("[REDACTED:high_entropy_token]"));
    assert!(!out.contains("PHL+ERIbxzlQ"));
}

#[test]
fn redact_leaves_shas_and_uuids_alone() {
    let input = "commit c0196f2a4b8d9e1f2a3b4c5d6e7f8091a2b3c4d5 trace 03f06137-5eb1-4ed9-9b0b-ee6899baa5fa";
    let (out, n) = redact_text(input);
    assert_eq!(n, 0);
    assert_eq!(out, input);
}

#[test]
fn redact_no_op_on_clean_text() {
    let (out, n) = redact_text("hello world, no secrets here");
    assert_eq!(n, 0);
    assert_eq!(out, "hello world, no secrets here");
}
