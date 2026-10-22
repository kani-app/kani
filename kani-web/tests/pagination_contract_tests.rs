#![allow(clippy::unwrap_used)]

//! Paginated GETs must honour the contract their OpenAPI parameters advertise.
//!
//! Generated clients may omit optional `page` and `page_size` parameters.
//!
//! These tests pin the documented shape: omitting paging must succeed. Assert on
//! the *status*, not on the body, so they stay valid as the payloads evolve.

mod common;
use axum::http::StatusCode;
use common::authed_get;
use tower::ServiceExt;

/// Endpoints whose `page`/`page_size` parameters are documented optional.
const PAGINATED: &[&str] = &[
    "/rest/library",
    "/rest/recent_updates",
    "/rest/admin/logs",
    "/rest/admin/audit-log",
];

#[tokio::test]
async fn paginated_endpoints_accept_a_request_with_no_paging_params() {
    let (app, cookie) = common::admin_app().await;

    for path in PAGINATED {
        let res = app
            .clone()
            .oneshot(authed_get(path, &cookie))
            .await
            .unwrap();
        assert_ne!(
            res.status(),
            StatusCode::BAD_REQUEST,
            "{path} documents page/page_size as optional but rejected a request that omitted them"
        );
        assert!(
            res.status().is_success(),
            "{path} without paging params returned {}",
            res.status()
        );
    }
}

#[tokio::test]
async fn global_search_accepts_only_its_required_query_param() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/global_search?query=whatever", &cookie))
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "global_search with only `query` returned {}",
        res.status()
    );
}

#[tokio::test]
async fn chapter_listing_defaults_its_page_when_omitted() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/manga/1/chapters", &cookie))
        .await
        .unwrap();
    assert_ne!(
        res.status(),
        StatusCode::BAD_REQUEST,
        "chapter listing rejected a request with no `page`, but documents it as optional"
    );
}

#[tokio::test]
async fn an_explicitly_invalid_page_is_still_rejected() {
    let (app, cookie) = common::admin_app().await;

    for path in ["/rest/library?page=0", "/rest/library?page_size=99999"] {
        let res = app
            .clone()
            .oneshot(authed_get(path, &cookie))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::BAD_REQUEST,
            "{path} should still fail validation"
        );
    }
}
