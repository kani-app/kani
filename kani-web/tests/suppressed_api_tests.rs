#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{authed_post, build_test_app, create_admin, login, test_state};
use kani_shared_test::{insert_manga, insert_source};
use tower::ServiceExt;

#[tokio::test]
async fn dismiss_is_a_noop_for_an_unknown_manga() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/manga/99999/dismiss-suppressed",
            &cookie,
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn dismiss_zeroes_the_suppressed_count() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let source_id = insert_source(&state.db, "s").await;
    let manga_id = insert_manga(&state.db, source_id, "m1", "Manga").await;
    sqlx::query("UPDATE manga SET suppressed_chapter_count = 4 WHERE id = ?")
        .bind(manga_id.0)
        .execute(&state.db)
        .await
        .unwrap();
    let app = build_test_app(state.clone()).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_post(
            &format!("/rest/manga/{}/dismiss-suppressed", manga_id.0),
            &cookie,
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let count: i64 = sqlx::query_scalar("SELECT suppressed_chapter_count FROM manga WHERE id = ?")
        .bind(manga_id.0)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(count, 0, "the signal is cleared");
}
