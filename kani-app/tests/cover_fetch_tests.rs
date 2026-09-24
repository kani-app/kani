#![allow(clippy::unwrap_used)]

//! Cover download behavior across `save_to_library` and
//! `download_and_store_cover`). A cover fetch failure is swallowed (the library
//! entry is still saved and a retry scheduled), so the observable is that
//! `local_cover_path` stays NULL — the bad bytes are never stored.

mod common;
use common::{insert_source, test_service};

use std::collections::HashMap;
use std::sync::Arc;

use kani_app::service::AppService;
use kani_app::source::{SourceBackend, YamlSource};
use kani_shared::ast::Expr;
use kani_shared_test::origin::{Response, TestOrigin};
use kani_yaml::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedHnp,
    ValidatedTotalPages,
};
use kani_yaml::yaml::schema::ResponseType;

fn lit_field(name: &str, value: Expr) -> ValidatedField {
    ValidatedField {
        name: name.to_string(),
        source: FieldSource::Blueprint(value),
        optional: false,
    }
}

/// Register a source whose manga_details returns a fixed cover_url pointing at
/// `{origin}/cover`. No manga row is pre-inserted, so save_to_library inserts it
/// fresh and runs the cover download.
fn wire_cover_source(svc: &AppService, source_id: i64, origin: &TestOrigin) {
    let details = ValidatedEndpoint {
        route: "/manga/$manga_id$".into(),
        method: "GET".into(),
        headers: vec![],
        queries: vec![],
        filter_mapping: vec![],
        filter_format: None,
        response_type: ResponseType::Html,
        container: ".manga".into(),
        bindings: vec![],
        fields: vec![
            lit_field("id", Expr::lit("m1")),
            lit_field("title", Expr::lit("Covered")),
            lit_field("cover_url", Expr::lit(origin.url("/cover"))),
            lit_field("description", Expr::lit("desc")),
            lit_field("status", Expr::lit("ongoing")),
            lit_field("authors", Expr::list(vec![])),
            lit_field("artists", Expr::list(vec![])),
            lit_field("tags", Expr::list(vec![])),
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
        id: "cover-source".into(),
        name: "Cover Source".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        manga_details: Some(details),
        ..Default::default()
    };
    let source = YamlSource::new(
        Arc::new(ext),
        kani_core::http::SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true),
        Arc::new(kani_core::cache::InMemoryCache::new()),
        format!("cover-{source_id}:"),
        HashMap::new(),
        true,
    );
    svc.sources
        .insert(source_id, SourceBackend::Yaml(Box::new(source)));
}

const DETAILS_HTML: &str = r#"<html><body><div class="manga"><h1>x</h1></div></body></html>"#;

async fn cover_path(svc: &AppService, manga: kani_app::ids::MangaId) -> Option<String> {
    sqlx::query_scalar("SELECT local_cover_path FROM manga WHERE id = ?")
        .bind(manga.0)
        .fetch_one(&svc.db)
        .await
        .unwrap()
}

/// Reads the bytes the recorded cover path points at.
///
/// The row is set after the write, so a non-null path usually implies a file —
/// but "is stored" is a claim about the library, and only this checks it.
async fn cover_bytes(svc: &AppService, manga: kani_app::ids::MangaId) -> Vec<u8> {
    let relative = cover_path(svc, manga)
        .await
        .expect("a stored cover must record its path");
    let root = svc.settings.read().await.library_path.clone();
    tokio::fs::read(std::path::Path::new(&root).join(&relative))
        .await
        .unwrap_or_else(|e| panic!("cover recorded at {relative} is not on disk: {e}"))
}

#[tokio::test]
async fn a_cover_served_as_html_is_rejected() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    origin.set(
        "/cover",
        Response::html("<html>definitely not an image</html>"),
    );

    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "cover-source").await;
    wire_cover_source(&svc, source_id, &origin);

    let manga = svc.save_to_library(source_id, "m1", false).await.unwrap();

    assert!(
        cover_path(&svc, manga).await.is_none(),
        "an HTML body must be rejected by the cover Content-Type gate, not stored"
    );
}

#[tokio::test]
async fn a_cover_served_as_octet_stream_is_still_stored() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    origin.set(
        "/cover",
        Response::ok(kani_shared_test::origin::jpeg_page(64, 96, false, 80))
            .header("Content-Type", "application/octet-stream"),
    );

    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "cover-source").await;
    wire_cover_source(&svc, source_id, &origin);

    let manga = svc.save_to_library(source_id, "m1", false).await.unwrap();

    let bytes = cover_bytes(&svc, manga).await;
    assert!(
        bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "a real JPEG must be judged on its bytes, not on a generic Content-Type"
    );
}

#[tokio::test]
async fn a_page_labelled_as_an_image_is_still_rejected() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    origin.set(
        "/cover",
        Response::ok(b"<html><body>Just a moment...</body></html>".to_vec())
            .header("Content-Type", "image/png"),
    );

    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "cover-source").await;
    wire_cover_source(&svc, source_id, &origin);

    let manga = svc.save_to_library(source_id, "m1", false).await.unwrap();

    assert!(
        cover_path(&svc, manga).await.is_none(),
        "an HTML challenge page claiming to be a PNG must not be stored as a cover"
    );
}

#[tokio::test]
async fn a_cover_larger_than_the_cap_is_rejected() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    origin.set("/cover", Response::image(vec![0u8; 11 * 1024 * 1024]));

    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "cover-source").await;
    wire_cover_source(&svc, source_id, &origin);

    let manga = svc.save_to_library(source_id, "m1", false).await.unwrap();

    assert!(
        cover_path(&svc, manga).await.is_none(),
        "a cover exceeding the 10 MB cap must be rejected, not stored truncated"
    );
}

#[tokio::test]
async fn a_failed_cover_is_retried_by_the_sweep() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    origin.set("/cover", Response::status(503));

    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "cover-source").await;
    wire_cover_source(&svc, source_id, &origin);

    let manga = svc.save_to_library(source_id, "m1", false).await.unwrap();
    assert!(
        cover_path(&svc, manga).await.is_none(),
        "the 503 leaves no cover stored"
    );
    assert!(
        svc.cover_retry_queue.lock().await.contains(&manga),
        "a failed cover must be queued for the retry sweep, not forgotten"
    );

    origin.set(
        "/cover",
        Response::image(kani_shared_test::origin::jpeg_page(64, 96, false, 80)),
    );
    svc.retry_single_cover(manga).await.unwrap();

    assert!(
        cover_path(&svc, manga).await.is_some(),
        "the retry stores the cover once the origin is healthy again"
    );
}

#[tokio::test]
async fn a_valid_image_cover_is_stored() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    origin.set(
        "/cover",
        Response::image(kani_shared_test::origin::jpeg_page(64, 96, false, 80)),
    );

    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "cover-source").await;
    wire_cover_source(&svc, source_id, &origin);

    let manga = svc.save_to_library(source_id, "m1", false).await.unwrap();

    let bytes = cover_bytes(&svc, manga).await;
    assert!(
        bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "the stored cover must be the JPEG that was fetched, got {} bytes starting {:?}",
        bytes.len(),
        &bytes[..bytes.len().min(4)]
    );
}
