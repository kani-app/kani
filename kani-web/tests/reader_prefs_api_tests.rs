#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{authed_get, body_json, build_test_app, create_admin, login, put_json, test_state};
use tower::ServiceExt;

#[tokio::test]
async fn put_reader_prefs_returns_204() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let manga_id = common::seed_manga_with_chapter(&state).await.0;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(put_json(
            &format!("/rest/manga/{manga_id}/tracking"),
            &cookie,
            serde_json::json!({ "reader_prefs": r#"{"mode":"webtoon"}"# }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn put_reader_prefs_is_returned_by_get_tracking() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let manga_id = common::seed_manga_with_chapter(&state).await.0;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    app.clone()
        .oneshot(put_json(
            &format!("/rest/manga/{manga_id}/tracking"),
            &cookie,
            serde_json::json!({ "reader_prefs": r#"{"mode":"scroll","fit":"width"}"# }),
        ))
        .await
        .unwrap();

    let res = app
        .oneshot(authed_get(
            &format!("/rest/manga/{manga_id}/tracking"),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(
        body["reader_prefs"],
        serde_json::json!(r#"{"mode":"scroll","fit":"width"}"#)
    );
}

#[tokio::test]
async fn put_reader_prefs_rejects_non_object_json() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let manga_id = common::seed_manga_with_chapter(&state).await.0;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(put_json(
            &format!("/rest/manga/{manga_id}/tracking"),
            &cookie,
            serde_json::json!({ "reader_prefs": "[1,2,3]" }),
        ))
        .await
        .unwrap();

    assert!(
        res.status().is_client_error(),
        "non-object JSON must be rejected with a 4xx, got {}",
        res.status()
    );
}
