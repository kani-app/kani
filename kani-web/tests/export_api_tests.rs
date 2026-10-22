#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{authed_get, body_json};
use tower::ServiceExt;

#[tokio::test]
async fn system_capabilities_returns_200_with_auth() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/system/capabilities", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(
        body["kcc"],
        serde_json::Value::Bool(false),
        "kcc should be false when kcc-c2e is not in PATH"
    );
    assert!(
        body.get("kcc_version").is_some(),
        "Response must include kcc_version key"
    );
}

#[tokio::test]
async fn export_kcc_returns_error_for_missing_chapter() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get(
            "/rest/chapters/999999/export/kcc?profile=kindle-pw",
            &cookie,
        ))
        .await
        .unwrap();

    assert!(
        !res.status().is_success(),
        "Expected error for missing chapter or missing kcc-c2e binary"
    );
}
