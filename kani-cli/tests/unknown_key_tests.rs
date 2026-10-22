#![allow(clippy::unwrap_used)]

//! A misspelt key must be refused, or the setting it carries silently does nothing. Each case
//! puts one unknown key somewhere the schema reads and asserts the error names it and its line.

use std::path::Path;

const BASE: &str = r#"id: typo-test
name: TypoTest
version: "0.1.0"
base_url: "https://example.com"
"#;

fn error_for(extra: &str) -> String {
    let src = format!("{BASE}{extra}");
    match kani_yaml::parse_and_validate(&src, Path::new("typo.yaml")) {
        Ok(_) => panic!("expected an unknown-key error for:\n{extra}"),
        Err(errors) => errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "),
    }
}

fn assert_names(error: &str, field: &str, line: usize) {
    assert!(
        error.contains(&format!("unknown field `{field}`"))
            && error.contains(&format!("at line {line} ")),
        "expected `{field}` reported at line {line}, got: {error}"
    );
}

#[test]
fn a_misspelt_top_level_key_is_refused() {
    assert_names(&error_for("endpionts: {}\n"), "endpionts", 5);
}

#[test]
fn a_misspelt_key_in_a_popular_endpoint_is_refused() {
    let error = error_for(
        "endpoints:\n  popular:\n    route: \"/p\"\n    contianer: \".item\"\n    fields:\n      \
         id: 'dom(\".id\").text()'\n      title: 'dom(\".t\").text()'\n",
    );
    assert_names(&error, "contianer", 6);
}

#[test]
fn a_misspelt_key_beside_delegate_to_is_refused() {
    let error = error_for(
        "endpoints:\n  popular:\n    delegate_to: search\n    empty_without_filter: true\n",
    );
    assert_names(&error, "empty_without_filter", 6);
}

#[test]
fn a_misspelt_key_in_a_field_is_refused() {
    let error = error_for(
        "endpoints:\n  search:\n    route: \"/s\"\n    fields:\n      id:\n        \
         expr: 'dom(\".id\").text()'\n        optinal: true\n      title: 'dom(\".t\").text()'\n",
    );
    assert_names(&error, "optinal", 9);
}

#[test]
fn a_misspelt_key_in_a_sort_pair_mapping_is_refused() {
    let error = error_for(
        "endpoints:\n  search:\n    route: \"/s\"\n    filter_mapping:\n      sort:\n        \
         kind: sort_pair\n        key_templat: \"order[{}]\"\n    fields:\n      \
         id: 'dom(\".id\").text()'\n      title: 'dom(\".t\").text()'\n",
    );
    assert_names(&error, "key_templat", 9);
}

#[test]
fn a_misspelt_key_in_a_fetched_option_set_is_refused() {
    let error = error_for("option_sets:\n  genres:\n    options_fetch_by: {}\n");
    assert_names(&error, "options_fetch_by", 6);
}

#[test]
fn the_legacy_for_each_concurrency_key_is_still_accepted() {
    let src = format!(
        "{BASE}endpoints:\n  search:\n    route: \"/s\"\n    fields:\n      \
         id: 'dom(\".id\").text()'\n      title: 'dom(\".t\").text()'\n    for_each:\n      \
         - endpoint: manga_details\n        url_expr: 'dom(\".l\").attr(\"href\")'\n        \
         merge_as: details\n        concurrency: 4\n  manga_details:\n    route: \"/m/$manga_id$\"\n    \
         fields:\n      id: 'dom(\".id\").text()'\n      title: 'dom(\".t\").text()'\n      \
         status: 'dom(\".s\").text()'\n"
    );
    kani_yaml::parse_and_validate(&src, Path::new("legacy.yaml")).unwrap();
}
