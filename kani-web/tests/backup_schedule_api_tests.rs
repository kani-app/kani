#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{authed_get, authed_post, body_json, put_json};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn admin_get_backup_schedule_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/admin/backup/schedule", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body.get("enabled").is_some());
    assert!(body.get("frequency").is_some());
    assert!(body.get("retain_n").is_some());
    assert!(body.get("destination").is_some());
}

#[tokio::test]
async fn admin_put_backup_schedule_round_trips() {
    let (app, cookie) = common::admin_app().await;

    let config = json!({
        "enabled": true,
        "frequency": { "type": "weekly", "weekday": 6, "hour": 4 },
        "retain_n": 3,
        "destination": { "type": "local", "path": "/tmp/kani-backups" },
        "passphrase": null
    });

    app.clone()
        .oneshot(put_json("/rest/admin/backup/schedule", &cookie, config))
        .await
        .unwrap();

    let res = app
        .oneshot(authed_get("/rest/admin/backup/schedule", &cookie))
        .await
        .unwrap();
    let body = body_json(res).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["retain_n"], 3);
    assert_eq!(body["frequency"]["type"], "weekly");
    assert_eq!(body["frequency"]["weekday"], 6);
}

#[tokio::test]
async fn admin_backup_run_now_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/admin/backup/run-now",
            &cookie,
            json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body.get("job_id").is_some());
}
