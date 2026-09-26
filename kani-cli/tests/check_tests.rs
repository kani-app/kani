#![allow(clippy::unwrap_used)]

//! `kani-cli check` must refuse what the server would refuse, so each case asserts the reason
//! rather than only that something objected.

use std::path::{Path, PathBuf};

use kani_cli::commands::check::problems;

const VALID: &str = r#"id: check-source
name: CheckSource
version: "0.1.0"
base_url: "https://example.com"

cache:
  auth:
    ttl: 3600

scripts:
  pure:
    bearer: |
      fn bearer(token) { "Bearer " + token }

pre_request: |
  let token = ctx.cache_get("auth", "token");
  if token != () {
    req.set_header("Authorization", bearer(token));
  }
  proceed()

on_status:
  "401": |
    ctx.cache_put("auth", "token", "fresh", 0);
    retry()
"#;

fn yaml(dir: &tempfile::TempDir, src: &str) -> PathBuf {
    let path = dir.path().join("extension.yaml");
    std::fs::write(&path, src).unwrap();
    path
}

fn yaml_problems(src: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    problems(&yaml(&dir, src)).unwrap()
}

fn assert_one_problem(found: &[String], needle: &str) {
    assert!(
        found.len() == 1 && found[0].contains(needle),
        "expected one problem mentioning {needle:?}, got {found:?}"
    );
}

#[test]
fn a_valid_yaml_extension_with_hooks_passes() {
    assert_eq!(yaml_problems(VALID), Vec::<String>::new());
}

#[test]
fn a_hook_calling_an_undefined_function_is_refused() {
    let src = VALID.replace("ctx.cache_put(", "ctx.cache_store(");
    assert_one_problem(&yaml_problems(&src), "`cache_store`");
}

#[test]
fn a_hook_using_an_undeclared_cache_namespace_is_refused() {
    let src = VALID.replace("cache:\n  auth:\n    ttl: 3600\n", "");
    let found = yaml_problems(&src);
    assert!(
        found.len() == 2
            && found
                .iter()
                .all(|p| p.contains("cache namespace 'auth'") && p.contains("does not declare")),
        "expected pre_request and on_status to report 'auth', got {found:?}"
    );
}

#[test]
fn a_reserved_id_is_refused() {
    let src = VALID.replace("id: check-source", "id: example");
    assert_one_problem(&yaml_problems(&src), "reserved");
}

#[test]
fn a_newer_min_kani_version_is_refused() {
    let src = VALID.replace(
        "version: \"0.1.0\"\n",
        "version: \"0.1.0\"\nmin_kani_version: \"99.0.0\"\n",
    );
    assert_one_problem(&yaml_problems(&src), "requires kani >= 99.0.0");
}

#[test]
fn an_unknown_capability_is_refused() {
    let src = VALID.replace(
        "version: \"0.1.0\"\n",
        "version: \"0.1.0\"\nrequires_capabilities: [\"teleport\"]\n",
    );
    assert_one_problem(&yaml_problems(&src), "unsupported capability 'teleport'");
}

fn workspace_file(parts: &[&str]) -> PathBuf {
    let mut path = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    for part in parts {
        path.push(part);
    }
    path
}

#[test]
fn the_fixture_wasm_extension_passes() {
    let path = workspace_file(&["..", "wasm_sources", "fixture.wasm"]);
    assert!(
        path.exists(),
        "wasm_sources/fixture.wasm: cargo run -p kani-cli -- build kani-fixture-source"
    );
    assert_eq!(problems(&path).unwrap(), Vec::<String>::new());
}

#[test]
fn a_truncated_wasm_file_is_refused() {
    let whole = std::fs::read(workspace_file(&["..", "wasm_sources", "fixture.wasm"])).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("truncated.wasm");
    std::fs::write(&path, &whole[..whole.len() / 2]).unwrap();
    assert_one_problem(&problems(&path).unwrap(), "not a loadable WASM component");
}

#[test]
fn a_component_importing_what_kani_lacks_is_refused() {
    let path = workspace_file(&["tests", "fixtures", "check", "unlinked-import.wasm"]);
    assert_one_problem(&problems(&path).unwrap(), "kani:extension/nope");
}
