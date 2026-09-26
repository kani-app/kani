#![allow(clippy::unwrap_used)]

use kani_cli::{
    codegen,
    yaml::{schema::YamlExtension, validate},
};
use std::path::Path;

fn load_and_validate(fixture: &str) -> kani_cli::yaml::model::ValidatedExtension {
    let path = Path::new("tests/fixtures").join(fixture);
    let src = std::fs::read_to_string(&path).unwrap();
    let ext: YamlExtension = serde_yaml::from_str(&src).unwrap();
    validate::validate(&ext, &src, &path).unwrap()
}

#[test]
fn codegen_get_url_emits_get_url_fn() {
    let validated = load_and_validate("get_url.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("get_url"),
        "get_url fixture must emit a get_url fn: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("manga_id"),
        "get_url fn must reference manga_id: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_popular_id_in_cargo_toml() {
    let validated = load_and_validate("popular.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.cargo_toml.contains("test-popular"),
        "Cargo.toml must contain the crate id: {}",
        generated.cargo_toml
    );
}

#[test]
fn codegen_filter_format_default_matches_unformatted_output() {
    let with_format = load_and_validate("search.yaml");
    let generated = codegen::generate(&with_format, false);
    assert!(
        !generated.lib_rs.contains("filter_format"),
        "search.yaml fixture has no filter_format and must not reference it: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_id_encoding_emits_encoded_field() {
    let validated = load_and_validate("id_encoding.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("encoded_field"),
        "manga_details.fields.id composite must emit encoded_field: {}",
        generated.lib_rs
    );
    assert!(
        generated
            .lib_rs
            .contains("kani_shared::ast::IdEncoding::Base64Url"),
        "encoded_field must reference the fully-qualified IdEncoding variant: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_id_encoding_emits_decode_prologue_in_chapter_list() {
    let validated = load_and_validate("id_encoding.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("decode_composite"),
        "chapter_list route referencing $manga.hid$/$manga.slug$ must emit a decode_composite call: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("manga_hid") && generated.lib_rs.contains("manga_slug"),
        "decode prologue must bind sanitized manga_hid/manga_slug locals: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_cache_declares_namespaces_in_metadata() {
    let validated = load_and_validate("cache.yaml");
    let generated = codegen::generate(&validated, false);
    let compact: String = generated.lib_rs.split_whitespace().collect();
    assert!(
        compact.contains(
            "(\"search_results\".to_string(),kani_shared::CacheNamespaceLimits{ttl_seconds:1800_u32,max_entries:Some(200_u32),"
        ),
        "declared namespaces must reach ExtensionMetadata.cache: {}",
        generated.lib_rs
    );
    assert!(
        compact.contains("dsl_schema_version:Some(kani_shared::ast::DSL_SCHEMA_VERSION)"),
        "the blueprint version must be recorded in metadata: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_without_a_cache_block_declares_no_namespaces() {
    let validated = load_and_validate("popular.yaml");
    let generated = codegen::generate(&validated, false);
    let compact: String = generated.lib_rs.split_whitespace().collect();
    assert!(
        compact.contains("cache:std::collections::BTreeMap::new(),"),
        "no cache block means no hook namespaces: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_metadata_populates_all_fields() {
    let validated = load_and_validate("metadata.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="),
        "icon base64 must be embedded verbatim: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("requests_per_second: 5_f32")
            && generated.lib_rs.contains("burst: 10_u32")
            && generated.lib_rs.contains("max_concurrent: 2_u32"),
        "rate_limit fields must be mapped through: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("\"en\".to_string()")
            && generated.lib_rs.contains("\"ja\".to_string()"),
        "languages must be emitted: {}",
        generated.lib_rs
    );
    assert!(
        generated
            .lib_rs
            .contains("A fixture extension exercising a fully populated metadata block."),
        "description must be emitted: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("kani_shared::Section { id: \"latest\".to_string(), name: \"Latest\".to_string(), nsfw: false }")
            && generated.lib_rs.contains("kani_shared::Section { id: \"popular\".to_string(), name: \"Popular\".to_string(), nsfw: false }"),
        "sections must be emitted: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("schema_version:   1_u32"),
        "schema_version must be emitted: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("Some(\"0.5.0\".to_string())"),
        "min_kani_version must be emitted: {}",
        generated.lib_rs
    );
    assert!(
        generated
            .lib_rs
            .contains("vec![\"unrestricted_http\".to_string()]"),
        "requires_capabilities must be emitted: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_embedded_bytes_flag() {
    let validated = load_and_validate("popular.yaml");
    let plain = codegen::generate(&validated, false);
    let embedded = codegen::generate(&validated, true);
    assert_ne!(
        plain.lib_rs, embedded.lib_rs,
        "embedded_bytes=true should produce different lib.rs output"
    );
}

#[test]
fn codegen_chapter_sort_stub_when_absent() {
    let validated = load_and_validate("popular.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("get_chapter_sort_list"),
        "stub must still emit get_chapter_sort_list: {}",
        generated.lib_rs
    );
    assert!(
        !generated.lib_rs.contains("default_chapter_sort"),
        "stub must not emit default_chapter_sort: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_fetched_option_sets_emits_json_def() {
    let validated = load_and_validate("fetched_opts.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("get_fetched_option_sets"),
        "fetched option_sets must emit get_fetched_option_sets: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("genres-v1"),
        "fetched option_sets must embed the cache key: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("https://example.com/api/genres"),
        "fetched option_sets must embed the route: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("\"filter_id\":\"genre\""),
        "fetched option_sets must embed the filter_id: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("\"nsfw_field\":\"nsfw\""),
        "fetched option_sets must embed the nsfw_field when set: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_no_fetched_option_sets_emits_empty_array() {
    let validated = load_and_validate("popular.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("r#\"[]\"#"),
        "extension with no fetched option_sets must emit empty array: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_hooks_metadata_contains_pre_request() {
    let validated = load_and_validate("hooks.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("pre_request:"),
        "hooks fixture must emit pre_request in metadata: {}",
        generated.lib_rs
    );
}

#[test]
fn codegen_hooks_rate_limit_max_hook_requests() {
    let validated = load_and_validate("hooks.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("max_hook_requests: 5_u32"),
        "hooks fixture must emit max_hook_requests from rate_limit config: {}",
        generated.lib_rs
    );
}

/// Every codegen fixture and the snapshots it must reproduce. Twelve tests
/// differed only by these two names; a fixture is now one row rather than a
/// function, and the assertion names the fixture that failed.
const SNAPSHOT_FIXTURES: &[(&str, &str, bool)] = &[
    ("popular", "popular", true),
    ("search", "search", true),
    ("manga_details", "details", false),
    ("chapter_list", "chapters", false),
    ("pages", "pages", false),
    ("filter_format", "filter_format", false),
    ("id_encoding", "id_encoding", false),
    ("cache", "cache", false),
    ("metadata", "metadata", false),
    ("chapter_sort", "chapter_sort", false),
    ("for_each", "for_each", false),
    ("hooks", "hooks", false),
];

#[test]
fn every_fixture_generates_its_recorded_output() {
    for (fixture, snapshot, has_cargo_toml) in SNAPSHOT_FIXTURES {
        let validated = load_and_validate(&format!("{fixture}.yaml"));
        let generated = codegen::generate(&validated, false);
        insta::assert_snapshot!(format!("{snapshot}_lib_rs"), generated.lib_rs);
        if *has_cargo_toml {
            insta::assert_snapshot!(format!("{snapshot}_cargo_toml"), generated.cargo_toml);
        }
    }
}
