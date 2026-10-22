#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{
    authed_delete, authed_get, authed_post, body_json, build_test_app_with_opds, create_admin,
    create_regular_user, login, test_state,
};
use tower::ServiceExt;

#[tokio::test]
async fn create_list_revoke_roundtrip() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, u, p).await;

    let res = app
        .clone()
        .oneshot(authed_post(
            "/rest/me/api-tokens",
            &cookie,
            serde_json::json!({ "name": "reader" }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let created = body_json(res).await;
    let raw = created["raw_token"].as_str().unwrap().to_owned();
    let id = created["id"].as_str().unwrap().to_owned();
    assert!(raw.starts_with("kani_"));

    let res = app
        .clone()
        .oneshot(authed_get("/rest/me/api-tokens", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let list = body_json(res).await;
    let arr = list.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "reader");
    assert!(arr[0].get("raw_token").is_none());
    assert!(arr[0].get("token_hash").is_none());

    let res = app
        .clone()
        .oneshot(authed_delete(&format!("/rest/me/api-tokens/{id}"), &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = app
        .clone()
        .oneshot(authed_get("/rest/me/api-tokens", &cookie))
        .await
        .unwrap();
    let list = body_json(res).await;
    assert!(list.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn create_rejects_empty_name() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, u, p).await;

    let res = app
        .clone()
        .oneshot(authed_post(
            "/rest/me/api-tokens",
            &cookie,
            serde_json::json!({ "name": "   " }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn users_are_isolated_from_each_others_tokens() {
    let state = test_state().await;
    let (au, ap) = create_admin(&state).await;
    let (bu, bp) = create_regular_user(&state, "bob").await;
    let app = build_test_app_with_opds(state).await;

    let admin_cookie = login(&app, au, ap).await;
    let bob_cookie = login(&app, bu, bp).await;

    let res = app
        .clone()
        .oneshot(authed_post(
            "/rest/me/api-tokens",
            &admin_cookie,
            serde_json::json!({ "name": "admin-token" }),
        ))
        .await
        .unwrap();
    let id = body_json(res).await["id"].as_str().unwrap().to_owned();

    let res = app
        .clone()
        .oneshot(authed_get("/rest/me/api-tokens", &bob_cookie))
        .await
        .unwrap();
    assert!(body_json(res).await.as_array().unwrap().is_empty());

    let res = app
        .clone()
        .oneshot(authed_delete(
            &format!("/rest/me/api-tokens/{id}"),
            &bob_cookie,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn token_timestamps_are_rfc3339_strings() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, u, p).await;

    let res = app
        .clone()
        .oneshot(authed_post(
            "/rest/me/api-tokens",
            &cookie,
            serde_json::json!({ "name": "reader", "expires_in_days": 30 }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let created = body_json(res).await;

    for field in ["created_at", "expires_at"] {
        let raw = &created[field];
        let text = raw.as_str().unwrap_or_else(|| {
            panic!(
                "{field} must be an RFC 3339 string, got {raw}; a bare epoch integer forces \
                    every client to guess seconds versus milliseconds"
            )
        });
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|e| panic!("{field} = {text:?} does not parse as RFC 3339: {e}"));
    }

    let res = app
        .clone()
        .oneshot(authed_get("/rest/me/api-tokens", &cookie))
        .await
        .unwrap();
    let list = body_json(res).await;
    let listed = &list.as_array().unwrap()[0];
    assert!(
        listed["created_at"].is_string(),
        "the listing must agree with the creation response, got {}",
        listed["created_at"]
    );
}
