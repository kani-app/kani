#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{
    authed_get, authed_patch, body_json, build_test_app, create_admin, insert_manga, insert_source,
    login, test_state,
};
use tower::ServiceExt;

#[tokio::test]
async fn set_scanlator_mode_stores_priority() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let src = insert_source(&state.db, "src").await;
    let manga_id = insert_manga(&state.db, src, "m1", "Test Manga").await;
    let db = state.db.clone();
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    // 'whitelist' first, so passing 'priority' cannot be confused with the
    // column's default.
    app.clone()
        .oneshot(authed_patch(
            &format!("/rest/manga/{manga_id}/scanlator_mode"),
            &cookie,
            serde_json::json!({"mode": "whitelist"}),
        ))
        .await
        .unwrap();

    let res = app
        .oneshot(authed_patch(
            &format!("/rest/manga/{manga_id}/scanlator_mode"),
            &cookie,
            serde_json::json!({"mode": "priority"}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let stored: String = sqlx::query_scalar("SELECT scanlator_mode FROM manga WHERE id = ?")
        .bind(manga_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(stored, "priority", "the mode must reach the manga row");
}

#[tokio::test]
async fn set_scanlator_mode_returns_200_for_whitelist() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let src = insert_source(&state.db, "src").await;
    let manga_id = insert_manga(&state.db, src, "m1", "Test Manga").await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_patch(
            &format!("/rest/manga/{manga_id}/scanlator_mode"),
            &cookie,
            serde_json::json!({"mode": "whitelist"}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn set_scanlator_mode_returns_400_for_invalid_mode() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let src = insert_source(&state.db, "src").await;
    let manga_id = insert_manga(&state.db, src, "m1", "Test Manga").await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_patch(
            &format!("/rest/manga/{manga_id}/scanlator_mode"),
            &cookie,
            serde_json::json!({"mode": "invalid-mode"}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = body_json(res).await;
    assert_eq!(body["code"], "validation_error");
}

#[tokio::test]
async fn set_scanlator_mode_persists_change() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let src = insert_source(&state.db, "src").await;
    let manga_id = insert_manga(&state.db, src, "m1", "Test Manga").await;
    let app = build_test_app(state.clone()).await;
    let cookie = login(&app, username, password).await;

    app.clone()
        .oneshot(authed_patch(
            &format!("/rest/manga/{manga_id}/scanlator_mode"),
            &cookie,
            serde_json::json!({"mode": "whitelist"}),
        ))
        .await
        .unwrap();

    let details_res = app
        .oneshot(authed_get(
            &format!("/rest/manga/{manga_id}/details"),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(details_res.status(), StatusCode::OK);
    let body = body_json(details_res).await;
    assert_eq!(body["scanlator_mode"], "whitelist");
}

#[tokio::test]
async fn get_chapter_scanlators_returns_200_for_existing_manga() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let src = insert_source(&state.db, "src").await;
    let manga_id = insert_manga(&state.db, src, "m1", "Test Manga").await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_get(
            &format!("/rest/manga/{manga_id}/scanlators"),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_scanlator_prefs_returns_empty_list_for_fresh_manga() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let src = insert_source(&state.db, "src").await;
    let manga_id = insert_manga(&state.db, src, "m1", "Test Manga").await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_get(
            &format!("/rest/manga/{manga_id}/scanlator_preferences"),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body, serde_json::json!([]));
}
