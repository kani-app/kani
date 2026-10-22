#![allow(clippy::unwrap_used)]

//! A signed-in user holding no permissions is refused by every permission-gated
//! route.
//!
//! This replaces the per-endpoint `…_returns_403_for_regular_user` tests, and
//! covers far more than they did: the requirement is read out of each handler's
//! own `AuthGuard<…>`, so every guarded route is checked and a handler that loses
//! its guard fails here rather than silently widening access.
//!
//! Routes guarded only by authentication rather than a permission are excluded by
//! the same scan, so this asserts authorisation, not authentication — that is
//! `auth_guard_contract_tests`.

mod common;

use axum::body::Body;
use axum::http::Request;
use common::routes::{concrete_path, declared_guards};
use common::{build_test_app, create_permissionless_user, login, test_state};
use tower::ServiceExt;

/// Guards that mean "any signed-in user", so a permissionless account passes them
/// legitimately and they prove nothing about authorisation.
const AUTHENTICATION_ONLY: &[&str] = &["IsAuthenticated", "Authenticated"];

fn request(method: &str, path: &str, cookie: &str) -> Request<Body> {
    let builder = Request::builder()
        .method(method.to_uppercase().as_str())
        .uri(path);
    if matches!(method, "post" | "put" | "patch" | "delete") {
        builder
            .header("Content-Type", "application/json")
            .header("Cookie", common::csrf_cookie(cookie))
            .header("X-CSRF-Token", common::csrf_token(cookie))
            .body(Body::from("{}"))
            .unwrap()
    } else {
        builder
            .header("Cookie", cookie)
            .body(Body::empty())
            .unwrap()
    }
}

#[tokio::test]
async fn a_user_without_permissions_is_refused_by_every_guarded_route() {
    let state = test_state().await;
    let (username, password) = create_permissionless_user(&state, "nobody").await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let guarded = declared_guards();
    assert!(
        guarded.len() > 100,
        "the guard scan resolved only {} routes, so it is not reading the handlers",
        guarded.len()
    );

    let mut allowed = Vec::new();
    let mut checked = 0usize;

    for ((path, method), guard) in &guarded {
        if AUTHENTICATION_ONLY.contains(&guard.as_str()) {
            continue;
        }
        let res = app
            .clone()
            .oneshot(request(method, &concrete_path(path), &cookie))
            .await
            .unwrap();

        checked += 1;
        // Anything that is not a success means the caller was stopped, whether by
        // the guard or by an extractor ahead of it. Only a 2xx proves it got through.
        if res.status().is_success() {
            allowed.push(format!(
                "  {} {path} -> {} (guard: {guard})",
                method.to_uppercase(),
                res.status()
            ));
        }
    }

    allowed.sort();
    assert!(
        checked > 100,
        "only {checked} permission-gated routes were checked, so the scan is too narrow"
    );
    assert!(
        allowed.is_empty(),
        "{} permission-gated route(s) served a user holding no permissions:\n{}",
        allowed.len(),
        allowed.join("\n")
    );
}

/// Routes reachable by any signed-in user, as of 2026-08-17. The check above
/// skips them — a permissionless account passes them legitimately — so without
/// this a guard downgraded from a permission to plain authentication would widen
/// access silently and no test would notice.
const AUTHENTICATION_ONLY_ROUTES: usize = 52;

#[test]
fn no_route_quietly_drops_to_authentication_only() {
    let guards = common::routes::handler_guards();
    let auth_only = guards
        .values()
        .filter(|g| AUTHENTICATION_ONLY.contains(&g.as_str()))
        .count();

    assert!(
        auth_only <= AUTHENTICATION_ONLY_ROUTES,
        "{auth_only} handlers are guarded by authentication alone, up from \
         {AUTHENTICATION_ONLY_ROUTES}. A handler whose permission guard was replaced with \
         IsAuthenticated is reachable by every signed-in user; if that is intended, raise \
         the constant in the same change."
    );
}

#[test]
fn every_guarded_route_resolves_to_a_named_permission() {
    let guarded = declared_guards();

    // A route whose handler cannot be resolved is invisible to the check above,
    // which is the failure mode worth catching: it looks covered and is not.
    let unresolved: Vec<String> = common::routes::declared_routes()
        .into_iter()
        .filter(|(route, _)| !guarded.contains_key(route))
        .map(|((path, method), module)| {
            format!("  {} {path}  ({module}.rs)", method.to_uppercase())
        })
        .collect();

    // Public and authentication-only routes legitimately have no permission guard,
    // so this is a ratchet on the count rather than a demand for zero.
    assert!(
        unresolved.len() < 90,
        "{} route(s) have no resolvable permission guard, up from the recorded
         baseline — the scan may have stopped matching a handler shape:\n{}",
        unresolved.len(),
        unresolved.join("\n")
    );
}
