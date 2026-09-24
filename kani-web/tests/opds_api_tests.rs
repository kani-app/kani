#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use axum::{body::Body, http::Request};
use common::{
    basic_auth, build_test_app_with_opds, create_admin, insert_manga, insert_source, login,
    test_state,
};
use http_body_util::BodyExt as _;
use tower::ServiceExt;

fn opds_req(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn opds_authed_req(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("Cookie", cookie)
        .body(Body::empty())
        .unwrap()
}

fn opds_basic_auth_req(uri: &str, username: &str, password: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("Authorization", basic_auth(username, password))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn opds_root_returns_200_with_session_auth() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(opds_authed_req("/opds", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let content_type = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("atom+xml"),
        "Expected Atom XML content-type, got: {content_type}"
    );
}

#[tokio::test]
async fn opds_root_returns_200_with_basic_auth() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;

    let res = app
        .oneshot(opds_basic_auth_req("/opds", username, password))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn opds_root_returns_401_with_wrong_basic_auth() {
    let state = test_state().await;
    let (_username, _password) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;

    let res = app
        .oneshot(opds_basic_auth_req("/opds", "admin", "wrongpassword"))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn opds_catalogue_returns_200_for_empty_library() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(opds_authed_req("/opds/catalogue", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn opds_catalogue_includes_seeded_manga() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let src = insert_source(&state.db, "src").await;
    insert_manga(&state.db, src, "m1", "Dragon Ball").await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(opds_authed_req("/opds/catalogue", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let xml = std::str::from_utf8(&bytes).unwrap();
    assert!(
        xml.contains("Dragon Ball"),
        "Feed should contain seeded manga title"
    );
}

#[tokio::test]
async fn opds_manga_returns_404_for_missing_id() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(opds_authed_req("/opds/manga/999999", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn opds_manga_returns_200_for_existing_manga() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let src = insert_source(&state.db, "src").await;
    let manga_id = insert_manga(&state.db, src, "m1", "Naruto").await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(opds_authed_req(&format!("/opds/manga/{manga_id}"), &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn opds_search_returns_200_with_auth() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(opds_authed_req("/opds/search?q=dragon+ball", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn opds_opensearch_returns_200_with_auth() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(opds_authed_req("/opds/opensearch", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let content_type = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("xml"),
        "Expected XML content-type for OpenSearch, got: {content_type}"
    );
}

#[tokio::test]
async fn opds_root_feed_contains_catalogue_link() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(opds_authed_req("/opds", &cookie))
        .await
        .unwrap();

    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let xml = std::str::from_utf8(&bytes).unwrap();
    assert!(
        xml.contains("/opds/catalogue"),
        "Root feed should link to catalogue"
    );
    assert!(
        xml.contains("urn:kani:root"),
        "Root feed should have expected id"
    );
}

#[tokio::test]
async fn opds_reflects_what_the_source_actually_returned() {
    use kani_app::source::{SourceBackend, YamlSource};
    use kani_shared::ast::Expr;
    use kani_shared_test::origin::{Response, TestOrigin};
    use kani_yaml::yaml::model::{
        FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedHnp,
        ValidatedTotalPages,
    };
    use kani_yaml::yaml::schema::ResponseType;
    use std::collections::HashMap;
    use std::sync::Arc;

    let origin = TestOrigin::start().await;
    origin.set(
        "/chapters/m1",
        Response::json(
            r#"{"chapters":[{"id":"c1","number":1,"title":"The Wasteland"},
                            {"id":"c2","number":2,"title":"Nightfall"}]}"#,
        ),
    );

    let state = test_state().await;
    let (username, password) = create_admin(&state).await;
    let src = insert_source(&state.db, "src").await;
    let manga = insert_manga(&state.db, src, "m1", "Dragon Ball").await;

    let field = |name: &str, ptr: &str| ValidatedField {
        name: name.to_string(),
        source: FieldSource::Blueprint(Expr::JsonPtr {
            target: Box::new(Expr::SelfRef),
            pointer: ptr.to_string(),
        }),
        optional: false,
    };
    let chapter_list = ValidatedEndpoint {
        route: "/chapters/$manga_id$".into(),
        method: "GET".into(),
        headers: vec![],
        queries: vec![],
        filter_mapping: vec![],
        filter_format: None,
        response_type: ResponseType::Json,
        container: "/chapters".into(),
        bindings: vec![],
        fields: vec![
            field("id", "/id"),
            field("number", "/number"),
            field("title", "/title"),
        ],
        scalars: vec![],
        has_next_page: ValidatedHnp::Static(false),
        total_pages: ValidatedTotalPages::None,
        pagination: None,
        composite_id_decodes: vec![],
        then_steps: vec![],
        for_each_steps: vec![],
        via: None,
        page_url: None,
        script_name: None,
        timeout_ms: 10_000,
        auto_scroll: true,
    };
    let ext = ValidatedExtension {
        id: "src".into(),
        name: "Src".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        chapter_list: Some(chapter_list),
        ..Default::default()
    };
    state.service.sources.insert(
        src,
        SourceBackend::Yaml(Box::new(YamlSource::new(
            Arc::new(ext),
            kani_core::http::SmartClient::new(None)
                .unwrap()
                .with_allow_loopback_egress(true),
            Arc::new(kani_core::cache::InMemoryCache::new()),
            "opds:".into(),
            HashMap::new(),
            true,
        ))),
    );

    state
        .service
        .fetch_and_store_chapters_silent(manga)
        .await
        .unwrap();

    sqlx::query("UPDATE chapters SET download_status = 2 WHERE manga_id = ?")
        .bind(manga.0)
        .execute(&state.db)
        .await
        .unwrap();

    let app = build_test_app_with_opds(state).await;
    let cookie = login(&app, username, password).await;
    let res = app
        .oneshot(opds_authed_req(
            &format!("/opds/manga/{}", manga.0),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let xml = std::str::from_utf8(&bytes).unwrap();
    assert!(
        xml.contains("The Wasteland") && xml.contains("Nightfall"),
        "the feed must list the chapters the source served, got: {xml}"
    );
}

/// Every OPDS entry point challenges an anonymous reader. OPDS is exempt from
/// the session guard because clients authenticate by HTTP Basic, so this is the
/// only thing asserting the feed is not open, and the challenge header is part
/// of the contract — a reader cannot log in without it.
#[tokio::test]
async fn every_opds_entry_point_challenges_an_anonymous_reader() {
    let state = test_state().await;
    let app = build_test_app_with_opds(state).await;

    for path in [
        "/opds",
        "/opds/catalogue",
        "/opds/manga/1",
        "/opds/search?q=test",
        "/opds/opensearch",
    ] {
        let res = app.clone().oneshot(opds_req(path)).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNAUTHORIZED,
            "{path} must not serve an anonymous reader"
        );
        let www_auth = res
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            www_auth.contains("Basic"),
            "{path} must challenge with Basic, got: {www_auth}"
        );
    }
}
