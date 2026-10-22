#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::authed_get;
use std::sync::Arc;
use tower::ServiceExt;

#[test]
fn range_status_partial_content_maps_to_206() {
    use kani_web::proxy::range_response_status;
    assert_eq!(range_response_status(true), StatusCode::PARTIAL_CONTENT);
}

#[test]
fn range_status_ok_maps_to_200() {
    use kani_web::proxy::range_response_status;
    assert_eq!(range_response_status(false), StatusCode::OK);
}

#[test]
fn range_headers_relay_content_type_content_range_and_etag() {
    use kani_web::proxy::build_range_response_headers;
    let mut upstream = rquest::header::HeaderMap::new();
    upstream.insert(rquest::header::CONTENT_TYPE, "image/jpeg".parse().unwrap());
    upstream.insert(
        rquest::header::CONTENT_RANGE,
        "bytes 0-999/10000".parse().unwrap(),
    );
    let out = build_range_response_headers(&upstream, "\"etag123\"");
    assert_eq!(
        out.get(axum::http::header::CONTENT_TYPE).unwrap(),
        "image/jpeg"
    );
    assert_eq!(
        out.get(axum::http::header::CONTENT_RANGE).unwrap(),
        "bytes 0-999/10000"
    );
    assert_eq!(out.get(axum::http::header::ETAG).unwrap(), "\"etag123\"");
}

#[test]
fn range_headers_missing_content_range_leaves_it_absent() {
    use kani_web::proxy::build_range_response_headers;
    let upstream = rquest::header::HeaderMap::new();
    let out = build_range_response_headers(&upstream, "\"e\"");
    assert!(out.get(axum::http::header::CONTENT_RANGE).is_none());
    assert!(out.get(axum::http::header::ETAG).is_some());
}

#[tokio::test]
async fn proxy_coalesce_does_not_cache_errors() {
    use moka::future::Cache;
    use std::sync::atomic::{AtomicU32, Ordering};

    let cache: Cache<String, Arc<String>> = Cache::builder().max_capacity(10).build();
    let call_count = Arc::new(AtomicU32::new(0));

    for _ in 0..2 {
        let count = call_count.clone();
        let _: Result<Arc<String>, _> = cache
            .try_get_with("key".to_string(), async move {
                count.fetch_add(1, Ordering::Relaxed);
                Err::<Arc<String>, String>("upstream 500".to_string())
            })
            .await;
    }

    assert_eq!(
        call_count.load(Ordering::Relaxed),
        2,
        "init invoked twice — error was not cached after first failure"
    );
}

#[test]
fn canonical_key_strips_bust_params() {
    use kani_web::proxy::canonical_proxy_key;
    assert_eq!(
        canonical_proxy_key("https://img.example.com/a.jpg?cb=1&ts=2"),
        "https://img.example.com/a.jpg"
    );
}

#[test]
fn canonical_key_preserves_other_params() {
    use kani_web::proxy::canonical_proxy_key;
    let result = canonical_proxy_key("https://img.example.com/a.jpg?size=800&_=123");
    assert!(result.contains("size=800"), "size param must be kept");
    assert!(!result.contains("_="), "cache-bust _ must be stripped");
}

#[test]
fn canonical_key_no_bust_params_unchanged() {
    use kani_web::proxy::canonical_proxy_key;
    let url = "https://img.example.com/a.jpg?page=2";
    assert_eq!(canonical_proxy_key(url), url);
}

#[tokio::test]
async fn proxy_stats_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/admin/proxy/stats", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}
