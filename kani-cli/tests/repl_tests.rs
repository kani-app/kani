#![allow(clippy::unwrap_used)]

use kani_cli::repl::{explain, test_cmd};
use std::path::Path;

fn fixture(name: &str) -> String {
    Path::new("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn inspect_repl_test_yaml() {
    kani_cli::repl::inspect::run(&fixture("repl-test.yaml")).unwrap();
}

#[test]
fn explain_trim_chain_has_steps() {
    let trace = explain::explain(r#"self.first("a").text().trim()"#).unwrap();
    assert!(
        trace.len() >= 3,
        "expected ≥3 trace steps, got {}",
        trace.len()
    );
    assert_eq!(
        trace.steps[0].expr_kind, "Trim",
        "outermost step should be Trim"
    );
    let last = trace.steps.last().unwrap();
    assert_eq!(last.expr_kind, "Self", "innermost step should be Self");
}

#[test]
fn explain_longer_chain() {
    let trace = explain::explain(r#"self.first("a").attr("href").split("/").at(-1)"#).unwrap();
    assert!(
        trace.len() >= 3,
        "expected ≥3 trace steps, got {}",
        trace.len()
    );
    assert_eq!(trace.steps[0].expr_kind, "At");
    assert_eq!(trace.steps.last().unwrap().expr_kind, "Self");
}

#[test]
fn explain_display_formatting() {
    let trace = explain::explain(r#"self.text().trim()"#).unwrap();
    let rendered = trace.to_string();
    assert!(rendered.contains("[Trim]"), "expected [Trim] in output");
    assert!(rendered.contains("[Text]"), "expected [Text] in output");
    assert!(rendered.contains("[Self]"), "expected [Self] in output");
    assert!(
        rendered.contains("  [Text]"),
        "Text should be indented under Trim"
    );
}

#[test]
fn test_command_counts_rows() {
    test_cmd::run_test(
        &fixture("repl-test.yaml"),
        &fixture("repl-test.har"),
        "popular",
        3,
        None,
    )
    .unwrap();
}

#[test]
fn test_command_wrong_count_fails() {
    let result = test_cmd::run_test(
        &fixture("repl-test.yaml"),
        &fixture("repl-test.har"),
        "popular",
        99,
        None,
    );
    assert!(result.is_err(), "should fail with wrong expected count");
}

#[test]
fn replay_matches_expected() {
    test_cmd::run_replay(
        &fixture("repl-test.yaml"),
        &fixture("repl-test.har"),
        "popular",
        &fixture("expected-popular.json"),
        None,
    )
    .unwrap();
}

/// The request a source would send is the thing a fixture cannot check, so it
/// gets asserted directly: declared queries, filter defaults, the multiselect
/// shape, and the pagination offset all have to appear.
#[test]
fn resolve_builds_the_request_the_host_would_send() {
    let (ext, ep) = test_cmd::load_endpoint(&fixture("resolve-request.yaml"), "popular").unwrap();
    let resolved = kani_cli::repl::resolve::resolve(
        &ext,
        &ep,
        "popular",
        &["page=3".to_string()],
        &["tags=action,drama".to_string()],
    )
    .unwrap();

    assert_eq!(
        resolved.url(),
        "https://api.example.com/items?from=page-3&order=score&rating=safe&tags[]=action&tags[]=drama&page=3"
    );
}

#[test]
fn resolve_applies_declared_filter_defaults_without_overrides() {
    let (ext, ep) = test_cmd::load_endpoint(&fixture("resolve-request.yaml"), "popular").unwrap();
    let resolved =
        kani_cli::repl::resolve::resolve(&ext, &ep, "popular", &["page=1".to_string()], &[])
            .unwrap();

    let rating: Vec<_> = resolved
        .request
        .queries
        .iter()
        .filter(|(k, _)| k == "rating")
        .collect();
    assert_eq!(
        rating,
        vec![&("rating".to_string(), "safe".to_string())],
        "the declared default was not applied: {:?}",
        resolved.request.queries
    );
}
