#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use common::{
    authed_get, authed_post, body_json, build_test_app, create_regular_user, get_req, login,
    post_json, test_state,
};
use kani_app::ServiceError;
use kani_web::error::AppError;
use tower::ServiceExt;

#[test]
fn service_conflict_maps_to_409() {
    let resp = AppError::from(ServiceError::Conflict("already in progress".into())).into_response();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[test]
fn service_forbidden_maps_to_403() {
    let resp = AppError::from(ServiceError::Forbidden("no access".into())).into_response();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[test]
fn service_not_found_maps_to_404() {
    let resp = AppError::from(ServiceError::NotFound("manga 1".into())).into_response();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn missing_manga_returns_404_with_json_code() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/manga/999999", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body = body_json(res).await;
    assert_eq!(body["code"], serde_json::json!("not_found"));
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn missing_source_returns_404_with_json_code() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/sources/999999", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body = body_json(res).await;
    assert_eq!(body["code"], serde_json::json!("not_found"));
}

#[tokio::test]
async fn unauthenticated_library_request_returns_401_with_json() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app.oneshot(get_req("/rest/library")).await.unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let body = body_json(res).await;
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn invalid_login_returns_401_with_json_error() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app
        .oneshot(post_json(
            "/rest/auth/login",
            serde_json::json!({"username": "nobody", "password": "wrong"}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let body = body_json(res).await;
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn regular_user_accessing_admin_endpoint_returns_403() {
    let state = test_state().await;
    let (username, password) = create_regular_user(&state, "carol").await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_get("/rest/admin/users", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let body = body_json(res).await;
    assert_eq!(body["code"], serde_json::json!("forbidden"));
    assert!(
        body["hint"].is_string(),
        "forbidden errors must include a hint"
    );
}

#[tokio::test]
async fn malformed_login_body_returns_400() {
    let state = test_state().await;
    let app = build_test_app(state).await;

    let res = app
        .oneshot(post_json(
            "/rest/auth/login",
            serde_json::json!({"username": "someone"}),
        ))
        .await
        .unwrap();

    assert!(
        res.status().is_client_error(),
        "malformed body should return 4xx, got {}",
        res.status()
    );
}

#[tokio::test]
async fn admin_create_user_with_short_password_returns_400() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/admin/users",
            &cookie,
            serde_json::json!({
                "username": "x",
                "email": "x@test.local",
                "password": "abc",
                "roles": []
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = body_json(res).await;
    assert_eq!(body["code"], serde_json::json!("validation_error"));
}

fn source_failure(kind: kani_shared::extension::ExtensionErrorKind) -> AppError {
    AppError::from(ServiceError::Core(kani_core::Error::Extension(
        kani_shared::extension::ExtensionError {
            kind,
            message: "the source said no".into(),
            source_url: None,
            retry_after_secs: None,
        },
    )))
}

fn browser_unavailable(code: &str) -> AppError {
    AppError::from(ServiceError::Core(
        kani_core::Error::BrowserCaptureUnavailable {
            code: code.into(),
            message: "capture unavailable".into(),
        },
    ))
}

#[tokio::test]
async fn each_source_failure_kind_gets_its_own_status_and_code() {
    use kani_shared::extension::ExtensionErrorKind as K;

    let cases = [
        (K::Network, StatusCode::BAD_GATEWAY, "source_network"),
        (K::Timeout, StatusCode::GATEWAY_TIMEOUT, "source_timeout"),
        (
            K::Updating,
            StatusCode::SERVICE_UNAVAILABLE,
            "source_updating",
        ),
        (
            K::RateLimited,
            StatusCode::TOO_MANY_REQUESTS,
            "source_rate_limited",
        ),
        (K::NotFound, StatusCode::NOT_FOUND, "source_not_found"),
        (
            K::ContentUnavailable,
            StatusCode::NOT_FOUND,
            "content_unavailable",
        ),
        (K::Auth, StatusCode::UNAUTHORIZED, "source_auth_required"),
        (K::Parse, StatusCode::BAD_GATEWAY, "source_parse"),
        (K::InvalidInput, StatusCode::BAD_REQUEST, "invalid_input"),
        (
            K::Internal,
            StatusCode::INTERNAL_SERVER_ERROR,
            "source_internal",
        ),
        (
            K::Unknown,
            StatusCode::INTERNAL_SERVER_ERROR,
            "source_error",
        ),
    ];

    for (kind, status, code) in cases {
        let res = source_failure(kind).into_response();
        assert_eq!(res.status(), status, "{kind:?} should map to {status}");
        let body = body_json(res).await;
        assert_eq!(body["code"], code, "{kind:?} should carry code {code}");
    }
}

#[tokio::test]
async fn a_source_failure_no_longer_collapses_into_a_generic_500() {
    use kani_shared::extension::ExtensionErrorKind as K;

    let res = source_failure(K::Network).into_response();
    let body = body_json(res).await;
    assert_eq!(
        body["error"], "the source said no",
        "an actionable source failure reaches the caller verbatim"
    );
}

#[tokio::test]
async fn internal_source_failures_do_not_leak_their_message() {
    use kani_shared::extension::ExtensionErrorKind as K;

    for kind in [K::Internal, K::Unknown] {
        let body = body_json(source_failure(kind).into_response()).await;
        assert_ne!(
            body["error"], "the source said no",
            "{kind:?} is not actionable and must not surface internals"
        );
    }
}

#[tokio::test]
async fn a_rate_limited_source_sends_retry_after() {
    let error = AppError::from(ServiceError::Core(kani_core::Error::Extension(
        kani_shared::extension::ExtensionError {
            kind: kani_shared::extension::ExtensionErrorKind::RateLimited,
            message: "slow down".into(),
            source_url: None,
            retry_after_secs: Some(42),
        },
    )));

    let res = error.into_response();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        res.headers().get("retry-after").unwrap(),
        "42",
        "the source's own number beats guessing with backoff"
    );
}

#[tokio::test]
async fn browser_capture_failures_map_to_flaresolverr_required() {
    for code in [
        "solver_not_configured",
        "solver_incompatible",
        "solver_unreachable",
        "solver_unauthorized",
    ] {
        let res = browser_unavailable(code).into_response();
        assert_eq!(
            res.status(),
            StatusCode::BAD_GATEWAY,
            "{code} should be a gateway failure, not a 500"
        );
        let body = body_json(res).await;
        assert_eq!(body["code"], "flaresolverr_required");
        assert!(
            body["hint"].is_string(),
            "{code} needs actionable guidance, got: {body}"
        );
    }
}

#[tokio::test]
async fn each_solver_state_gets_its_own_hint() {
    let mut hints = std::collections::HashSet::new();
    for code in [
        "solver_not_configured",
        "solver_incompatible",
        "solver_unreachable",
        "solver_unauthorized",
    ] {
        let body = body_json(browser_unavailable(code).into_response()).await;
        hints.insert(body["hint"].as_str().unwrap_or_default().to_string());
    }
    assert_eq!(hints.len(), 4, "each solver state needs its own diagnosis");
}
