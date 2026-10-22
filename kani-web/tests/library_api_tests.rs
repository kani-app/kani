#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{
    authed_get, authed_post, body_json, build_test_app, create_admin, insert_manga, insert_source,
    login, test_state,
};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn get_library_returns_empty_list_for_fresh_db() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/library?page=1&page_size=20", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["items"], serde_json::json!([]));
}

#[tokio::test]
async fn scan_all_library_returns_200_for_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/library/scan-all",
            &cookie,
            serde_json::json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    let job_id = body["job_id"]
        .as_str()
        .expect("scan-all must return job_id");
    assert!(Uuid::parse_str(job_id).is_ok(), "job_id must be a UUID");
}

#[tokio::test]
async fn get_library_invalid_page_returns_400() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/library?page=0&page_size=20", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn scan_manga_all_returns_200_for_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/manga/scan",
            &cookie,
            serde_json::json!({ "ids": "all" }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    let job_id = body["job_id"]
        .as_str()
        .expect("scan with ids=all must return job_id");
    assert!(Uuid::parse_str(job_id).is_ok(), "job_id must be a UUID");
}

#[tokio::test]
async fn scan_manga_ids_empty_returns_200_for_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/manga/scan",
            &cookie,
            serde_json::json!({ "ids": [] }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn scan_manga_invalid_body_returns_422() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/manga/scan",
            &cookie,
            serde_json::json!({ "ids": 42 }),
        ))
        .await
        .unwrap();

    assert!(
        res.status().is_client_error(),
        "expected 4xx, got {}",
        res.status(),
    );
}

#[tokio::test]
async fn a_library_item_links_its_thumbnail_not_the_full_cover() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let app = build_test_app(state.clone()).await;
    let cookie = login(&app, u, p).await;

    let source_id = insert_source(&state.db, "src-cover").await;
    let manga_id = insert_manga(&state.db, source_id, "ext-cover", "Covered").await;

    let library_path = state.service.settings.read().await.library_path.clone();
    let covers_dir = library_path.join("covers");
    tokio::fs::create_dir_all(&covers_dir).await.unwrap();
    let jpeg = kani_shared_test::origin::jpeg_page(400, 600, false, 80);
    tokio::fs::write(covers_dir.join(format!("{manga_id}.jpg")), &jpeg)
        .await
        .unwrap();
    sqlx::query("UPDATE manga SET local_cover_path = ? WHERE id = ?")
        .bind(format!("covers/{manga_id}.jpg"))
        .bind(manga_id)
        .execute(&state.db)
        .await
        .unwrap();
    state
        .service
        .generate_and_store_thumbnails(manga_id)
        .await
        .unwrap();

    let cover_hash: String = sqlx::query_scalar("SELECT cover_hash FROM manga WHERE id = ?")
        .bind(manga_id)
        .fetch_one(&state.db)
        .await
        .unwrap();

    let res = app
        .oneshot(authed_get("/rest/library?page=1&page_size=20", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = body_json(res).await;
    let cover_url = body["items"][0]["cover_url"]
        .as_str()
        .expect("a manga with a local cover must expose a cover_url");

    assert!(
        cover_url.contains("size=sm"),
        "the grid asked for the full-size original: {cover_url}"
    );
    assert!(
        cover_url.contains(&format!("h={}", &cover_hash[..16])),
        "cover_url carries no matching hash, so the response cannot be marked immutable: {cover_url}"
    );
}
