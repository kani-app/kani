#![allow(clippy::unwrap_used)]

mod common;
use axum::http::{StatusCode, header};
use common::{build_test_app, create_admin, test_state};
use tower::ServiceExt;

fn get_with(path: &str, cookie: &str, inm: Option<&str>) -> axum::http::Request<axum::body::Body> {
    let mut req = common::authed_get(path, cookie);
    if let Some(v) = inm {
        req.headers_mut()
            .insert(header::IF_NONE_MATCH, v.parse().unwrap());
    }
    req
}

const TAGGED: [&str; 3] = [
    "/rest/library?page=1&page_size=20",
    "/rest/library/1/0",
    "/rest/manga/1/chapters?page=1&page_size=20",
];

#[tokio::test]
async fn a_tagged_list_returns_an_etag_and_then_a_304() {
    let (app, cookie) = common::admin_app().await;

    for path in TAGGED {
        let first = app
            .clone()
            .oneshot(get_with(path, &cookie, None))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK, "{path}");
        let tag = first
            .headers()
            .get(header::ETAG)
            .unwrap_or_else(|| panic!("{path} was not tagged"))
            .to_str()
            .unwrap()
            .to_owned();

        let second = app
            .clone()
            .oneshot(get_with(path, &cookie, Some(&tag)))
            .await
            .unwrap();
        assert_eq!(
            second.status(),
            StatusCode::NOT_MODIFIED,
            "{path} re-sent an unchanged body"
        );
        let body = common::body_bytes(second).await;
        assert!(body.is_empty(), "{path} sent a body with its 304");
    }
}

#[tokio::test]
async fn a_stale_tag_gets_the_full_body() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(get_with(
            "/rest/library?page=1&page_size=20",
            &cookie,
            Some("\"deadbeefdeadbeef\""),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(!common::body_bytes(res).await.is_empty());
}

#[tokio::test]
async fn the_tag_follows_the_library_contents() {
    let state = test_state().await;
    let (u, p) = create_admin(&state).await;
    let db = state.db.clone();
    let cache = state.service.cache.clone();
    let app = build_test_app(state).await;
    let cookie = common::login(&app, u, p).await;

    let before = app
        .clone()
        .oneshot(get_with("/rest/library?page=1&page_size=20", &cookie, None))
        .await
        .unwrap();
    let tag = before
        .headers()
        .get(header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    let src_id: i64 = sqlx::query_scalar(
        "INSERT INTO sources (name, version) VALUES ('src', '0.1') RETURNING id",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO manga (source_id, source_manga_id, name, status) \
         VALUES (?, 'mid', 'New', 0)",
    )
    .bind(src_id)
    .execute(&db)
    .await
    .unwrap();
    cache.invalidate_library();

    let after = app
        .clone()
        .oneshot(get_with(
            "/rest/library?page=1&page_size=20",
            &cookie,
            Some(&tag),
        ))
        .await
        .unwrap();
    assert_eq!(
        after.status(),
        StatusCode::OK,
        "a 304 here would hide a new manga from every polling client until \
         something else changed"
    );
    assert_ne!(after.headers().get(header::ETAG).unwrap(), tag.as_str());
}

#[tokio::test]
async fn a_different_query_gets_a_different_tag() {
    let (app, cookie) = common::admin_app().await;

    let a = app
        .clone()
        .oneshot(get_with("/rest/library?page=1&page_size=20", &cookie, None))
        .await
        .unwrap();
    let b = app
        .clone()
        .oneshot(get_with(
            "/rest/manga/1/chapters?page=1&page_size=20",
            &cookie,
            None,
        ))
        .await
        .unwrap();
    assert_ne!(
        a.headers().get(header::ETAG).unwrap(),
        b.headers().get(header::ETAG).unwrap()
    );
}

#[tokio::test]
async fn a_streamed_response_is_left_alone() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(get_with("/rest/library/backup", &cookie, None))
        .await
        .unwrap();
    assert!(
        res.headers().get(header::ETAG).is_none(),
        "the layer must not have been applied router-wide"
    );
}

#[tokio::test]
async fn a_tagged_per_user_list_is_not_shared_cacheable() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(get_with("/rest/library?page=1&page_size=20", &cookie, None))
        .await
        .unwrap();
    let cc = res
        .headers()
        .get(header::CACHE_CONTROL)
        .expect("an ETag invites caching, so the policy must be stated")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        cc.contains("private"),
        "a shared cache could otherwise hand one user's library to another; got {cc}"
    );
    assert!(
        cc.contains("no-cache"),
        "the tag promises revalidation; got {cc}"
    );
}
