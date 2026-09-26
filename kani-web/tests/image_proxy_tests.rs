#![allow(clippy::unwrap_used)]

//! Image-proxy fetch behavior, driven end-to-end through a signed
//! token against a real upstream `TestOrigin`. Tests shorten retry, timeout, and
//! size limits through `AppState::proxy_config`.

mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{authed_get, build_test_app_with_proxy, create_admin, login, test_state};
use kani_shared_test::origin::{Body as OriginBody, Response, TestOrigin};
use kani_web::proxy::{ProxyConfig, make_proxy_url};
use kani_web::state::AppState;
use std::time::Duration;
use tower::ServiceExt;

/// Fast timings with room to override individual fields.
fn fast_proxy_config() -> ProxyConfig {
    ProxyConfig {
        base_delay: Duration::from_millis(1),
        retry_jitter: Duration::ZERO,
        min_host_interval: Duration::ZERO,
        ..ProxyConfig::default()
    }
}

fn signed_get(state: &AppState, upstream_url: &str, cookie: &str) -> Request<Body> {
    let signed = make_proxy_url(
        upstream_url,
        "http://ref.test/",
        None,
        &state.proxy_secret,
        None,
    );
    authed_get(&signed, cookie)
}

#[tokio::test]
async fn the_image_proxy_retries_a_429_then_succeeds() {
    let origin = TestOrigin::start().await;
    origin.script(
        "/img.jpg",
        vec![
            Response::status(429),
            Response::image(kani_shared_test::origin::jpeg_page(16, 16, false, 80)),
        ],
    );
    let mut state = test_state().await;
    state.proxy_config = fast_proxy_config();
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_proxy(state.clone()).await;
    let cookie = login(&app, u, p).await;

    let res = app
        .oneshot(signed_get(&state, &origin.url("/img.jpg"), &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        &body[..],
        &kani_shared_test::origin::jpeg_page(16, 16, false, 80)[..],
        "the retried request served the image"
    );
    assert_eq!(origin.hits("/img.jpg"), 2, "the 429 was retried once");
}

#[tokio::test]
async fn the_image_proxy_retries_cloudflare_origin_errors() {
    for status in [522, 523] {
        let origin = TestOrigin::start().await;
        origin.script(
            "/img.jpg",
            vec![
                Response::status(status),
                Response::image(kani_shared_test::origin::jpeg_page(16, 16, false, 80)),
            ],
        );
        let mut state = test_state().await;
        state.proxy_config = fast_proxy_config();
        let (u, p) = create_admin(&state).await;
        let app = build_test_app_with_proxy(state.clone()).await;
        let cookie = login(&app, u, p).await;

        let res = app
            .oneshot(signed_get(&state, &origin.url("/img.jpg"), &cookie))
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK, "a {status} was retried");
        assert_eq!(origin.hits("/img.jpg"), 2, "the {status} was retried once");
    }
}

#[tokio::test]
async fn the_image_proxy_times_out_rather_than_hanging() {
    let origin = TestOrigin::start().await;
    origin.set("/img.jpg", Response::status(200).body(OriginBody::Stall));
    let mut state = test_state().await;
    state.proxy_config = ProxyConfig {
        request_timeout: Duration::from_millis(150),
        max_retries: 1,
        ..fast_proxy_config()
    };
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_proxy(state.clone()).await;
    let cookie = login(&app, u, p).await;

    let res = tokio::time::timeout(
        Duration::from_secs(10),
        app.oneshot(signed_get(&state, &origin.url("/img.jpg"), &cookie)),
    )
    .await
    .expect("the proxy must return, not hang, on a stalling upstream")
    .unwrap();

    assert!(
        res.status().is_server_error(),
        "a timed-out upstream surfaces as an error, got {}",
        res.status()
    );
}

#[tokio::test]
async fn concurrent_proxy_requests_for_one_url_coalesce() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/img.jpg",
        Response::image(kani_shared_test::origin::jpeg_page(16, 16, false, 80))
            .delay(Duration::from_millis(200)),
    );
    let mut state = test_state().await;
    state.proxy_config = fast_proxy_config();
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_proxy(state.clone()).await;
    let cookie = login(&app, u, p).await;

    let mut handles = Vec::new();
    for _ in 0..5 {
        let app = app.clone();
        let req = signed_get(&state, &origin.url("/img.jpg"), &cookie);
        handles.push(tokio::spawn(async move { app.oneshot(req).await }));
    }
    for h in handles {
        let res = h.await.unwrap().unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    assert_eq!(
        origin.hits("/img.jpg"),
        1,
        "five concurrent requests hit the upstream once"
    );
}

#[tokio::test]
async fn an_oversized_image_is_capped() {
    let origin = TestOrigin::start().await;
    origin.set("/img.jpg", Response::image(vec![7; 4096]));
    let mut state = test_state().await;
    state.proxy_config = ProxyConfig {
        max_image_bytes: 64,
        ..fast_proxy_config()
    };
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_proxy(state.clone()).await;
    let cookie = login(&app, u, p).await;

    let res = app
        .oneshot(signed_get(&state, &origin.url("/img.jpg"), &cookie))
        .await
        .unwrap();

    assert!(
        res.status().is_server_error(),
        "a body past the cap is refused, got {}",
        res.status()
    );
}

#[tokio::test]
async fn an_image_served_as_octet_stream_is_proxied() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/img",
        Response::ok(kani_shared_test::origin::jpeg_page(32, 48, false, 80))
            .header("Content-Type", "application/octet-stream"),
    );
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_proxy(state.clone()).await;
    let cookie = login(&app, u, p).await;

    let res = app
        .oneshot(signed_get(&state, &origin.url("/img"), &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK, "a real JPEG must be served");
    assert_eq!(
        res.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("image/jpeg"),
        "the served type comes from the bytes, not from the upstream's label"
    );
}

#[tokio::test]
async fn a_page_labelled_as_an_image_is_not_proxied() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/not-img",
        Response::ok(b"<html><body>Just a moment...</body></html>".to_vec())
            .header("Content-Type", "image/png"),
    );
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_proxy(state.clone()).await;
    let cookie = login(&app, u, p).await;

    let res = app
        .oneshot(signed_get(&state, &origin.url("/not-img"), &cookie))
        .await
        .unwrap();

    assert!(
        !res.status().is_success(),
        "an HTML page claiming to be a PNG must not be served as an image, got {}",
        res.status()
    );
}

#[tokio::test]
async fn an_upstream_that_ignores_range_still_serves_the_reader() {
    let origin = TestOrigin::start().await;
    let image = vec![9u8; 2048];
    origin.set("/img.jpg", Response::image(image.clone()));
    origin.ignore_range(true);

    let mut state = test_state().await;
    state.proxy_config = fast_proxy_config();
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_proxy(state.clone()).await;
    let cookie = login(&app, u, p).await;

    let signed = make_proxy_url(
        &origin.url("/img.jpg"),
        "http://ref.test/",
        None,
        &state.proxy_secret,
        None,
    );
    let mut req = authed_get(&signed, &cookie);
    req.headers_mut().insert(
        axum::http::header::RANGE,
        axum::http::HeaderValue::from_static("bytes=0-511"),
    );

    let res = app.oneshot(req).await.unwrap();

    assert_eq!(
        res.status(),
        StatusCode::OK,
        "an ignored range is a complete response, not partial content"
    );
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        body.len(),
        image.len(),
        "the reader still receives the whole usable image"
    );
}

#[tokio::test]
async fn a_cached_image_skips_the_upstream_throttle() {
    const INTERVAL: Duration = Duration::from_millis(300);

    let origin = TestOrigin::start().await;
    origin.set(
        "/img.jpg",
        Response::image(kani_shared_test::origin::jpeg_page(16, 16, false, 80)),
    );
    let mut state = test_state().await;
    state.proxy_config = ProxyConfig {
        min_host_interval: INTERVAL,
        ..fast_proxy_config()
    };
    let (u, p) = create_admin(&state).await;
    let app = build_test_app_with_proxy(state.clone()).await;
    let cookie = login(&app, u, p).await;

    let first = app
        .clone()
        .oneshot(signed_get(&state, &origin.url("/img.jpg"), &cookie))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let started = std::time::Instant::now();
    let second = app
        .clone()
        .oneshot(signed_get(&state, &origin.url("/img.jpg"), &cookie))
        .await
        .unwrap();
    let elapsed = started.elapsed();

    assert_eq!(second.status(), StatusCode::OK);
    let body = axum::body::to_bytes(second.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        &body[..],
        &kani_shared_test::origin::jpeg_page(16, 16, false, 80)[..],
        "the cached image was served intact"
    );
    assert_eq!(
        origin.hits("/img.jpg"),
        1,
        "the second request was served from cache, not refetched"
    );
    assert!(
        elapsed < INTERVAL,
        "a cache hit waited {elapsed:?}, which is at least the {INTERVAL:?} host interval it does not owe"
    );
}
