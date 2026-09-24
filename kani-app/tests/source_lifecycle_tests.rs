#![allow(clippy::unwrap_used)]

//! Database and registry source lifecycle behavior, including disabled-source
//! classification and removal of the backend and installed artifact.

mod common;
use common::{insert_source, test_service};

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use kani_app::error::ServiceError;
use kani_app::source::{SourceBackend, YamlSource};
use kani_core::http::{SmartClient, Timings};
use kani_shared::ast::Expr;
use kani_shared_test::insert_user;
use kani_shared_test::origin::{Body, Response, TestOrigin};
use kani_yaml::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedHnp,
    ValidatedTotalPages,
};
use kani_yaml::yaml::schema::ResponseType;

fn yaml_backend(name: &str) -> SourceBackend {
    let ext = ValidatedExtension {
        id: name.into(),
        name: name.into(),
        version: "1.0.0".into(),
        base_url: "http://127.0.0.1:1".into(),
        language: "en".into(),
        unrestricted_http: true,
        ..Default::default()
    };
    SourceBackend::Yaml(Box::new(YamlSource::new(
        Arc::new(ext),
        kani_core::http::SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true),
        Arc::new(kani_core::cache::InMemoryCache::new()),
        "test:".into(),
        HashMap::new(),
        true,
    )))
}

#[tokio::test]
async fn a_disabled_source_reports_disabled_not_not_found() {
    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "disabled-src").await;
    sqlx::query("UPDATE sources SET enabled = 0 WHERE id = ?")
        .bind(source_id)
        .execute(&svc.db)
        .await
        .unwrap();

    match svc.get_metadata(source_id).await {
        Err(ServiceError::SourceDisabled(id)) => assert_eq!(id, source_id),
        other => panic!("expected SourceDisabled, got {other:?}"),
    }

    match svc.get_metadata(999_999).await {
        Err(ServiceError::NotFound(_)) => {}
        other => panic!("expected NotFound for an unknown source, got {other:?}"),
    }
}

#[tokio::test]
async fn uninstall_removes_the_backend_and_the_artifact() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "admin").await;
    let name = "uninstall-me";
    let source_id = insert_source(&svc.db, name).await;
    svc.sources.insert(source_id, yaml_backend(name));

    let storage = svc.settings.read().await.wasm_storage_path.clone();
    let artifact = storage.join(format!("{name}.wasm"));
    tokio::fs::write(&artifact, b"fake wasm bytes")
        .await
        .unwrap();
    assert!(artifact.exists(), "precondition: the artifact was planted");
    assert!(
        svc.sources.contains_key(source_id),
        "precondition: backend registered"
    );

    svc.delete_source(source_id, user).await.unwrap();

    assert!(
        !svc.sources.contains_key(source_id),
        "the backend was removed from the registry"
    );
    assert!(!artifact.exists(), "the .wasm artifact was deleted");
    let deleted_at: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM sources WHERE id = ?")
            .bind(source_id)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert!(deleted_at.is_some(), "the row was soft-deleted");
}

#[tokio::test]
async fn a_deleted_source_does_not_hang_an_in_flight_request() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::status(200).body(Body::Stall));
    let svc = Arc::new(test_service().await);
    let user = insert_user(&svc.db, "admin").await;
    let source_id = insert_source(&svc.db, "stall-src").await;

    let ext = ValidatedExtension {
        id: "stall-src".into(),
        name: "stall-src".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        manga_details: Some(details_endpoint()),
        ..Default::default()
    };
    let client = SmartClient::new(None)
        .unwrap()
        .with_allow_loopback_egress(true)
        .with_timings(Timings {
            request_timeout: Duration::from_millis(200),
            retry_base_delay: Duration::from_millis(1),
            retry_jitter: Duration::ZERO,
            ..Timings::default()
        });
    svc.sources.insert(
        source_id,
        SourceBackend::Yaml(Box::new(YamlSource::new(
            Arc::new(ext),
            client,
            Arc::new(kani_core::cache::InMemoryCache::new()),
            "test:".into(),
            HashMap::new(),
            true,
        ))),
    );

    let svc2 = svc.clone();
    let call = tokio::spawn(async move { svc2.get_manga_details(source_id, "m1").await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    svc.delete_source(source_id, user).await.unwrap();

    let res = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("the in-flight request must not hang")
        .unwrap();
    assert!(
        res.is_err(),
        "the stalled request is bounded by its timeout, got {res:?}"
    );
}

#[tokio::test]
async fn concurrent_installs_of_the_same_extension_serialise() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/ext1.wasm",
        Response::status(200)
            .header("Content-Type", "application/wasm")
            .body(Body::Bytes(b"artifact bytes".to_vec()))
            .delay(Duration::from_millis(150)),
    );
    let svc = Arc::new(test_service().await);
    let index = serde_json::json!({
        "name": "R", "maintainer_key": "KEY",
        "extensions": [{
            "id": "ext1", "name": "Ext", "version": "1.0.0", "format": "wasm",
            "sha256": "00".repeat(32), "signature": "x", "author_key": "x", "url": "/ext1.wasm",
        }],
    })
    .to_string();
    let repo_id: i64 = sqlx::query_scalar(
        "INSERT INTO repo_trust (url, name, maintainer_key, index_cache) \
         VALUES (?, 'R', 'KEY', ?) RETURNING id",
    )
    .bind(origin.base())
    .bind(index)
    .fetch_one(&svc.db)
    .await
    .unwrap();

    let (a, b) = {
        let s1 = svc.clone();
        let s2 = svc.clone();
        let h1 =
            tokio::spawn(async move { s1.install_source_from_repo(repo_id, "ext1", None).await });
        let h2 =
            tokio::spawn(async move { s2.install_source_from_repo(repo_id, "ext1", None).await });
        (h1.await.unwrap(), h2.await.unwrap())
    };

    for res in [a, b] {
        match res {
            Err(ServiceError::Validation(msg)) => {
                assert!(msg.contains("Integrity"), "unexpected error: {msg}")
            }
            other => panic!("expected an integrity failure, got {other:?}"),
        }
    }
}

fn details_endpoint() -> ValidatedEndpoint {
    ValidatedEndpoint {
        route: "/manga/$manga_id$".into(),
        method: "GET".into(),
        headers: vec![],
        queries: vec![],
        filter_mapping: vec![],
        filter_format: None,
        response_type: ResponseType::Html,
        container: ".manga".into(),
        bindings: vec![],
        fields: vec![ValidatedField {
            name: "id".into(),
            source: FieldSource::Blueprint(Expr::lit("m1")),
            optional: false,
        }],
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
    }
}
