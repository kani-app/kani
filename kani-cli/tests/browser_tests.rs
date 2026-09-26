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
fn browser_payload_fixture_validates() {
    let _ = load_and_validate("browser_payload.yaml");
}

#[test]
fn browser_payload_schema_parses_via_field() {
    let path = Path::new("tests/fixtures/browser_payload.yaml");
    let src = std::fs::read_to_string(path).unwrap();
    let ext: YamlExtension = serde_yaml::from_str(&src).unwrap();
    let details = ext.endpoints.manga_details.as_ref().unwrap();
    assert_eq!(
        details.via,
        Some(kani_cli::yaml::schema::EndpointVia::BrowserPayload)
    );
    assert_eq!(
        details.page_url.as_deref(),
        Some("https://example.com/manga/$manga_id$")
    );
    assert_eq!(details.script.as_deref(), Some("fetch_popular"));
    assert_eq!(details.timeout_ms, 15000);
    assert_eq!(details.auto_scroll, Some(false));
}

#[test]
fn browser_payload_codegen_emits_capture_page_payload() {
    let validated = load_and_validate("browser_payload.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("capture_page_payload"),
        "browser endpoint must emit capture_page_payload call: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("SCRIPT_FETCH_POPULAR"),
        "browser endpoint must reference the script constant: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("15000"),
        "browser endpoint must embed the timeout: {}",
        generated.lib_rs
    );
    assert!(generated.lib_rs.contains("capture_page_payload_configured"));
    let capture = generated
        .lib_rs
        .find("capture_page_payload_configured")
        .expect("configured capture call");
    let capture = &generated.lib_rs[capture..generated.lib_rs.len().min(capture + 500)];
    assert!(
        capture.contains("false"),
        "disabled auto-scroll must be emitted: {capture}"
    );
}

#[test]
fn browser_payload_auto_scroll_defaults_to_false_after_validation() {
    let validated = load_and_validate("browser_payload_full.yaml");
    assert!(!validated.manga_details.as_ref().unwrap().auto_scroll);
}

#[test]
fn browser_payload_full_covers_all_endpoint_kinds() {
    let validated = load_and_validate("browser_payload_full.yaml");
    let generated = codegen::generate(&validated, false);
    let src = &generated.lib_rs;

    assert!(
        !src.contains("unimplemented"),
        "browser codegen must not emit unimplemented!(): {src}"
    );
    assert_eq!(
        src.matches("capture_page_payload").count(),
        5,
        "expected 5 capture_page_payload calls: {src}"
    );
    assert!(
        src.contains("v8_context"),
        "must call the guest v8_context::capture_page_payload wrapper: {src}"
    );
    assert_eq!(
        src.matches("JsonHandle").count(),
        5,
        "each browser endpoint must parse its payload via JsonHandle::parse: {src}"
    );
    for method in [
        "get_popular_manga",
        "search_manga",
        "get_manga_details",
        "get_chapter_list",
        "get_pages",
    ] {
        assert!(src.contains(method), "missing method {method}: {src}");
    }
}

#[test]
fn browser_payload_codegen_emits_script_static() {
    let validated = load_and_validate("browser_payload.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.lib_rs.contains("include_str!"),
        "browser scripts must emit include_str! static: {}",
        generated.lib_rs
    );
    assert!(
        generated.lib_rs.contains("scripts/fetch_popular.js"),
        "script static must reference src/scripts/fetch_popular.js: {}",
        generated.lib_rs
    );
}

#[test]
fn browser_payload_generated_crate_has_script() {
    let validated = load_and_validate("browser_payload.yaml");
    let generated = codegen::generate(&validated, false);
    assert!(
        generated.browser_scripts.contains_key("fetch_popular"),
        "GeneratedCrate.browser_scripts must include fetch_popular"
    );
}

#[test]
fn validate_browser_requires_page_url() {
    let path = Path::new("tests/fixtures/browser_payload.yaml");
    let src = std::fs::read_to_string(path).unwrap();
    let mut ext: YamlExtension = serde_yaml::from_str(&src).unwrap();
    ext.endpoints.manga_details.as_mut().unwrap().page_url = None;
    let result = validate::validate(&ext, &src, path);
    assert!(result.is_err(), "missing page_url must fail validation");
    let errs = result.err().unwrap();
    assert!(
        errs.iter().any(|e| e.to_string().contains("page_url")),
        "error must mention page_url: {errs:?}"
    );
}

#[test]
fn validate_browser_requires_declared_script() {
    let path = Path::new("tests/fixtures/browser_payload.yaml");
    let src = std::fs::read_to_string(path).unwrap();
    let mut ext: YamlExtension = serde_yaml::from_str(&src).unwrap();
    ext.endpoints.manga_details.as_mut().unwrap().script = Some("nonexistent_script".to_string());
    let result = validate::validate(&ext, &src, path);
    assert!(result.is_err(), "undeclared script must fail validation");
    let errs = result.err().unwrap();
    assert!(
        errs.iter()
            .any(|e| e.to_string().contains("nonexistent_script")),
        "error must mention the undeclared script name: {errs:?}"
    );
}
