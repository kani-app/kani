#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{authed_get, authed_post, body_json};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn list_saved_searches_returns_200_for_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/saved-searches", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn create_saved_search_returns_201_for_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/saved-searches",
            &cookie,
            json!({
                "name": "Ongoing",
                "query_json": "{\"status_filter\":1}"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CREATED);
    let body = body_json(res).await;
    assert_eq!(body["name"], "Ongoing");
}
