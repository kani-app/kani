#![allow(clippy::unwrap_used)]

//! Repository artifact integrity during installation.
//!
//! Size and digest checks precede signature verification, allowing those rejection
//! paths to use a directly seeded trusted index without signing fixtures.

mod common;
use common::test_service;
use kani_app::error::ServiceError;
use kani_app::service::AppService;
use kani_shared_test::origin::{Body, Response, TestOrigin};

/// Seed a repo whose cached index advertises one extension `ext1` at `artifact_url`
/// with the given SHA-256. The key and signature are placeholders for checks that
/// reject the artifact before signature verification.
async fn seed_repo_with_entry(
    svc: &AppService,
    repo_url: &str,
    artifact_url: &str,
    sha256: &str,
) -> i64 {
    let index = serde_json::json!({
        "name": "Test Repo",
        "maintainer_key": "KEY",
        "extensions": [{
            "id": "ext1",
            "name": "Ext One",
            "version": "1.0.0",
            "format": "wasm",
            "sha256": sha256,
            "signature": "x",
            "author_key": "x",
            "url": artifact_url,
        }],
    });
    common::seed_signed_repo(&svc.db, repo_url, index).await
}

#[tokio::test]
async fn an_artifact_larger_than_the_cap_is_rejected() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/ext1.wasm",
        Response::status(200)
            .header("Content-Type", "application/wasm")
            .body(Body::Truncated {
                bytes: vec![0u8; 16],
                announced: 50 * 1024 * 1024,
                sent: 16,
            }),
    );
    let svc = test_service().await;
    let repo_id = seed_repo_with_entry(&svc, &origin.base(), "/ext1.wasm", "deadbeef").await;

    let res = svc.install_source_from_repo(repo_id, "ext1", None).await;

    assert!(
        matches!(res, Err(ServiceError::Internal(_))),
        "an oversized artifact is rejected, got {res:?}"
    );
}

#[tokio::test]
async fn an_artifact_whose_hash_does_not_match_is_rejected() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/ext1.wasm",
        Response::status(200)
            .header("Content-Type", "application/wasm")
            .body(Body::Bytes(b"the real artifact bytes".to_vec())),
    );
    let svc = test_service().await;
    let repo_id = seed_repo_with_entry(&svc, &origin.base(), "/ext1.wasm", &"00".repeat(32)).await;

    let res = svc.install_source_from_repo(repo_id, "ext1", None).await;

    match res {
        Err(ServiceError::Validation(msg)) => assert!(
            msg.contains("Integrity"),
            "hash mismatch surfaces as an integrity failure, got: {msg}"
        ),
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn an_index_entry_url_cannot_point_at_another_host() {
    let origin = TestOrigin::start().await;
    let svc = test_service().await;
    let repo_id = seed_repo_with_entry(
        &svc,
        &origin.base(),
        "http://192.0.2.1/evil.wasm",
        "deadbeef",
    )
    .await;

    let res = svc.install_source_from_repo(repo_id, "ext1", None).await;

    match res {
        Err(ServiceError::Validation(msg)) => assert!(
            msg.contains("does not match the repository host"),
            "cross-host artifact is refused for the right reason, got: {msg}"
        ),
        other => panic!("expected a host-mismatch validation error, got {other:?}"),
    }
}
