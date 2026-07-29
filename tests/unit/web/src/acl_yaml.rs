//! Parsing of the bootstrap access-control YAML.
//!
//! This runs on every server start, so what it accepts and rejects decides
//! whether a typo becomes a loud startup failure or a silently missing grant.
//! Two asymmetries are deliberate and neither is visible from the types: a
//! blank file is an empty document rather than an error, so commenting a file
//! out is a no-op; and the document tolerates unknown top-level keys — it must,
//! because the shipped file still carries the `rules:` key that moved into core
//! — while a department entry does not, so a misspelled field there is caught
//! rather than dropped.

use systemprompt_web_admin::repositories::config::acl_yaml_loader::parse_yaml_doc;
use systemprompt_web_admin::repositories::config::acl_yaml_types::DepartmentsDoc;

const SHIPPED_DEPARTMENTS: &str =
    include_str!("../../../../services/access-control/departments.yaml");

const REL: &str = "access-control/departments.yaml";

fn parse(yaml: &str) -> Result<DepartmentsDoc, String> {
    parse_yaml_doc::<DepartmentsDoc>(REL, yaml).map_err(|e| e.to_string())
}

#[test]
fn reads_departments_with_descriptions() {
    let doc = parse(
        "departments:\n  - name: Platform\n    description: Owns the gateway\n  - name: Support\n",
    )
    .unwrap();

    assert_eq!(doc.departments.len(), 2);
    assert_eq!(doc.departments[0].name, "Platform");
    assert_eq!(doc.departments[0].description, "Owns the gateway");
    assert_eq!(doc.departments[1].name, "Support");
    assert_eq!(doc.departments[1].description, "");
}

#[test]
fn a_blank_file_is_an_empty_document() {
    for blank in ["", "   ", "\n\n", "\t\n  \n"] {
        let doc = parse(blank).unwrap();
        assert!(doc.departments.is_empty(), "blank input {blank:?} parsed");
    }
}

#[test]
fn a_comment_only_file_is_an_empty_document() {
    // Not the blank-input branch — this reaches serde, which reads a
    // document of only comments as null and `DepartmentsDoc` as all-default.
    let doc = parse("# every department was removed\n").unwrap();
    assert!(doc.departments.is_empty());
}

#[test]
fn an_absent_departments_key_is_an_empty_list() {
    let doc = parse("rules: []\n").unwrap();
    assert!(doc.departments.is_empty());
}

#[test]
fn an_empty_departments_list_is_accepted() {
    let doc = parse("departments: []\n").unwrap();
    assert!(doc.departments.is_empty());
}

#[test]
fn an_unknown_top_level_key_is_ignored() {
    // `rules:` was removed from this extension's schema in core 0.12.0 but is
    // still present in shipped files; rejecting it would break those boots.
    let doc = parse("departments:\n  - name: Platform\nrules: []\nfuture_key: 3\n").unwrap();
    assert_eq!(doc.departments.len(), 1);
    assert_eq!(doc.departments[0].name, "Platform");
}

#[test]
fn an_unknown_department_key_is_rejected() {
    let err = parse("departments:\n  - name: Platform\n    desription: typo\n").unwrap_err();
    assert!(
        err.contains("desription"),
        "error did not name the key: {err}"
    );
}

#[test]
fn a_department_without_a_name_is_rejected() {
    let err = parse("departments:\n  - description: no name here\n").unwrap_err();
    assert!(err.contains("name"), "error did not name the field: {err}");
}

#[test]
fn malformed_yaml_is_rejected_and_names_the_file() {
    for broken in [
        "departments:\n  - name: [unclosed\n",
        "departments:\n\t- name: tabs are not indentation\n",
        "departments: : :\n",
    ] {
        let err = parse(broken).unwrap_err();
        assert!(
            err.contains(REL),
            "error for {broken:?} did not name the file: {err}"
        );
    }
}

#[test]
fn a_scalar_where_a_list_belongs_is_rejected() {
    let err = parse("departments: Platform\n").unwrap_err();
    assert!(err.contains(REL), "{err}");
}

#[test]
fn the_shipped_departments_file_parses() {
    let doc = parse(SHIPPED_DEPARTMENTS).unwrap();
    for dept in &doc.departments {
        assert!(
            !dept.name.trim().is_empty(),
            "shipped department has no name"
        );
    }
}
