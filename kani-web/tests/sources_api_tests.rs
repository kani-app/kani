#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{authed_get, authed_post, body_array, body_json, put_json};
use tower::ServiceExt;

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
async fn get_source_returns_200_for_authed_user() {
    let (app, cookie, state) = common::admin_app_with_state().await;
    let id = kani_shared_test::insert_source(&state.db, "fetch-me").await;

    let res = app
        .oneshot(authed_get(&format!("/rest/sources/{id}"), &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["name"], serde_json::json!("fetch-me"));
}

#[tokio::test]
async fn an_empty_source_cannot_be_created() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/sources",
            &cookie,
            serde_json::json!({ "name": "placeholder" }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn set_browser_enabled_returns_200_and_persists_for_admin() {
    let (app, cookie, state) = common::admin_app_with_state().await;
    let id = kani_shared_test::insert_source(&state.db, "browser-src").await;

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
    let (app, cookie, state) = common::admin_app_with_state().await;

    kani_shared_test::insert_source(&state.db, "alpha").await;
    kani_shared_test::insert_source(&state.db, "beta").await;

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
    let (app, cookie, state) = common::admin_app_with_state().await;
    let id = kani_shared_test::insert_source(&state.db, "gamma").await;

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
    let (app, cookie, state) = common::admin_app_with_state().await;
    let id = kani_shared_test::insert_source(&state.db, "delta").await;

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

#[tokio::test]
async fn an_admin_can_grant_a_source_local_hosts() {
    let (app, cookie, state) = common::admin_app_with_state().await;
    let id = kani_shared_test::insert_source(&state.db, "komga").await;

    let res = app
        .clone()
        .oneshot(put_json(
            &format!("/rest/sources/{id}/local-hosts"),
            &cookie,
            serde_json::json!({ "hosts": ["komga.lan:25600", " 192.168.1.20 "] }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        body_json(res).await["hosts"],
        serde_json::json!(["komga.lan:25600", "192.168.1.20"])
    );

    let res = app
        .oneshot(authed_get(
            &format!("/rest/sources/{id}/local-hosts"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(
        body_json(res).await["hosts"],
        serde_json::json!(["komga.lan:25600", "192.168.1.20"])
    );
}

#[tokio::test]
async fn local_hosts_require_authentication() {
    let (app, _, state) = common::admin_app_with_state().await;
    let id = kani_shared_test::insert_source(&state.db, "komga").await;
    let res = app
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/rest/sources/{id}/local-hosts"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_regular_user_cannot_grant_local_hosts() {
    let state = common::test_state().await;
    let id = kani_shared_test::insert_source(&state.db, "komga").await;
    let (username, password) = common::create_regular_user(&state, "reader").await;
    let app = common::build_test_app(state).await;
    let cookie = common::login(&app, username, password).await;
    let res = app
        .oneshot(put_json(
            &format!("/rest/sources/{id}/local-hosts"),
            &cookie,
            serde_json::json!({ "hosts": ["komga.lan"] }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_host_that_cannot_be_granted_is_refused_and_nothing_is_stored() {
    let (app, cookie, state) = common::admin_app_with_state().await;
    let id = kani_shared_test::insert_source(&state.db, "komga").await;
    for host in [
        "127.0.0.1",
        "169.254.169.254",
        "localhost",
        "http://komga.lan/",
    ] {
        let res = app
            .clone()
            .oneshot(put_json(
                &format!("/rest/sources/{id}/local-hosts"),
                &cookie,
                serde_json::json!({ "hosts": ["komga.lan", host] }),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{host}");
    }
    let res = app
        .oneshot(authed_get(
            &format!("/rest/sources/{id}/local-hosts"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(body_json(res).await["hosts"], serde_json::json!([]));
}
