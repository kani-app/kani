#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use axum::{body::Body, http::Request};
use common::{authed_get, body_json, build_test_app, create_admin, login, put_json, test_state};
use tower::ServiceExt;

fn authed_post_json(uri: &str, cookie: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Cookie", common::csrf_cookie(cookie))
        .header("X-CSRF-Token", common::csrf_token(cookie))
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap()
}

#[tokio::test]
async fn get_bookmarks_returns_200_authed() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let (_, chapter_id) = common::seed_manga_with_chapter(&state).await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_get(
            &format!("/rest/chapter/{chapter_id}/bookmarks"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body, serde_json::json!([]));
}

#[tokio::test]
async fn toggle_bookmark_adds_and_returns_state() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let (_, chapter_id) = common::seed_manga_with_chapter(&state).await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .clone()
        .oneshot(authed_post_json(
            &format!("/rest/chapter/{chapter_id}/bookmarks"),
            &cookie,
            serde_json::json!({ "page_index": 3 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["bookmarked"], serde_json::json!(true));
}

#[tokio::test]
async fn get_chapter_note_returns_200_authed() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let (_, chapter_id) = common::seed_manga_with_chapter(&state).await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_get(
            &format!("/rest/chapter/{chapter_id}/note"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["note"], serde_json::json!(null));
}

#[tokio::test]
async fn a_chapter_note_is_stored_and_read_back() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let (_, chapter_id) = common::seed_manga_with_chapter(&state).await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .clone()
        .oneshot(put_json(
            &format!("/rest/chapter/{chapter_id}/note"),
            &cookie,
            serde_json::json!({ "note": "interesting chapter" }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let fetched = app
        .oneshot(common::authed_get(
            &format!("/rest/chapter/{chapter_id}/note"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(fetched.status(), StatusCode::OK);

    let body = common::body_json(fetched).await;
    assert_eq!(
        body["note"], "interesting chapter",
        "the stored note must come back, got {body}"
    );
}

#[tokio::test]
async fn get_manga_chapter_notes_returns_notes_object() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let (manga_id, chapter_id) = common::seed_manga_with_chapter(&state).await;
    let app = build_test_app(state.clone()).await;
    let cookie = login(&app, username, password).await;

    app.clone()
        .oneshot(put_json(
            &format!("/rest/chapter/{chapter_id}/note"),
            &cookie,
            serde_json::json!({ "note": "chapter note text" }),
        ))
        .await
        .unwrap();

    let res = app
        .oneshot(authed_get(
            &format!("/rest/manga/{manga_id}/chapter-notes"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    let notes = body["notes"].as_array().expect("notes should be an array");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["chapter_id"], serde_json::json!(chapter_id));
    assert_eq!(notes[0]["note"], serde_json::json!("chapter note text"));
}
