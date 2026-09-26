#![allow(clippy::unwrap_used)]

use kani_cli::{
    commands::build::apply_factory_overrides,
    yaml::{
        schema::{FactorySource, YamlExtension},
        validate,
    },
};
use std::collections::BTreeMap;
use std::path::Path;

fn load_factory_yaml(fixture: &str) -> (YamlExtension, String) {
    let path = Path::new("tests/fixtures").join(fixture);
    let src = std::fs::read_to_string(&path).unwrap();
    let ext: YamlExtension = serde_yaml::from_str(&src).unwrap();
    (ext, src)
}

fn make_source(id: &str, name: &str, base_url: &str, lang: &str) -> FactorySource {
    FactorySource {
        id: id.to_string(),
        name: name.to_string(),
        base_url: base_url.to_string(),
        language: lang.to_string(),
        mihon_source_id: None,
        overrides: BTreeMap::new(),
    }
}

#[test]
fn factory_schema_parses_sources() {
    let (ext, _) = load_factory_yaml("factory.yaml");
    let factory = ext.factory.expect("factory.yaml must have a factory block");
    assert_eq!(factory.sources.len(), 2);
    assert_eq!(factory.sources[0].id, "factory-alpha");
    assert_eq!(factory.sources[1].id, "factory-beta");
    assert_eq!(factory.sources[1].language, "ja");
}

#[test]
fn factory_schema_has_override_entry() {
    let (ext, _) = load_factory_yaml("factory.yaml");
    let factory = ext.factory.unwrap();
    let beta = &factory.sources[1];
    assert!(
        beta.overrides.contains_key("endpoints.search.route"),
        "beta source must have an endpoints.search.route override"
    );
}

#[test]
fn apply_factory_overrides_sets_top_level_fields() {
    let (_, src) = load_factory_yaml("factory.yaml");
    let base: serde_yaml::Value = serde_yaml::from_str(&src).unwrap();
    let source = make_source("new-id", "New Name", "https://new.example.com", "fr");

    let result = apply_factory_overrides(base, &source);
    let result_ext: YamlExtension = serde_yaml::from_value(result).unwrap();

    assert_eq!(result_ext.id, "new-id");
    assert_eq!(result_ext.name, "New Name");
    assert_eq!(result_ext.base_url, "https://new.example.com");
    assert_eq!(result_ext.language, "fr");
}

#[test]
fn apply_factory_overrides_dot_path_sets_nested_value() {
    let (_, src) = load_factory_yaml("factory.yaml");
    let base: serde_yaml::Value = serde_yaml::from_str(&src).unwrap();
    let mut source = make_source("beta", "Beta", "https://beta.example.com", "ja");
    source.overrides.insert(
        "endpoints.search.route".to_string(),
        serde_yaml::Value::String("$base_url$/find?q=$query$".to_string()),
    );

    let result = apply_factory_overrides(base, &source);
    let result_ext: YamlExtension = serde_yaml::from_value(result).unwrap();

    let search_route = result_ext
        .endpoints
        .search
        .as_ref()
        .and_then(|e| e.route.as_deref())
        .unwrap();
    assert_eq!(search_route, "$base_url$/find?q=$query$");
}

#[test]
fn validate_factory_rejects_empty_sources() {
    use kani_cli::yaml::schema::FactoryBlock;
    let block = FactoryBlock {
        template: None,
        sources: vec![],
    };
    let errors = validate::validate_factory(&block);
    assert!(
        !errors.is_empty(),
        "empty sources must produce a validation error"
    );
    assert!(errors[0].to_string().contains("must not be empty"));
}

#[test]
fn validate_factory_rejects_duplicate_ids() {
    use kani_cli::yaml::schema::FactoryBlock;
    let block = FactoryBlock {
        template: None,
        sources: vec![
            make_source("dup", "First", "https://a.com", "en"),
            make_source("dup", "Second", "https://b.com", "en"),
        ],
    };
    let errors = validate::validate_factory(&block);
    assert!(
        errors.iter().any(|e| e.to_string().contains("duplicate")),
        "duplicate source ids must produce a validation error: {errors:?}"
    );
}

#[test]
fn validate_factory_rejects_empty_base_url() {
    use kani_cli::yaml::schema::FactoryBlock;
    let block = FactoryBlock {
        template: None,
        sources: vec![make_source("ok-id", "Name", "", "en")],
    };
    let errors = validate::validate_factory(&block);
    assert!(
        errors.iter().any(|e| e.to_string().contains("base_url")),
        "empty base_url must produce a validation error: {errors:?}"
    );
}

#[test]
fn factory_yaml_validates_cleanly() {
    let (ext, src) = load_factory_yaml("factory.yaml");
    let path = Path::new("tests/fixtures/factory.yaml");
    let factory = ext.factory.as_ref().expect("factory block required");
    let factory_errors = validate::validate_factory(factory);
    assert!(
        factory_errors.is_empty(),
        "factory.yaml must have no factory errors: {factory_errors:?}"
    );

    for source_def in &factory.sources {
        let base: serde_yaml::Value = serde_yaml::from_str(&src).unwrap();
        let expanded = apply_factory_overrides(base, source_def);
        let expanded_src = serde_yaml::to_string(&expanded).unwrap();
        let expanded_ext: YamlExtension = serde_yaml::from_value(
            serde_yaml::from_str::<serde_yaml::Value>(&expanded_src).unwrap(),
        )
        .unwrap();
        let result = validate::validate(&expanded_ext, &expanded_src, path);
        assert!(
            result.is_ok(),
            "source '{}' must validate cleanly: {:?}",
            source_def.id,
            result.err()
        );
    }
}

#[test]
fn factory_build_rejects_interpreted_only_features() {
    let dir = tempfile::tempdir().unwrap();
    let factory = dir.path().join("dedup-factory.yaml");
    std::fs::write(
        &factory,
        r#"id: dedup-template
name: DedupTemplate
version: "0.1.0"
base_url: "https://example.com"
endpoints:
  search:
    route: "/search"
    container: ".item"
    fields:
      id: 'self.first(".id").text()'
      title: 'self.first(".title").text()'
    for_each:
      - endpoint: manga_details
        url_expr: 'self.first(".link").attr("href")'
        merge_as: details
        deduplicate_by: 'self.ptr("/details/id").str()'
  manga_details:
    route: "/manga/$manga_id$"
    container: ":root"
    fields:
      id: '"$manga_id$"'
      title: 'dom(".title").text()'
      status: '"unknown"'
factory:
  sources:
    - id: dedup-alpha
      name: Alpha
      base_url: "https://alpha.example.com"
"#,
    )
    .unwrap();
    let ext_root = dir.path().join("extensions");
    let out_dir = dir.path().join("out");
    std::fs::create_dir_all(&ext_root).unwrap();

    let err = kani_cli::commands::build::run(
        Some(factory.to_str().unwrap()),
        false,
        false,
        None,
        Some(ext_root.to_str().unwrap()),
        Some(out_dir.to_str().unwrap()),
        false,
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("deduplicate_by"),
        "refused for the interpreted-only key, got: {err}"
    );
    assert!(
        !ext_root.join("kani-dedup-alpha").exists(),
        "no crate may be generated for a refused source"
    );
}
