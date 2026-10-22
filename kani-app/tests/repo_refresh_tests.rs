#![allow(clippy::unwrap_used)]

//! Repository index refresh under a conditional-GET 304. `refresh_repo` uses
//! `safe_get_conditional`; a 304 must reuse the cached index and must fail when
//! no cached index exists.

mod common;
use common::test_service;
use kani_app::service::AppService;
use kani_shared_test::origin::{Response, TestOrigin};

const INDEX_JSON: &str = r#"{"name":"Test Repo","maintainer_key":"KEY","extensions":[]}"#;

async fn seed_repo(svc: &AppService, url: &str, index_cache: Option<&str>) -> i64 {
    if let Some(text) = index_cache {
        return common::seed_signed_repo(&svc.db, url, serde_json::from_str(text).unwrap()).await;
    }
    sqlx::query_scalar(
        "INSERT INTO repo_trust (url, name, maintainer_key, index_cache) \
         VALUES (?, 'Test Repo', 'KEY', NULL) RETURNING id",
    )
    .bind(url)
    .fetch_one(&svc.db)
    .await
    .unwrap()
}

async fn cached(svc: &AppService, id: i64) -> Option<String> {
    sqlx::query_scalar("SELECT index_cache FROM repo_trust WHERE id = ?")
        .bind(id)
        .fetch_one(&svc.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_304_yields_the_cached_index() {
    let origin = TestOrigin::start().await;
    origin.set("/index.json", Response::status(304));
    let svc = test_service().await;
    let id = seed_repo(&svc, &origin.base(), Some(INDEX_JSON)).await;
    let seeded = cached(&svc, id).await;

    svc.refresh_repo(id, None).await.unwrap();

    let cache: Option<String> =
        sqlx::query_scalar("SELECT index_cache FROM repo_trust WHERE id = ?")
            .bind(id)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert_eq!(
        cache.as_deref(),
        seeded.as_deref(),
        "the 304 reused the cached index unchanged"
    );
    let refreshed: Option<String> =
        sqlx::query_scalar("SELECT last_refreshed_at FROM repo_trust WHERE id = ?")
            .bind(id)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert!(refreshed.is_some(), "the refresh timestamp advanced");
}

#[tokio::test]
async fn a_304_with_no_cached_index_is_an_error() {
    let origin = TestOrigin::start().await;
    origin.set("/index.json", Response::status(304));
    let svc = test_service().await;
    let id = seed_repo(&svc, &origin.base(), None).await;

    let res = svc.refresh_repo(id, None).await;

    assert!(
        res.is_err(),
        "a 304 with nothing cached must error rather than yield an empty index"
    );
}

use base64::Engine as _;

fn valid_pubkey_b64() -> String {
    use ed25519_dalek::SigningKey;
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    base64::engine::general_purpose::STANDARD.encode(sk.verifying_key().to_bytes())
}

#[tokio::test]
async fn an_index_with_no_signature_is_refused() {
    let key = valid_pubkey_b64();
    let origin = TestOrigin::start().await;
    let index = format!(r#"{{"name":"Test Repo","maintainer_key":"{key}","extensions":[]}}"#);
    origin.set("/index.json", Response::json(&index));
    origin.set("/index.json.sig", Response::status(404));

    let svc = test_service().await;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO repo_trust (url, name, maintainer_key, index_cache) \
         VALUES (?, 'Test Repo', ?, NULL) RETURNING id",
    )
    .bind(origin.base())
    .bind(&key)
    .fetch_one(&svc.db)
    .await
    .unwrap();

    let res = svc.refresh_repo(id, None).await;
    assert!(
        res.is_err(),
        "an index served without its signature is refused, got {res:?}"
    );
}

#[tokio::test]
async fn a_repo_that_starts_failing_does_not_lose_its_cached_index() {
    let origin = TestOrigin::start().await;
    origin.set("/index.json", Response::status(500));
    let svc = test_service().await;
    let id = seed_repo(&svc, &origin.base(), Some(INDEX_JSON)).await;
    let seeded = cached(&svc, id).await;

    let res = svc.refresh_repo(id, None).await;

    assert!(res.is_err(), "the failing refresh surfaces an error");
    let cache: Option<String> =
        sqlx::query_scalar("SELECT index_cache FROM repo_trust WHERE id = ?")
            .bind(id)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert_eq!(
        cache.as_deref(),
        seeded.as_deref(),
        "the cached index survived the failed refresh"
    );
}

#[tokio::test]
async fn a_repo_trusted_before_signatures_were_stored_fetches_them_when_opened() {
    use kani_app::source::signing::{pubkey_b64, sign_artifact, signature_b64};
    let key = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
    let pk = pubkey_b64(&key);
    let index = format!(
        r#"{{"name":"Test Repo","maintainer_key":"{pk}","extensions":[{{"id":"ext1","name":"Ext One","version":"1.0.0","format":"wasm","sha256":"00","signature":"x","author_key":"x","url":"/ext1.wasm"}}]}}"#
    );
    let origin = TestOrigin::start().await;
    origin.set("/index.json", Response::json(&index));
    origin.set(
        "/index.json.sig",
        Response::status(200).body(kani_shared_test::origin::Body::Bytes(
            signature_b64(&sign_artifact(index.as_bytes(), &key)).into_bytes(),
        )),
    );

    let svc = test_service().await;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO repo_trust (url, name, maintainer_key, index_cache) \
         VALUES (?, 'Test Repo', ?, ?) RETURNING id",
    )
    .bind(origin.base())
    .bind(&pk)
    .bind(format!(
        r#"{{"name":"Test Repo","maintainer_key":"{pk}","extensions":[]}}"#
    ))
    .fetch_one(&svc.db)
    .await
    .unwrap();

    let extensions = svc.list_repo_extensions(id).await.unwrap();
    assert_eq!(
        extensions.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["ext1"],
        "the signed index was fetched and verified in place of the unsigned cache"
    );
    let stored: Option<String> =
        sqlx::query_scalar("SELECT index_sig FROM repo_trust WHERE id = ?")
            .bind(id)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert!(stored.is_some(), "the signature is kept for later loads");
}
