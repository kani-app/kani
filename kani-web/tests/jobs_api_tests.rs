#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use kani_shared_test::{insert_manga, insert_source};
use tower::ServiceExt;

#[tokio::test]
async fn list_jobs_200_admin() {
    let state = common::test_state().await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let app = common::build_test_app(state).await;
    let res = app
        .oneshot(common::authed_get("/rest/jobs", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = common::body_json(res).await;
    assert!(
        body["jobs"].is_array(),
        "jobs list is paged: {{ jobs, total }}"
    );
    assert_eq!(body["total"], serde_json::json!(0));
}

#[tokio::test]
async fn list_jobs_accepts_comma_separated_status_filter() {
    let state = common::test_state().await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let app = common::build_test_app(state).await;
    let res = app
        .oneshot(common::authed_get(
            "/rest/jobs?status=pending,running&limit=10",
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = common::body_json(res).await;
    assert!(body["jobs"].is_array());
}

#[tokio::test]
async fn get_job_404_nonexistent() {
    let state = common::test_state().await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let fake_id = uuid::Uuid::new_v4();
    let app = common::build_test_app(state).await;
    let res = app
        .oneshot(common::authed_get(
            &format!("/rest/jobs/{fake_id}"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancel_job_404_nonexistent() {
    let state = common::test_state().await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let fake_id = uuid::Uuid::new_v4();
    let app = common::build_test_app(state).await;
    let res = app
        .oneshot(common::authed_delete(
            &format!("/rest/jobs/{fake_id}"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn download_all_404_nonexistent_manga() {
    let state = common::test_state().await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let app = common::build_test_app(state).await;
    let res = app
        .oneshot(common::authed_post(
            "/rest/manga/99999/download_all",
            &cookie,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn download_all_200_returns_job_id() {
    let state = common::test_state().await;
    let source_id = insert_source(&state.db, "src").await;
    let manga_id = insert_manga(&state.db, source_id, "m1", "Manga").await;
    let app = common::build_test_app(state.clone()).await;
    common::create_admin(&state).await;
    let cookie = common::login(&app, "admin", "Password1234!").await;
    let app = common::build_test_app(state).await;
    let res = app
        .oneshot(common::authed_post(
            &format!("/rest/manga/{}/download_all", manga_id),
            &cookie,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = common::body_json(res).await;
    assert!(
        body.get("job_id").and_then(|v| v.as_str()).is_some(),
        "response must contain a job_id string field; got: {body}"
    );
}

#[tokio::test]
async fn a_queued_job_can_be_paused_and_resumed() {
    let state = common::test_state().await;
    let (u, p) = common::create_admin(&state).await;
    let db = state.db.clone();

    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO jobs (id, job_type, status, priority, description, params_json, created_at) \
         VALUES (?, 'integrity_scrub', 'pending', 0, 'test', ?, unixepoch())",
    )
    .bind(&id)
    .bind(
        serde_json::json!({ "id": uuid::Uuid::new_v4(), "depth": "quick", "fix": false })
            .to_string(),
    )
    .execute(&db)
    .await
    .unwrap();

    let app = common::build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let res = app
        .clone()
        .oneshot(common::authed_post(
            &format!("/rest/jobs/{id}/pause"),
            &cookie,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "pausing a queued job must work"
    );

    let status: String = sqlx::query_scalar("SELECT status FROM jobs WHERE id = ?")
        .bind(&id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(status, "paused");

    let res = app
        .oneshot(common::authed_post(
            &format!("/rest/jobs/{id}/resume"),
            &cookie,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let status: String = sqlx::query_scalar("SELECT status FROM jobs WHERE id = ?")
        .bind(&id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(
        status, "pending",
        "resume must put it back where the scheduler will find it"
    );
}

#[tokio::test]
async fn pausing_a_job_that_is_not_pending_is_refused() {
    let state = common::test_state().await;
    let (u, p) = common::create_admin(&state).await;
    let db = state.db.clone();

    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO jobs (id, job_type, status, priority, description, created_at) \
         VALUES (?, 'integrity_scrub', 'completed', 0, 'done', unixepoch())",
    )
    .bind(&id)
    .execute(&db)
    .await
    .unwrap();

    let app = common::build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let res = app
        .oneshot(common::authed_post(
            &format!("/rest/jobs/{id}/pause"),
            &cookie,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert!(
        !res.status().is_success(),
        "a finished job cannot be paused; the old stub returned ok for this"
    );
}
