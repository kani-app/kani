#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{
    authed_delete, authed_get, authed_post, body_json, build_test_app, create_admin, login,
    test_state,
};
use tower::ServiceExt;

use ed25519_dalek::SigningKey;
use kani_app::source::signing::pubkey_b64;

fn gen_key() -> SigningKey {
    let bytes: [u8; 32] = rand::random();
    SigningKey::from_bytes(&bytes)
}

fn pk_b64(key: &SigningKey) -> String {
    pubkey_b64(key)
}

/// Seeds trusted repository state without exercising repository enrollment.
async fn seed_repo(
    state: &kani_web::state::AppState,
    url: &str,
    name: &str,
    key: &SigningKey,
) -> i64 {
    use kani_app::source::signing::{sign_artifact, signature_b64};
    let pk = pk_b64(key);
    let index_json = serde_json::json!({"name": name, "maintainer_key": pk, "extensions": []});
    let index_str = serde_json::to_string(&index_json).unwrap();
    let sig = signature_b64(&sign_artifact(index_str.as_bytes(), key));
    sqlx::query_scalar!(
        "INSERT INTO repo_trust (url, name, maintainer_key, index_cache, index_sig) \
         VALUES (?, ?, ?, ?, ?) RETURNING id",
        url,
        name,
        pk,
        index_str,
        sig
    )
    .fetch_one(&state.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn list_repos_returns_empty_for_admin() {
    let (app, cookie) = common::admin_app().await;
    let res = app
        .oneshot(authed_get("/rest/sources/repos", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body.as_array().map(|a| a.is_empty()).unwrap_or(false));
}

#[tokio::test]
async fn list_blocked_repos_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;
    let res = app
        .oneshot(authed_get("/rest/admin/sources/blocked-repos", &cookie))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn add_repo_with_non_https_url_returns_400() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/sources/repos",
            &cookie,
            serde_json::json!({"url": "http://insecure.example.com"}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_repo_returns_200_for_seeded_repo() {
    let state = test_state().await;
    let key = gen_key();
    let repo_id = seed_repo(&state, "https://example-repo.com", "My Repo", &key).await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_get(
            &format!("/rest/sources/repos/{repo_id}"),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["name"], "My Repo");
    assert_eq!(body["url"], "https://example-repo.com");
}

#[tokio::test]
async fn delete_repo_returns_no_content() {
    let state = test_state().await;
    let key = gen_key();
    let repo_id = seed_repo(&state, "https://to-delete.example.com", "Repo", &key).await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_delete(
            &format!("/rest/sources/repos/{repo_id}"),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn list_repo_extensions_returns_empty_for_seeded_repo() {
    let state = test_state().await;
    let key = gen_key();
    let repo_id = seed_repo(&state, "https://ext-list.example.com", "Ext Repo", &key).await;
    let (username, password) = create_admin(&state).await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_get(
            &format!("/rest/sources/repos/{repo_id}/extensions"),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body.as_array().map(|a| a.is_empty()).unwrap_or(false));
}

#[tokio::test]
async fn admin_block_and_delete_blocked_repo() {
    let (app, cookie) = common::admin_app().await;

    let block_res = app
        .clone()
        .oneshot(authed_post(
            "/rest/admin/sources/blocked-repos",
            &cookie,
            serde_json::json!({
                "url": "https://block-me.example.com",
                "reason": "integration test"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(block_res.status(), StatusCode::NO_CONTENT);

    let list_res = app
        .clone()
        .oneshot(authed_get("/rest/admin/sources/blocked-repos", &cookie))
        .await
        .unwrap();
    let list = body_json(list_res).await;
    let entries = list.as_array().unwrap();
    assert_eq!(entries.len(), 1);
    let id = entries[0]["id"].as_i64().unwrap();

    let del_res = app
        .oneshot(authed_delete(
            &format!("/rest/admin/sources/blocked-repos/{id}"),
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(del_res.status(), StatusCode::NO_CONTENT);
}
