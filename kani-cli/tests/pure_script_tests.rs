#![allow(clippy::unwrap_used)]

use kani_cli::{
    codegen,
    yaml::{schema::YamlExtension, validate},
};
use std::path::Path;

fn errors_of(
    result: Result<kani_cli::yaml::model::ValidatedExtension, Vec<kani_yaml::YamlError>>,
) -> Vec<String> {
    result
        .err()
        .expect("validation must reject this fixture")
        .iter()
        .map(std::string::ToString::to_string)
        .collect()
}

fn validate_str(
    yaml: &str,
) -> Result<kani_cli::yaml::model::ValidatedExtension, Vec<kani_yaml::YamlError>> {
    let path = Path::new("test.yaml");
    let ext: YamlExtension = serde_yaml::from_str(yaml).expect("fixture must parse as YAML");
    validate::validate(&ext, yaml, path)
}

fn load_and_validate(fixture: &str) -> kani_cli::yaml::model::ValidatedExtension {
    let path = Path::new("tests/fixtures").join(fixture);
    let src = std::fs::read_to_string(&path).unwrap();
    let ext: YamlExtension = serde_yaml::from_str(&src).unwrap();
    validate::validate(&ext, &src, &path).unwrap()
}

#[test]
fn pure_scripts_fixture_validates() {
    let _ = load_and_validate("pure_scripts.yaml");
}

#[test]
fn pure_scripts_pass_through_to_validated() {
    let validated = load_and_validate("pure_scripts.yaml");
    assert!(
        validated.pure_scripts.contains_key("slug"),
        "pure_scripts must contain 'slug': {:?}",
        validated.pure_scripts.keys().collect::<Vec<_>>()
    );
    assert!(
        validated.pure_scripts.contains_key("clean_title"),
        "pure_scripts must contain 'clean_title': {:?}",
        validated.pure_scripts.keys().collect::<Vec<_>>()
    );
}

#[test]
fn pure_scripts_codegen_emits_include_str() {
    let validated = load_and_validate("pure_scripts.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated
            .lib_rs
            .contains("include_str!(\"scripts/slug.rhai\")"),
        "generated lib.rs must reference slug.rhai: {}",
        generated.lib_rs
    );
    assert!(
        generated
            .lib_rs
            .contains("include_str!(\"scripts/clean_title.rhai\")"),
        "generated lib.rs must reference clean_title.rhai: {}",
        generated.lib_rs
    );
}

#[test]
fn pure_scripts_codegen_emits_scripts_field_in_metadata() {
    let validated = load_and_validate("pure_scripts.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("scripts:"),
        "generated lib.rs must have scripts field: {}",
        generated.lib_rs
    );
    assert!(
        generated
            .lib_rs
            .contains("std::collections::BTreeMap::from"),
        "generated lib.rs must use BTreeMap::from for non-empty scripts: {}",
        generated.lib_rs
    );
}

#[test]
fn pure_scripts_in_generated_crate() {
    let validated = load_and_validate("pure_scripts.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.pure_scripts.contains_key("slug"),
        "GeneratedCrate.pure_scripts must contain 'slug'"
    );
    assert!(
        generated.pure_scripts.contains_key("clean_title"),
        "GeneratedCrate.pure_scripts must contain 'clean_title'"
    );
}

#[test]
fn pure_scripts_codegen_empty_map_for_no_scripts() {
    let validated = validate_str(
        r#"
id: no-scripts
name: NoScripts
version: "0.1.0"
base_url: "https://example.com"
"#,
    )
    .unwrap();
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("scripts:")
            && generated
                .lib_rs
                .contains("std::collections::BTreeMap::new()"),
        "generated lib.rs must emit empty BTreeMap for extensions without scripts: {}",
        generated.lib_rs
    );
}

#[test]
fn validate_pure_script_syntax_error_rejected() {
    let result = validate_str(
        r#"
id: bad-script
name: BadScript
version: "0.1.0"
base_url: "https://example.com"
scripts:
  pure:
    bad_fn: "fn bad_fn(s) { s.to_upper( // missing paren"
"#,
    );
    assert!(
        result.is_err(),
        "syntax error in pure script must fail validation"
    );
    let errs = result.err().unwrap();
    assert!(
        errs.iter().any(|e| e.to_string().contains("bad_fn")),
        "error must mention the function name: {:?}",
        errs
    );
}

#[test]
fn validate_pure_script_empty_name_rejected() {
    let result = validate_str(
        r#"
id: empty-name
name: EmptyName
version: "0.1.0"
base_url: "https://example.com"
scripts:
  pure:
    "": "fn foo(s) { s }"
"#,
    );
    // A fixture can fail validation for any reason, so the message has to name the
    // rule under test — otherwise this passes when that rule is gone.
    let messages = errors_of(result);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("scripts.pure: function name must not be empty")),
        "the empty-name rule must be what rejects it, got: {messages:?}"
    );
}

#[test]
fn validate_pure_script_empty_body_rejected() {
    let result = validate_str(
        r#"
id: empty-body
name: EmptyBody
version: "0.1.0"
base_url: "https://example.com"
scripts:
  pure:
    my_fn: ""
"#,
    );
    let messages = errors_of(result);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("script source must not be empty")),
        "the empty-body rule must be what rejects it, got: {messages:?}"
    );
}
