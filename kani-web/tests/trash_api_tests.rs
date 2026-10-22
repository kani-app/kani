#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{
    authed_delete, authed_get, authed_post, body_json, build_test_app, create_admin,
    create_regular_user, test_state,
};
use common::{insert_manga, insert_source};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn list_trash_returns_200_for_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/trash", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn untrash_manga_returns_404_for_regular_user_missing_id() {
    let state = test_state().await;
    let (username, password) = create_regular_user(&state, "alice").await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;

    let res = app
        .oneshot(authed_post(
            "/rest/manga/999999/untrash",
            &cookie,
            json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn purge_trash_all_returns_200_for_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_delete("/rest/trash", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body.get("purged").is_some());
}

#[tokio::test]
async fn purge_trash_all_returns_200_for_regular_user() {
    let state = test_state().await;
    let (username, password) = create_regular_user(&state, "bob").await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;

    let res = app
        .oneshot(authed_delete("/rest/trash", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["purged"], 0);
}

#[tokio::test]
async fn purge_trash_one_returns_404_for_regular_user_missing_id() {
    let state = test_state().await;
    let (username, password) = create_regular_user(&state, "carol").await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;

    let res = app
        .oneshot(authed_delete("/rest/trash/999999", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_manga_returns_undo_token() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let source_id = insert_source(&state.service.db, "test-src").await;
    let manga_id = insert_manga(&state.service.db, source_id, "m1", "Test Manga").await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;

    let res = app
        .oneshot(authed_delete(
            &format!("/rest/manga/{}", manga_id.0),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    let token = body["undo_token"]
        .as_str()
        .expect("undo_token must be a string");
    assert!(
        uuid::Uuid::parse_str(token).is_ok(),
        "undo_token must be a UUID"
    );
}

#[tokio::test]
async fn untrash_by_token_returns_422_for_missing_token_field() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post("/rest/manga/untrash", &cookie, json!({})))
        .await
        .unwrap();

    assert!(
        res.status().is_client_error(),
        "expected 4xx for missing token, got {}",
        res.status()
    );
}

#[tokio::test]
async fn untrash_by_token_returns_404_for_unknown_token() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/manga/untrash",
            &cookie,
            json!({ "token": "00000000-0000-0000-0000-000000000001" }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn untrash_by_token_returns_200_for_valid_token() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let source_id = insert_source(&state.service.db, "test-src").await;
    let manga_id = insert_manga(&state.service.db, source_id, "m2", "Undo Test").await;
    let app = build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;

    let del_res = app
        .clone()
        .oneshot(authed_delete(
            &format!("/rest/manga/{}", manga_id.0),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(del_res.status(), StatusCode::OK);
    let del_body = body_json(del_res).await;
    let token = del_body["undo_token"].as_str().unwrap().to_string();

    let res = app
        .oneshot(authed_post(
            "/rest/manga/untrash",
            &cookie,
            json!({ "token": token }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}
