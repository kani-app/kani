#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{
    authed_get, authed_patch, authed_post, body_json, build_test_app, create_admin,
    create_regular_user, login, test_state,
};
use tower::ServiceExt;

#[tokio::test]
async fn get_settings_returns_200_for_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/settings", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(
        body.get("auto_scan").is_some(),
        "settings must include auto_scan"
    );
}

#[tokio::test]
async fn a_plain_user_cannot_read_the_infrastructure_settings() {
    let state = test_state().await;
    let (admin_u, admin_p) = create_admin(&state).await;
    let (user_u, user_p) = create_regular_user(&state, "plain").await;
    let app = build_test_app(state).await;

    let admin_cookie = login(&app, admin_u, admin_p).await;
    let res = app
        .clone()
        .oneshot(authed_patch(
            "/rest/settings",
            &admin_cookie,
            serde_json::json!({ "Email": {
                "email_enabled": true,
                "email_provider": "smtp",
                "email_provider_config": "{\"host\":\"smtp.internal.lan\",\"port\":587,\"username\":\"kani@example.com\",\"password\":\"hunter2\"}",
                "email_from_address": "kani@example.com",
                "app_url": "https://kani.internal.lan",
                "password_reset_enabled": true,
                "email_verification_required": false
            }}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "admin may configure email");

    let user_cookie = login(&app, user_u, user_p).await;
    let res = app
        .clone()
        .oneshot(authed_get("/rest/settings", &user_cookie))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "a plain user may read settings"
    );
    let body = body_json(res).await;

    for field in [
        "email_provider_config",
        "email_from_address",
        "app_url",
        "flaresolverr_url",
        "library_path",
        "wasm_storage_path",
    ] {
        assert_eq!(
            body.get(field).and_then(|v| v.as_str()),
            Some(""),
            "{field} describes the deployment and must be withheld from a plain user"
        );
    }
    let serialised = body.to_string();
    assert!(
        !serialised.contains("smtp.internal.lan") && !serialised.contains("kani.internal.lan"),
        "no infrastructure host may appear anywhere in the payload: {serialised}"
    );

    assert!(body.get("auto_scan").is_some());
    assert_eq!(
        body.get("email_enabled").and_then(|v| v.as_bool()),
        Some(true)
    );

    let res = app
        .oneshot(authed_get("/rest/settings", &admin_cookie))
        .await
        .unwrap();
    let admin_body = body_json(res).await;
    assert_eq!(
        admin_body.get("app_url").and_then(|v| v.as_str()),
        Some("https://kani.internal.lan"),
        "an admin must still be able to read what they configured"
    );
}

#[tokio::test]
async fn patch_settings_scan_updates_interval() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(authed_patch(
            "/rest/settings",
            &cookie,
            serde_json::json!({
                "Scan": {
                    "auto_scan": false,
                    "scan_interval_minutes": 120,
                    "scan_exclude_completed": false,
                    "upgrade_detection_enabled": true,
                    "upgrade_min_res_gain": 1.5,
                    "upgrade_confirm_fetches": 2
                }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let get_res = app
        .oneshot(authed_get("/rest/settings", &cookie))
        .await
        .unwrap();
    assert_eq!(get_res.status(), StatusCode::OK);
    let body = body_json(get_res).await;
    assert_eq!(body["scan_interval_minutes"], serde_json::json!(120));
}

#[tokio::test]
async fn patch_settings_invalid_body_returns_4xx() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_patch(
            "/rest/settings",
            &cookie,
            serde_json::json!({"Scan": {"auto_scan": "not-a-bool", "scan_interval_minutes": 60, "scan_exclude_completed": false}}),
        ))
        .await
        .unwrap();

    assert!(
        res.status().is_client_error(),
        "invalid settings body should return 4xx, got {}",
        res.status()
    );
}

#[tokio::test]
async fn get_settings_shows_env_setting_defaults() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/settings", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["trash_retention_days"], serde_json::json!(30));
    assert_eq!(body["audit_retention_days"], serde_json::json!(365));
    assert_eq!(body["audit_security_retention_days"], serde_json::json!(0));
    assert_eq!(body["max_login_attempts"], serde_json::json!(5));
    assert_eq!(body["max_ip_attempts"], serde_json::json!(20));
    assert_eq!(body["login_lockout_seconds"], serde_json::json!(900));
    assert_eq!(body["session_timeout_secs"], serde_json::json!(2592000));
    assert_eq!(body["thumbnail_formats"], serde_json::json!("jpeg"));
}

#[tokio::test]
async fn patch_settings_maintenance_updates() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(authed_patch(
            "/rest/settings",
            &cookie,
            serde_json::json!({
                "Maintenance": {
                    "trash_retention_days": 14,
                    "audit_retention_days": 180,
                    "audit_security_retention_days": 90,
                    "disk_warn_threshold": 0.25,
                    "thumbnail_formats": "jpeg",
                    "integrity_quick_scrub_interval_hours": 12,
                    "integrity_deep_scrub_interval_hours": 336,
                    "scrub_on_startup": true
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let get_res = app
        .oneshot(authed_get("/rest/settings", &cookie))
        .await
        .unwrap();
    let body = body_json(get_res).await;
    assert_eq!(body["trash_retention_days"], serde_json::json!(14));
    assert_eq!(body["audit_retention_days"], serde_json::json!(180));
    assert_eq!(body["audit_security_retention_days"], serde_json::json!(90));
    assert_eq!(
        body["integrity_quick_scrub_interval_hours"],
        serde_json::json!(12)
    );
    assert_eq!(
        body["integrity_deep_scrub_interval_hours"],
        serde_json::json!(336)
    );
    assert_eq!(body["scrub_on_startup"], serde_json::json!(true));
    assert_eq!(body["disk_warn_threshold"], serde_json::json!(0.25));
}

#[tokio::test]
async fn patch_settings_security_updates() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(authed_patch(
            "/rest/settings",
            &cookie,
            serde_json::json!({
                "Security": {
                    "max_login_attempts": 8,
                    "max_ip_attempts": 40,
                    "login_lockout_seconds": 600,
                    "session_timeout_secs": 86400
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let get_res = app
        .oneshot(authed_get("/rest/settings", &cookie))
        .await
        .unwrap();
    let body = body_json(get_res).await;
    assert_eq!(body["max_login_attempts"], serde_json::json!(8));
    assert_eq!(body["max_ip_attempts"], serde_json::json!(40));
    assert_eq!(body["login_lockout_seconds"], serde_json::json!(600));
    assert_eq!(body["session_timeout_secs"], serde_json::json!(86400));
}

#[tokio::test]
async fn patch_settings_maintenance_invalid_threshold_returns_4xx() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_patch(
            "/rest/settings",
            &cookie,
            serde_json::json!({
                "Maintenance": {
                    "trash_retention_days": 14,
                    "audit_retention_days": 180,
                    "audit_security_retention_days": 90,
                    "disk_warn_threshold": 1.5,
                    "thumbnail_formats": "jpeg"
                }
            }),
        ))
        .await
        .unwrap();
    assert!(
        res.status().is_client_error(),
        "out-of-range disk_warn_threshold should return 4xx, got {}",
        res.status()
    );
}

#[tokio::test]
async fn patch_settings_security_invalid_attempts_returns_4xx() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_patch(
            "/rest/settings",
            &cookie,
            serde_json::json!({
                "Security": {
                    "max_login_attempts": 0,
                    "max_ip_attempts": 40,
                    "login_lockout_seconds": 600,
                    "session_timeout_secs": 86400
                }
            }),
        ))
        .await
        .unwrap();
    assert!(
        res.status().is_client_error(),
        "max_login_attempts=0 should return 4xx, got {}",
        res.status()
    );
}

#[tokio::test]
async fn patch_settings_performance_updates() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(authed_patch(
            "/rest/settings",
            &cookie,
            serde_json::json!({
                "Performance": {
                    "max_concurrent_jobs": 16,
                    "db_maintenance_interval_hours": 12,
                    "db_vacuum_interval_hours": 72,
                    "audit_prune_interval_hours": 240,
                    "trash_purge_interval_hours": 96
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let get_res = app
        .oneshot(authed_get("/rest/settings", &cookie))
        .await
        .unwrap();
    let body = body_json(get_res).await;
    assert_eq!(body["max_concurrent_jobs"], serde_json::json!(16));
    assert_eq!(body["db_maintenance_interval_hours"], serde_json::json!(12));
    assert_eq!(body["db_vacuum_interval_hours"], serde_json::json!(72));
    assert_eq!(body["audit_prune_interval_hours"], serde_json::json!(240));
    assert_eq!(body["trash_purge_interval_hours"], serde_json::json!(96));
}

#[tokio::test]
async fn patch_settings_performance_invalid_returns_4xx() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_patch(
            "/rest/settings",
            &cookie,
            serde_json::json!({
                "Performance": {
                    "max_concurrent_jobs": 0,
                    "db_maintenance_interval_hours": 24,
                    "db_vacuum_interval_hours": 168,
                    "audit_prune_interval_hours": 168,
                    "trash_purge_interval_hours": 168
                }
            }),
        ))
        .await
        .unwrap();
    assert!(
        res.status().is_client_error(),
        "max_concurrent_jobs=0 should return 4xx, got {}",
        res.status()
    );
}

#[tokio::test]
async fn patch_settings_rejects_a_zero_scrub_interval() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_patch(
            "/rest/settings",
            &cookie,
            serde_json::json!({
                "Maintenance": {
                    "trash_retention_days": 14,
                    "audit_retention_days": 180,
                    "audit_security_retention_days": 90,
                    "disk_warn_threshold": 0.25,
                    "thumbnail_formats": "jpeg",
                    "integrity_quick_scrub_interval_hours": 0,
                    "integrity_deep_scrub_interval_hours": 168,
                    "scrub_on_startup": false
                }
            }),
        ))
        .await
        .unwrap();

    assert!(
        res.status().is_client_error(),
        "a zero interval would reschedule the scrub continuously and hash the \
         whole library in a loop"
    );
}

#[tokio::test]
async fn patch_settings_rejects_an_out_of_range_upgrade_gain() {
    let (app, cookie) = common::admin_app().await;

    for gain in [0.5, 9.0] {
        let res = app
            .clone()
            .oneshot(authed_patch(
                "/rest/settings",
                &cookie,
                serde_json::json!({
                    "Scan": {
                        "auto_scan": false,
                        "scan_interval_minutes": 120,
                        "scan_exclude_completed": false,
                        "upgrade_detection_enabled": true,
                        "upgrade_min_res_gain": gain,
                        "upgrade_confirm_fetches": 2
                    }
                }),
            ))
            .await
            .unwrap();
        assert!(
            res.status().is_client_error(),
            "a gain below 1.0 would flag every re-encode as an upgrade, and an \
             absurd one would flag nothing; got {} for {gain}",
            res.status()
        );
    }
}

#[tokio::test]
async fn solver_test_rejects_an_empty_url() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/settings/solver/test",
            &cookie,
            serde_json::json!({ "url": "   " }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn solver_test_reports_an_unreachable_solver_without_failing_the_request() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/settings/solver/test",
            &cookie,
            serde_json::json!({ "url": "http://127.0.0.1:1/v1" }),
        ))
        .await
        .unwrap();

    assert_eq!(
        res.status(),
        StatusCode::OK,
        "a failed probe is a result, not an HTTP error"
    );
    let body = body_json(res).await;
    assert_eq!(body["status"], "unreachable");
    assert_eq!(body["insecure_transport"], false);
}

#[tokio::test]
async fn solver_test_flags_plain_http_to_a_routable_host() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/settings/solver/test",
            &cookie,
            serde_json::json!({ "url": "http://solver.example.com/v1" }),
        ))
        .await
        .unwrap();

    let body = body_json(res).await;
    assert_eq!(
        body["insecure_transport"], true,
        "the key and captured pages would cross the network in the clear"
    );
}
