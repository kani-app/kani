#![allow(clippy::unwrap_used)]

//! First-run setup. The endpoint that creates the instance's first account is
//! reachable without a session, so its two gates — an empty user table and a
//! local caller — are the whole security model and are pinned here.

mod common;
use axum::http::StatusCode;
use common::{body_json, build_test_app, create_admin, post_json, test_state};
use tower::ServiceExt;

/// `KANI_ALLOW_REMOTE_SETUP` is read from the process environment, and cargo
/// runs every test in this file on threads of one process. Without a lock, a
/// test that sets it races the test asserting the default refusal.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn a_fresh_instance_reports_that_it_needs_setup() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app
        .oneshot(common::get_req("/rest/auth/setup-state"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(
        body.get("needs_setup").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[tokio::test]
async fn an_instance_with_an_account_does_not() {
    let state = test_state().await;
    create_admin(&state).await;
    let app = build_test_app(state).await;

    let res = app
        .oneshot(common::get_req("/rest/auth/setup-state"))
        .await
        .unwrap();
    let body = body_json(res).await;
    assert_eq!(
        body.get("needs_setup").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[tokio::test]
async fn setup_refuses_a_caller_whose_address_is_unknown() {
    let _env = ENV_LOCK.lock().await;
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app
        .oneshot(post_json(
            "/rest/auth/setup",
            serde_json::json!({
                "username": "owner",
                "email": "owner@example.com",
                "password": "OwnerPassword123!"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "an unplaceable caller must not be able to claim the instance"
    );
    let body = body_json(res).await;
    assert!(
        body.to_string().contains("local network"),
        "the refusal should say how to proceed: {body}"
    );
}

#[tokio::test]
async fn setup_rejects_a_weak_password() {
    let _env = ENV_LOCK.lock().await;
    unsafe { std::env::set_var("KANI_ALLOW_REMOTE_SETUP", "true") };
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app
        .oneshot(post_json(
            "/rest/auth/setup",
            serde_json::json!({ "username": "owner", "email": "o@e.com", "password": "short" }),
        ))
        .await
        .unwrap();
    unsafe { std::env::remove_var("KANI_ALLOW_REMOTE_SETUP") };

    assert!(
        res.status().is_client_error(),
        "a weak first password must be refused, got {}",
        res.status()
    );
}

#[tokio::test]
async fn the_first_account_becomes_an_administrator() {
    let _env = ENV_LOCK.lock().await;
    unsafe { std::env::set_var("KANI_ALLOW_REMOTE_SETUP", "true") };
    let state = test_state().await;
    let app = build_test_app(state.clone()).await;

    let res = app
        .clone()
        .oneshot(post_json(
            "/rest/auth/setup",
            serde_json::json!({
                "username": "owner",
                "email": "owner@example.com",
                "password": "OwnerPassword123!"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "setup should succeed");

    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT r.role_slug FROM user_roles r JOIN users u ON u.id = r.user_id WHERE u.username = ?",
    )
    .bind("owner")
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert!(
        roles.iter().any(|r| r == "admin"),
        "the person who sets the instance up owns it, got {roles:?}"
    );

    let res = app
        .oneshot(post_json(
            "/rest/auth/setup",
            serde_json::json!({ "username": "second", "email": "s@e.com", "password": "SecondPassword123!" }),
        ))
        .await
        .unwrap();
    unsafe { std::env::remove_var("KANI_ALLOW_REMOTE_SETUP") };
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
