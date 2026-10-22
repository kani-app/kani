#![allow(clippy::unwrap_used)]

//! Every REST route refuses an unauthenticated caller.
//!
//! This replaces the per-endpoint `…_returns_401_without_auth` tests. Those were
//! one test per route, each asserting a single status code, so they cost a test
//! apiece and still only covered the routes somebody remembered to write one for.
//! Driving the assertion from the router's own route table inverts that: a new
//! endpoint is covered the moment it is mounted, and forgetting to guard one is a
//! failure rather than a silent gap.
//!
//! A route that should be reachable without a session has to be named in
//! [`PUBLIC`] with a reason, which is the list worth reviewing in a security pass.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::routes::{concrete_path, declared_routes};
use common::{build_test_app, test_state};
use tower::ServiceExt;

/// Routes that answer an anonymous caller successfully, and why. Every entry is a
/// deliberate hole in the guard, so this list is the one to read in a security pass.
const PUBLIC: &[(&str, &str, &str)] = &[
    // Establishing or recovering a session cannot itself require one.
    ("/rest/auth/login", "post", "mints the session"),
    (
        "/rest/auth/logout",
        "post",
        "idempotent; no session to end is not an error",
    ),
    (
        "/rest/auth/setup-state",
        "get",
        "drives first-run routing before any account exists",
    ),
    (
        "/rest/auth/captcha",
        "get",
        "shown on the unauthenticated login form",
    ),
    (
        "/rest/auth/password-reset-enabled",
        "get",
        "the login form asks before offering the link",
    ),
    (
        "/rest/auth/registration-enabled",
        "get",
        "the login form asks before offering sign-up",
    ),
    // Version and first-run flags drive the pre-login UI.
    (
        "/rest/system/info",
        "get",
        "feature flags the login screen needs",
    ),
];

fn public_reason(path: &str, method: &str) -> Option<&'static str> {
    if let Some((_, _, why)) = PUBLIC.iter().find(|(p, m, _)| *p == path && *m == method) {
        return Some(why);
    }
    // OPDS and passkeys authenticate themselves rather than by session cookie.
    if path.starts_with("/rest/opds") {
        Some("HTTP Basic, not a session cookie")
    } else if path.starts_with("/rest/auth/passkey/") {
        Some("WebAuthn challenge flow")
    } else {
        None
    }
}

fn request(method: &str, path: &str) -> Request<Body> {
    let builder = Request::builder()
        .method(method.to_uppercase().as_str())
        .uri(path);
    match method {
        "post" | "put" | "patch" => builder
            .header("Content-Type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
        _ => builder.body(Body::empty()).unwrap(),
    }
}

#[tokio::test]
async fn every_route_refuses_an_unauthenticated_caller() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let routes = declared_routes();
    assert!(
        routes.len() > 200,
        "the route scan found only {} routes, so it is not parsing the modules",
        routes.len()
    );

    let mut served = Vec::new();
    let mut inconclusive = Vec::new();
    let mut checked = 0usize;

    for ((path, method), module) in &routes {
        if public_reason(path, method).is_some() {
            continue;
        }
        let res = app
            .clone()
            .oneshot(request(method, &concrete_path(path)))
            .await
            .unwrap();

        checked += 1;
        let status = res.status();
        let line = format!(
            "  {} {path} -> {status} ({module}.rs)",
            method.to_uppercase()
        );

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::METHOD_NOT_ALLOWED {
            // 405 means the scan inferred a method the router does not serve,
            // which openapi_coverage_tests already polices.
            continue;
        }
        if status.is_success() || status.is_redirection() {
            served.push(line);
        } else {
            // A 4xx that is not 401 means an extractor rejected the placeholder
            // body or path before the handler consulted the session. That does not
            // prove the route is unguarded, so it is reported rather than failed.
            inconclusive.push(line);
        }
    }

    served.sort();
    inconclusive.sort();
    assert!(
        checked > 150,
        "only {checked} routes were checked, so the exemption list is swallowing the surface"
    );
    assert!(
        served.is_empty(),
        "{} route(s) served an unauthenticated caller. Either the guard is missing, or the \
         route is deliberately public and belongs in PUBLIC with a reason:\n{}\n\n\
         (for context, {} route(s) answered a non-401 4xx, where an extractor rejected the \
         synthetic request before auth was consulted)",
        served.len(),
        served.join("\n"),
        inconclusive.len(),
    );
}

#[test]
fn the_public_list_names_only_routes_that_exist() {
    let routes = declared_routes();

    // A stale exemption protects nothing and quietly covers for the next route
    // that lands on the same path.
    let mut phantom: Vec<String> = PUBLIC
        .iter()
        .filter(|(path, method, _)| {
            !routes.contains_key(&((*path).to_string(), (*method).to_string()))
        })
        .map(|(path, method, _)| format!("  {} {path}", method.to_uppercase()))
        .collect();

    phantom.sort();
    assert!(
        phantom.is_empty(),
        "the public exemption list names {} route(s) the router no longer serves:\n{}",
        phantom.len(),
        phantom.join("\n")
    );
}
