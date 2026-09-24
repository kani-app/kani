#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{
    authed_get, authed_post, body_array, body_json, build_test_app, create_regular_user, login,
    put_json, test_state,
};
use tower::ServiceExt;

/// Creates a source as admin and returns its id.
async fn create_source(app: &axum::Router, cookie: &str, name: &str) -> i64 {
    let res = app
        .clone()
        .oneshot(authed_post(
            "/rest/sources",
            cookie,
            serde_json::json!({ "name": name }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    body_json(res).await["id"].as_i64().expect("numeric id")
}

#[tokio::test]
async fn list_sources_returns_empty_list_on_fresh_db() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/sources", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let sources = body_array(res).await;
    assert!(sources.is_empty(), "fresh DB should have no sources");
}

#[tokio::test]
async fn add_source_requires_source_install_permission() {
    let state = test_state().await;
    let (username, password) = create_regular_user(&state, "bob").await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_post(
            "/rest/sources",
            &cookie,
            serde_json::json!({"name": "my-source"}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let body = body_json(res).await;
    assert_eq!(body["code"], serde_json::json!("forbidden"));
}

#[tokio::test]
async fn add_source_returns_201_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(authed_post(
            "/rest/sources",
            &cookie,
            serde_json::json!({"name": "test-source"}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CREATED);
    let body = body_json(res).await;
    assert!(
        body["id"].is_number(),
        "response must contain numeric id, got: {body}"
    );

    let list_res = app
        .oneshot(authed_get("/rest/sources", &cookie))
        .await
        .unwrap();
    let sources = body_array(list_res).await;
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["name"], serde_json::json!("test-source"));
}

#[tokio::test]
async fn add_source_returns_400_for_empty_name() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/sources",
            &cookie,
            serde_json::json!({"name": ""}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_source_returns_200_for_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let create_res = app
        .clone()
        .oneshot(authed_post(
            "/rest/sources",
            &cookie,
            serde_json::json!({"name": "fetch-me"}),
        ))
        .await
        .unwrap();
    assert_eq!(create_res.status(), StatusCode::CREATED);
    let created = body_json(create_res).await;
    let id = created["id"].as_i64().expect("id must be numeric");

    let res = app
        .oneshot(authed_get(&format!("/rest/sources/{id}"), &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["name"], serde_json::json!("fetch-me"));
}

#[tokio::test]
async fn set_browser_enabled_returns_200_and_persists_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let id = create_source(&app, &cookie, "browser-src").await;

    let res = app
        .clone()
        .oneshot(put_json(
            &format!("/rest/sources/{id}/browser-enabled"),
            &cookie,
            serde_json::json!({ "enabled": false }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let get_res = app
        .oneshot(authed_get(&format!("/rest/sources/{id}"), &cookie))
        .await
        .unwrap();
    let body = body_json(get_res).await;
    assert_eq!(body["browser_enabled"], serde_json::json!(false));
}

#[tokio::test]
async fn set_browser_enabled_rejects_invalid_body() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(put_json(
            "/rest/sources/1/browser-enabled",
            &cookie,
            serde_json::json!({ "wrong_field": true }),
        ))
        .await
        .unwrap();

    assert!(
        res.status().is_client_error(),
        "missing `enabled` field should be a 4xx, got {}",
        res.status()
    );
}

#[tokio::test]
async fn bulk_capabilities_returns_200_with_auth() {
    let (app, cookie) = common::admin_app().await;

    create_source(&app, &cookie, "alpha").await;
    create_source(&app, &cookie, "beta").await;

    let res = app
        .clone()
        .oneshot(authed_get("/rest/sources/capabilities", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = body_array(res).await;
    assert_eq!(body.len(), 2, "one entry per installed source");
    for entry in &body {
        assert!(
            entry.get("source_id").is_some(),
            "each entry names its source"
        );
        assert_eq!(
            entry.get("streaming_chapters").and_then(|v| v.as_bool()),
            Some(true),
            "capability flags are flattened, not nested"
        );
    }
}

#[tokio::test]
async fn bulk_capabilities_is_empty_not_an_error_with_no_sources() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(authed_get("/rest/sources/capabilities", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body_array(res).await.is_empty());
}

#[tokio::test]
async fn bulk_route_is_not_swallowed_by_the_per_source_route() {
    let (app, cookie) = common::admin_app().await;
    let id = create_source(&app, &cookie, "gamma").await;

    let bulk = app
        .clone()
        .oneshot(authed_get("/rest/sources/capabilities", &cookie))
        .await
        .unwrap();
    assert_eq!(bulk.status(), StatusCode::OK, "bulk route must win");

    let single = app
        .clone()
        .oneshot(authed_get(
            &format!("/rest/sources/{id}/capabilities"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(single.status(), StatusCode::OK);
    let one = body_json(single).await;
    assert_eq!(
        one.get("streaming_chapters").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[tokio::test]
async fn bulk_and_per_source_agree() {
    let (app, cookie) = common::admin_app().await;
    let id = create_source(&app, &cookie, "delta").await;

    let bulk = body_array(
        app.clone()
            .oneshot(authed_get("/rest/sources/capabilities", &cookie))
            .await
            .unwrap(),
    )
    .await;
    let single = body_json(
        app.clone()
            .oneshot(authed_get(
                &format!("/rest/sources/{id}/capabilities"),
                &cookie,
            ))
            .await
            .unwrap(),
    )
    .await;

    let from_bulk = bulk
        .iter()
        .find(|e| e.get("source_id").and_then(|v| v.as_i64()) == Some(id))
        .expect("the created source appears in the bulk listing");
    assert_eq!(
        from_bulk.get("streaming_chapters"),
        single.get("streaming_chapters"),
        "bulk and per-source disagree about the same source"
    );
}

fn wasm_upload(cookie: Option<&str>, bytes: &[u8]) -> axum::http::Request<axum::body::Body> {
    let boundary = "kani-test-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"x.wasm\"\r\n\
         Content-Type: application/wasm\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri("/rest/sources/wasm")
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        );
    if let Some(cookie) = cookie {
        builder = builder
            .header("Cookie", common::csrf_cookie(cookie))
            .header("X-CSRF-Token", common::csrf_token(cookie));
    }
    builder.body(axum::body::Body::from(body)).unwrap()
}

fn fixture_wasm() -> Vec<u8> {
    std::fs::read(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("wasm_sources")
            .join("fixture.wasm"),
    )
    .expect("wasm_sources/fixture.wasm: cargo run -p kani-cli -- build kani-fixture-source")
}

#[tokio::test]
async fn install_wasm_creates_the_source_named_by_the_artifact() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(wasm_upload(Some(&cookie), &fixture_wasm()))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let id = body_json(res).await["id"].as_i64().expect("numeric id");
    let source = body_json(
        app.oneshot(authed_get(&format!("/rest/sources/{id}"), &cookie))
            .await
            .unwrap(),
    )
    .await;
    let name = source["name"].as_str().unwrap_or_default();
    assert!(
        !name.is_empty() && !name.starts_with("pending-"),
        "the row is named by the artifact, got {name:?}"
    );
}

#[tokio::test]
async fn install_wasm_requires_authentication() {
    let (app, _) = common::admin_app().await;
    let res = app
        .oneshot(wasm_upload(None, &fixture_wasm()))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn install_wasm_rejects_bytes_that_are_not_an_extension() {
    let (app, cookie) = common::admin_app().await;
    let res = app
        .oneshot(wasm_upload(Some(&cookie), b"not a wasm module"))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
