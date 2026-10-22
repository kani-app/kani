#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{authed_get, authed_post, body_json};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn db_stats_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/admin/db/stats", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body["db_size_bytes"].as_i64().unwrap_or(-1) > 0);
    assert!(body["page_size"].as_i64().unwrap_or(-1) > 0);
}

#[tokio::test]
async fn db_analyze_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post("/rest/admin/db/analyze", &cookie, json!({})))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body["job_id"].as_str().is_some());
}

#[tokio::test]
async fn db_vacuum_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post("/rest/admin/db/vacuum", &cookie, json!({})))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body["job_id"].as_str().is_some());
}

#[tokio::test]
async fn run_maintenance_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post("/rest/admin/maintenance", &cookie, json!({})))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body["analyze_job_id"].as_str().is_some());
    assert!(body["vacuum_job_id"].as_str().is_some());
}

#[tokio::test]
async fn trigger_recurring_returns_200_with_job_id_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/admin/recurring/db_maintenance/run",
            &cookie,
            json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body["job_id"].as_str().is_some());
}

#[tokio::test]
async fn trigger_recurring_unknown_kind_returns_404() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/admin/recurring/not_a_real_kind/run",
            &cookie,
            json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
