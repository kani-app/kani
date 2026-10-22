#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{
    authed_get, authed_post, body_json, build_test_app, create_admin, create_regular_user, get_req,
    login, post_json, test_state,
};
use tower::ServiceExt;

#[tokio::test]
async fn login_with_valid_credentials_returns_ok() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app(state).await;

    let res = app
        .oneshot(post_json(
            "/rest/auth/login",
            serde_json::json!({"username": username, "password": password}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["ok"], serde_json::json!(true));
}

#[tokio::test]
async fn login_with_invalid_password_returns_401() {
    let state = test_state().await;
    let (username, _) = create_admin(&state).await;
    let app = build_test_app(state).await;

    let res = app
        .oneshot(post_json(
            "/rest/auth/login",
            serde_json::json!({"username": username, "password": "wrongpassword"}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_with_unknown_user_returns_401() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app
        .oneshot(post_json(
            "/rest/auth/login",
            serde_json::json!({"username": "nobody", "password": "anything"}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_me_returns_user_for_authenticated_session() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(authed_get("/rest/auth/me", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["username"], serde_json::json!(common::ADMIN_USERNAME));
}

#[tokio::test]
async fn auth_me_returns_401_without_session() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app.oneshot(get_req("/rest/auth/me")).await.unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_invalidates_session() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(authed_post(
            "/rest/auth/logout",
            &cookie,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = app
        .clone()
        .oneshot(authed_get("/rest/auth/me", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sessions_list_includes_current_session() {
    let (app, cookie) = common::admin_app().await;

    let mut sessions = serde_json::json!([]);
    for _ in 0..20 {
        let res = app
            .clone()
            .oneshot(authed_get("/rest/auth/sessions", &cookie))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        sessions = body["sessions"].clone();
        if sessions.as_array().is_some_and(|a| !a.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let list = sessions.as_array().unwrap();
    assert!(
        !list.is_empty(),
        "the live session must appear in the inventory"
    );
    assert!(
        list.iter()
            .any(|s| s["is_current"] == serde_json::json!(true)),
        "the calling session should be flagged is_current: {list:?}"
    );
}

#[tokio::test]
async fn sessions_list_unauthenticated_returns_401() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app.oneshot(get_req("/rest/auth/sessions")).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn registration_enabled_returns_correct_flag() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app
        .oneshot(get_req("/rest/auth/registration-enabled"))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["enabled"], serde_json::json!(true));
}

#[tokio::test]
async fn get_current_user_returns_full_user_for_authenticated_session() {
    let state = test_state().await;
    let (username, password) = create_regular_user(&state, "alice").await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_get("/rest/auth/current_user", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["username"], serde_json::json!(username));
    assert!(
        body.get("password_hash").is_none(),
        "password hash must not be exposed"
    );
}

#[tokio::test]
async fn a_revoked_session_cookie_stops_working() {
    let (app, victim) = common::admin_app().await;
    let survivor = common::second_admin_session(&app).await;

    let ok = app
        .clone()
        .oneshot(authed_get("/rest/auth/sessions", &victim))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);

    let listing = app
        .clone()
        .oneshot(authed_get("/rest/auth/sessions", &survivor))
        .await
        .unwrap();
    let sessions = common::body_json(listing).await;
    let ids: Vec<String> = sessions["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["id"].as_str().map(str::to_owned))
        .collect();
    assert!(
        ids.len() >= 2,
        "both logins should be listed, got {}",
        ids.len()
    );

    let res = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("DELETE")
                .uri("/rest/auth/sessions")
                .header(axum::http::header::COOKIE, common::csrf_cookie(&survivor))
                .header("X-CSRF-Token", common::csrf_token(&survivor))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let replay = app
        .clone()
        .oneshot(authed_get("/rest/auth/sessions", &victim))
        .await
        .unwrap();
    assert_eq!(
        replay.status(),
        StatusCode::UNAUTHORIZED,
        "a revoked session must not be able to keep making requests"
    );

    let still_ok = app
        .oneshot(authed_get("/rest/auth/sessions", &survivor))
        .await
        .unwrap();
    assert_eq!(still_ok.status(), StatusCode::OK);
}
