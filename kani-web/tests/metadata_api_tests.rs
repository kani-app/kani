#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{
    authed_get, authed_post, body_array, body_json, build_test_app, create_admin, insert_manga,
    insert_source, login, test_state,
};
use tower::ServiceExt;

#[tokio::test]
async fn list_metadata_providers_returns_200_for_authed_user() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/sources/metadata-providers", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let providers = body_array(res).await;
    assert!(
        !providers.is_empty(),
        "stub provider should always be present"
    );
    assert!(
        providers.iter().any(|p| p["id"] == "stub"),
        "stub provider must be listed"
    );
}

#[tokio::test]
async fn enrich_manga_metadata_returns_200_with_stub_provider() {
    let state = test_state().await;
    let pool = state.db.clone();
    let source_id = insert_source(&pool, "Test Source").await;
    let manga_id = insert_manga(&pool, source_id, "test-manga-1", "Test Manga").await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_post(
            &format!("/rest/manga/{}/enrich-metadata", manga_id.0),
            &cookie,
            serde_json::json!({ "provider": "stub" }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    let fields = body["fields_updated"].as_array().unwrap();
    assert!(
        fields.iter().any(|f| f == "description"),
        "stub provider should fill description"
    );
}

#[tokio::test]
async fn enrich_manga_metadata_returns_404_for_unknown_provider() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/manga/1/enrich-metadata",
            &cookie,
            serde_json::json!({ "provider": "does-not-exist" }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
