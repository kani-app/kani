#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{build_test_app, create_admin, test_state};
use tower::ServiceExt;

fn post(
    cookie: &str,
    key: Option<&str>,
    body: serde_json::Value,
) -> axum::http::Request<axum::body::Body> {
    let mut b = axum::http::Request::builder()
        .method("POST")
        .uri("/rest/me/api-tokens")
        .header(axum::http::header::COOKIE, common::csrf_cookie(cookie))
        .header("X-CSRF-Token", common::csrf_token(cookie))
        .header(axum::http::header::CONTENT_TYPE, "application/json");
    if let Some(k) = key {
        b = b.header("Idempotency-Key", k);
    }
    b.body(axum::body::Body::from(body.to_string())).unwrap()
}

fn token_body(name: &str) -> serde_json::Value {
    serde_json::json!({ "name": name })
}

async fn token_count(app: &axum::Router, cookie: &str) -> usize {
    let res = app
        .clone()
        .oneshot(common::authed_get("/rest/me/api-tokens", cookie))
        .await
        .unwrap();
    common::body_json(res).await.as_array().unwrap().len()
}

#[tokio::test]
async fn a_retry_with_the_same_key_replays_instead_of_writing_twice() {
    let (app, cookie) = common::admin_app().await;

    let first = app
        .clone()
        .oneshot(post(&cookie, Some("retry-1"), token_body("bot")))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);
    assert!(
        first.headers().get("x-idempotent-replay").is_none(),
        "the original request is not a replay"
    );
    let first_body = common::body_json(first).await;

    let second = app
        .clone()
        .oneshot(post(&cookie, Some("retry-1"), token_body("bot")))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::CREATED);
    assert_eq!(
        second.headers().get("x-idempotent-replay").unwrap(),
        "true",
        "the client must be able to tell a replay from a fresh write"
    );
    let second_body = common::body_json(second).await;

    assert_eq!(
        first_body, second_body,
        "a replay returns the original response verbatim — including the raw \
         token, which is shown exactly once and would otherwise be lost"
    );
    assert_eq!(
        token_count(&app, &cookie).await,
        1,
        "the write happened once"
    );
}

#[tokio::test]
async fn reusing_a_key_for_a_different_request_is_refused() {
    let (app, cookie) = common::admin_app().await;

    let first = app
        .clone()
        .oneshot(post(&cookie, Some("dup"), token_body("bot-a")))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);

    let second = app
        .clone()
        .oneshot(post(&cookie, Some("dup"), token_body("bot-b")))
        .await
        .unwrap();
    assert_eq!(
        second.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "replaying bot-a's response for a bot-b request would silently drop the \
         second write; the client bug must surface instead"
    );
    assert_eq!(token_count(&app, &cookie).await, 1);
}

#[tokio::test]
async fn distinct_keys_both_execute() {
    let (app, cookie) = common::admin_app().await;

    for key in ["k1", "k2"] {
        let res = app
            .clone()
            .oneshot(post(&cookie, Some(key), token_body("bot")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }
    assert_eq!(token_count(&app, &cookie).await, 2);
}

#[tokio::test]
async fn writes_without_a_key_are_untouched() {
    let (app, cookie) = common::admin_app().await;

    for _ in 0..2 {
        let res = app
            .clone()
            .oneshot(post(&cookie, None, token_body("bot")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }
    assert_eq!(
        token_count(&app, &cookie).await,
        2,
        "the middleware must be inert for clients that do not opt in"
    );
}

#[tokio::test]
async fn one_users_key_never_replays_to_another_user() {
    let state = test_state().await;
    let (au, ap) = create_admin(&state).await;
    let (bu, bp) = common::create_regular_user(&state, "beta").await;
    let app = build_test_app(state).await;

    let a_cookie = common::login(&app, au, ap).await;
    let b_cookie = common::login(&app, bu, bp).await;

    let a = app
        .clone()
        .oneshot(post(&a_cookie, Some("shared"), token_body("a-bot")))
        .await
        .unwrap();
    assert_eq!(a.status(), StatusCode::CREATED);

    let b = app
        .clone()
        .oneshot(post(&b_cookie, Some("shared"), token_body("b-bot")))
        .await
        .unwrap();
    assert_eq!(
        b.status(),
        StatusCode::CREATED,
        "an unrelated caller picking the same key string must not be blocked \
         by, or shown, the first caller's result"
    );
    assert!(b.headers().get("x-idempotent-replay").is_none());

    assert_eq!(token_count(&app, &a_cookie).await, 1);
    assert_eq!(token_count(&app, &b_cookie).await, 1);
}

#[tokio::test]
async fn a_get_is_never_recorded() {
    let (app, cookie) = common::admin_app().await;

    let mut req = common::authed_get("/rest/me/api-tokens", &cookie);
    req.headers_mut()
        .insert("Idempotency-Key", "g1".parse().unwrap());
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers().get("x-idempotent-replay").is_none());

    let write = app
        .clone()
        .oneshot(post(&cookie, Some("g1"), token_body("bot")))
        .await
        .unwrap();
    assert_eq!(write.status(), StatusCode::CREATED);
    assert!(write.headers().get("x-idempotent-replay").is_none());
}

#[tokio::test]
async fn a_malformed_key_is_rejected() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(post(&cookie, Some(""), token_body("bot")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let long = "x".repeat(256);
    let res = app
        .clone()
        .oneshot(post(&cookie, Some(&long), token_body("bot")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(token_count(&app, &cookie).await, 0);
}
