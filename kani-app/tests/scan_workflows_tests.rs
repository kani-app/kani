#![allow(clippy::unwrap_used)]

//! Extension-driven scan workflows. A rescan that finds *new* chapters is
//! only reachable when the listing grows between scans, which a static fixture
//! can't express. `TestOrigin.script()` serves response N then N+1, so the second
//! scan sees a grown listing.

mod common;
use common::{insert_chapter, insert_manga, insert_source, test_service};

use std::collections::HashMap;
use std::sync::Arc;

use kani_app::events::AppEvent;
use kani_app::ids::MangaId;
use kani_app::service::AppService;
use kani_app::source::{SourceBackend, YamlSource};
use kani_shared::ast::Expr;
use kani_shared_test::origin::{Response, TestOrigin};
use kani_yaml::yaml::model::{
    FieldSource, ValidatedEndpoint, ValidatedExtension, ValidatedField, ValidatedHnp,
    ValidatedTotalPages,
};
use kani_yaml::yaml::schema::ResponseType;

const LISTING_2: &str = r#"<html><body>
    <div class="ch" data-id="ch-1"><span class="title">Chapter 1</span></div>
    <div class="ch" data-id="ch-2"><span class="title">Chapter 2</span></div>
</body></html>"#;

const LISTING_3: &str = r#"<html><body>
    <div class="ch" data-id="ch-1"><span class="title">Chapter 1</span></div>
    <div class="ch" data-id="ch-2"><span class="title">Chapter 2</span></div>
    <div class="ch" data-id="ch-3"><span class="title">Chapter 3</span></div>
</body></html>"#;

fn field(name: &str, expr: Expr) -> ValidatedField {
    ValidatedField {
        name: name.to_string(),
        source: FieldSource::Blueprint(expr),
        optional: false,
    }
}

fn chapter_list_endpoint() -> ValidatedEndpoint {
    ValidatedEndpoint {
        route: "/manga/$manga_id$/chapters".into(),
        method: "GET".into(),
        headers: vec![],
        queries: vec![],
        filter_mapping: vec![],
        filter_format: None,
        response_type: ResponseType::Html,
        container: ".ch".into(),
        bindings: vec![],
        fields: vec![
            field("id", Expr::self_ref().attr("data-id")),
            field("title", Expr::self_ref().first(".title").text()),
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
    }
}

/// Like [`chapter_list_endpoint`] but `has_next_page` is statically true, so the
/// scan loop always advances to the next page until a fetch/extraction fails.
fn chapter_list_endpoint_always_paginated() -> ValidatedEndpoint {
    ValidatedEndpoint {
        has_next_page: ValidatedHnp::Static(true),
        ..chapter_list_endpoint()
    }
}

/// Register a YAML source whose chapter_list always claims another page.
async fn wire_source_always_paginated(svc: &AppService, origin: &TestOrigin) -> MangaId {
    let source_id = insert_source(&svc.db, "fixture-source").await;
    let manga_id = insert_manga(&svc.db, source_id, "m1", "Fixture Manga").await;
    let ext = ValidatedExtension {
        id: "fixture-source".into(),
        name: "Fixture Source".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        chapter_list: Some(chapter_list_endpoint_always_paginated()),
        ..Default::default()
    };
    svc.sources.insert(
        source_id,
        SourceBackend::Yaml(Box::new(YamlSource::new(
            Arc::new(ext),
            kani_core::http::SmartClient::new(None)
                .unwrap()
                .with_allow_loopback_egress(true),
            Arc::new(kani_core::cache::InMemoryCache::new()),
            "test:".into(),
            HashMap::new(),
            true,
        ))),
    );
    manga_id
}

/// Register a YAML source (with only a chapter_list endpoint) pointed at `origin`.
async fn wire_source(svc: &AppService, origin: &TestOrigin) -> MangaId {
    let source_id = insert_source(&svc.db, "fixture-source").await;
    let manga_id = insert_manga(&svc.db, source_id, "m1", "Fixture Manga").await;
    let ext = ValidatedExtension {
        id: "fixture-source".into(),
        name: "Fixture Source".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        chapter_list: Some(chapter_list_endpoint()),
        ..Default::default()
    };
    svc.sources.insert(
        source_id,
        SourceBackend::Yaml(Box::new(YamlSource::new(
            Arc::new(ext),
            kani_core::http::SmartClient::new(None)
                .unwrap()
                .with_allow_loopback_egress(true),
            Arc::new(kani_core::cache::InMemoryCache::new()),
            "test:".into(),
            HashMap::new(),
            true,
        ))),
    );
    manga_id
}

/// Drain the refresh channel and count `NewChapters` events (with their counts).
fn drain_new_chapter_counts(rx: &mut tokio::sync::broadcast::Receiver<AppEvent>) -> Vec<usize> {
    let mut counts = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let AppEvent::NewChapters { count, .. } = ev {
            counts.push(count);
        }
    }
    counts
}

#[tokio::test]
async fn a_grown_listing_emits_new_chapters_once_with_the_right_count() {
    let origin = TestOrigin::start().await;
    origin.script(
        "/manga/m1/chapters",
        vec![Response::html(LISTING_2), Response::html(LISTING_3)],
    );
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;

    let first = svc.scan_for_new_chapters(manga_id).await.unwrap();
    assert_eq!(first.len(), 2, "first scan discovers both chapters");

    let mut rx = svc.subscribe_refresh();
    let second = svc.scan_for_new_chapters(manga_id).await.unwrap();
    assert_eq!(second.len(), 1, "only the newly-listed chapter is new");

    assert_eq!(
        drain_new_chapter_counts(&mut rx),
        vec![1],
        "exactly one NewChapters event, count 1 — not 3"
    );
}

#[tokio::test]
async fn a_failing_webhook_does_not_block_the_triggering_action() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1/chapters", Response::html(LISTING_2));
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;

    svc.webhook_service.allow_private_egress_for_test();
    let hook = TestOrigin::start().await;
    hook.set(
        "/hook",
        Response::status(200).body(kani_shared_test::origin::Body::Stall),
    );
    sqlx::query("INSERT INTO webhooks (url, events, enabled) VALUES (?, '[\"*\"]', 1)")
        .bind(hook.url("/hook"))
        .execute(&svc.db)
        .await
        .unwrap();
    svc.spawn_webhook_listener();

    let started = std::time::Instant::now();
    let found = svc.scan_for_new_chapters(manga_id).await.unwrap();
    let elapsed = started.elapsed();

    assert_eq!(found.len(), 2, "the scan still did its real work");
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "the scan must not wait on webhook delivery, took {elapsed:?}"
    );
}

#[tokio::test]
async fn a_duplicate_chapter_id_in_one_listing_is_deduplicated() {
    let origin = TestOrigin::start().await;
    origin.set(
        "/manga/m1/chapters",
        Response::html(
            r#"<html><body>
            <div class="ch" data-id="ch-1"><span class="title">Chapter 1</span></div>
            <div class="ch" data-id="ch-1"><span class="title">Chapter 1 again</span></div>
            <div class="ch" data-id="ch-2"><span class="title">Chapter 2</span></div>
            </body></html>"#,
        ),
    );
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;

    svc.fetch_and_store_chapters_silent(manga_id).await.unwrap();

    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chapters WHERE manga_id = ? AND source_chapter_id = 'ch-1'",
    )
    .bind(manga_id.0)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(rows, 1, "a repeated chapter id must yield exactly one row");

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chapters WHERE manga_id = ?")
        .bind(manga_id.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(total, 2, "the distinct chapters both survive");
}

#[tokio::test]
async fn a_chapter_list_page_failure_is_reported_not_silently_completed() {
    let origin = TestOrigin::start().await;
    origin.script(
        "/manga/m1/chapters",
        vec![
            Response::html(LISTING_2),
            Response::html(
                r#"<html><body><div class="ch"><span class="title">broken</span></div></body></html>"#,
            ),
        ],
    );
    let svc = test_service().await;
    let manga_id = wire_source_always_paginated(&svc, &origin).await;

    let res = svc.fetch_and_store_chapters_silent(manga_id).await;
    assert!(
        res.is_err(),
        "a failure on page 2 must be reported, not silently completed with only page 1: {res:?}"
    );
    assert_eq!(
        origin.hits("/manga/m1/chapters"),
        2,
        "the scan advanced to page 2 (where it failed), proving page 1 was not treated as complete"
    );
}

#[tokio::test]
async fn scan_stops_after_the_configured_run_of_known_pages() {
    let origin = TestOrigin::start().await;
    origin.script("/manga/m1/chapters", vec![Response::html(LISTING_2)]);
    let svc = test_service().await;
    let manga_id = wire_source_always_paginated(&svc, &origin).await;
    insert_chapter(&svc.db, manga_id, "ch-1", 1.0).await;
    insert_chapter(&svc.db, manga_id, "ch-2", 2.0).await;
    svc.settings.write().await.scan_barren_page_tolerance = 2;

    svc.fetch_and_store_chapters_silent(manga_id).await.unwrap();

    assert_eq!(origin.hits("/manga/m1/chapters"), 2);
}

#[tokio::test]
async fn a_slow_source_does_not_block_other_writers_during_a_scan() {
    let origin = TestOrigin::start().await;
    origin.script(
        "/manga/m1/chapters",
        vec![Response::html(LISTING_2).delay(std::time::Duration::from_millis(1500))],
    );
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("../migrations").run(&pool).await.unwrap();
    let svc = AppService::new_for_test(pool).await;
    let manga_id = wire_source(&svc, &origin).await;

    let scanner = svc.clone();
    let scan = tokio::spawn(async move { scanner.fetch_and_store_chapters_silent(manga_id).await });

    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let started = std::time::Instant::now();
    sqlx::query("UPDATE manga SET name = 'Renamed While Scanning' WHERE id = ?")
        .bind(manga_id)
        .execute(&svc.db)
        .await
        .expect("an unrelated write must not wait on the scan's network I/O");
    let waited = started.elapsed();

    assert!(
        waited < std::time::Duration::from_millis(500),
        "an unrelated write waited {waited:?} on a scan that was still fetching"
    );

    scan.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_run_of_known_pages_shorter_than_the_tolerance_does_not_end_the_scan() {
    let origin = TestOrigin::start().await;
    origin.script(
        "/manga/m1/chapters",
        vec![
            Response::html(LISTING_2),
            Response::html(LISTING_2),
            Response::html(LISTING_3),
        ],
    );
    let svc = test_service().await;
    let manga_id = wire_source_always_paginated(&svc, &origin).await;
    insert_chapter(&svc.db, manga_id, "ch-1", 1.0).await;
    insert_chapter(&svc.db, manga_id, "ch-2", 2.0).await;
    svc.settings.write().await.scan_barren_page_tolerance = 3;

    svc.fetch_and_store_chapters_silent(manga_id).await.unwrap();

    let found: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chapters WHERE manga_id = ? AND source_chapter_id = 'ch-3'",
    )
    .bind(manga_id)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(
        found, 1,
        "the chapter beyond two known pages was never reached"
    );
}

#[tokio::test]
async fn a_rescan_with_no_change_emits_nothing() {
    let origin = TestOrigin::start().await;
    origin.script(
        "/manga/m1/chapters",
        vec![Response::html(LISTING_2), Response::html(LISTING_2)],
    );
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;

    svc.scan_for_new_chapters(manga_id).await.unwrap();

    let mut rx = svc.subscribe_refresh();
    let second = svc.scan_for_new_chapters(manga_id).await.unwrap();
    assert!(
        second.is_empty(),
        "an unchanged listing yields no new chapters"
    );
    assert!(
        drain_new_chapter_counts(&mut rx).is_empty(),
        "no NewChapters event fires when nothing is new"
    );
}

#[tokio::test]
async fn a_chapter_that_disappears_from_the_listing_is_not_deleted() {
    let origin = TestOrigin::start().await;
    origin.script(
        "/manga/m1/chapters",
        vec![Response::html(LISTING_3), Response::html(LISTING_2)],
    );
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;

    assert_eq!(svc.scan_for_new_chapters(manga_id).await.unwrap().len(), 3);
    assert!(
        svc.scan_for_new_chapters(manga_id)
            .await
            .unwrap()
            .is_empty()
    );

    let ch3_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chapters WHERE manga_id = ? AND source_chapter_id = 'ch-3'",
    )
    .bind(manga_id.0)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(
        ch3_exists, 1,
        "a chapter that vanished from the source is not deleted"
    );
}

#[tokio::test]
async fn a_relisted_chapter_keeps_its_row_identity() {
    let origin = TestOrigin::start().await;
    origin.script(
        "/manga/m1/chapters",
        vec![Response::html(LISTING_2), Response::html(LISTING_2)],
    );
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;

    svc.scan_for_new_chapters(manga_id).await.unwrap();
    let id_before: i64 = sqlx::query_scalar(
        "SELECT id FROM chapters WHERE manga_id = ? AND source_chapter_id = 'ch-1'",
    )
    .bind(manga_id.0)
    .fetch_one(&svc.db)
    .await
    .unwrap();

    svc.scan_for_new_chapters(manga_id).await.unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chapters WHERE manga_id = ?")
        .bind(manga_id.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let id_after: i64 = sqlx::query_scalar(
        "SELECT id FROM chapters WHERE manga_id = ? AND source_chapter_id = 'ch-1'",
    )
    .bind(manga_id.0)
    .fetch_one(&svc.db)
    .await
    .unwrap();

    assert_eq!(rows, 2, "no duplicate rows from a re-list");
    assert_eq!(
        id_before, id_after,
        "the chapter keeps its row id across scans"
    );
}

#[tokio::test]
async fn a_new_chapter_fires_the_configured_webhook() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1/chapters", Response::html(LISTING_2));
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;

    sqlx::query("INSERT INTO webhooks (url, events, enabled) VALUES ('https://hook.invalid/x', '[\"*\"]', 1)")
        .execute(&svc.db)
        .await
        .unwrap();

    svc.spawn_webhook_listener();
    svc.scan_for_new_chapters(manga_id).await.unwrap();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let payload = loop {
        let row: Option<String> = sqlx::query_scalar(
            "SELECT payload FROM webhook_deliveries WHERE event_type = 'chapter.new' ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(&svc.db)
        .await
        .unwrap();
        if let Some(p) = row {
            break p;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("no chapter.new webhook delivery was recorded");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };

    assert!(
        payload.contains("Fixture Manga"),
        "payload names the manga: {payload}"
    );
    assert!(
        payload.contains("Chapter 1"),
        "payload names a chapter: {payload}"
    );
}

/// Count chapter-download jobs submitted (rows persist through terminal status).
async fn download_job_count(svc: &AppService) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE job_type = 'chapter_download'")
        .fetch_one(&svc.db)
        .await
        .unwrap()
}

async fn chapter_ids_of(svc: &AppService, manga_id: MangaId) -> Vec<i64> {
    sqlx::query_scalar("SELECT id FROM chapters WHERE manga_id = ? ORDER BY id")
        .bind(manga_id.0)
        .fetch_all(&svc.db)
        .await
        .unwrap()
}

/// A chapter-download job whose description names exactly this chapter. Matches
/// on `description` (not `params_json`, which the manager nulls on completion),
/// so it is race-free against the job actually running.
async fn has_download_job_for(svc: &AppService, chapter_id: i64) -> bool {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE job_type = 'chapter_download' AND description LIKE ?",
    )
    .bind(format!("Download chapter {chapter_id} (%"))
    .fetch_one(&svc.db)
    .await
    .unwrap();
    n >= 1
}

async fn enable_global_auto_scan(svc: &AppService) {
    svc.settings.write().await.auto_scan = true;
}

#[tokio::test]
async fn a_new_chapter_is_enqueued_when_auto_download_is_on() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1/chapters", Response::html(LISTING_2));
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;
    enable_global_auto_scan(&svc).await;
    sqlx::query("UPDATE manga SET auto_download = 1 WHERE id = ?")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.run_auto_scan_once().await;

    let chapters = chapter_ids_of(&svc, manga_id).await;
    assert_eq!(chapters.len(), 2, "both chapters were scanned in");
    for ch in chapters {
        assert!(
            has_download_job_for(&svc, ch).await,
            "chapter {ch} has its own download job enqueued"
        );
    }
}

#[tokio::test]
async fn a_new_chapter_is_not_enqueued_when_auto_download_is_off() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1/chapters", Response::html(LISTING_2));
    let svc = test_service().await;
    let _manga_id = wire_source(&svc, &origin).await;
    enable_global_auto_scan(&svc).await;

    svc.run_auto_scan_once().await;

    let chapters: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chapters")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(chapters, 2, "chapters are still scanned in");
    assert_eq!(
        download_job_count(&svc).await,
        0,
        "no download job without auto_download"
    );
}

#[tokio::test]
async fn category_membership_enables_auto_download() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1/chapters", Response::html(LISTING_2));
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;
    enable_global_auto_scan(&svc).await;

    let cat_id: i64 =
        sqlx::query_scalar("INSERT INTO categories (name) VALUES ('Follows') RETURNING id")
            .fetch_one(&svc.db)
            .await
            .unwrap();
    sqlx::query("INSERT INTO manga_categories (manga_id, category_id) VALUES (?, ?)")
        .bind(manga_id.0)
        .bind(cat_id)
        .execute(&svc.db)
        .await
        .unwrap();
    svc.settings.write().await.auto_download_category_ids = format!("[{cat_id}]");

    svc.run_auto_scan_once().await;

    let chapters = chapter_ids_of(&svc, manga_id).await;
    assert!(!chapters.is_empty(), "chapters were scanned in");
    for ch in chapters {
        assert!(
            has_download_job_for(&svc, ch).await,
            "category membership enqueues chapter {ch} even with the manga flag off"
        );
    }
}

#[tokio::test]
async fn auto_download_respects_download_rules() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1/chapters", Response::html(LISTING_2));
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;
    enable_global_auto_scan(&svc).await;
    sqlx::query("UPDATE manga SET auto_download = 1 WHERE id = ?")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO download_rules (manga_id, rule_type, value) VALUES (?, 'language_include', 'zz')")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.run_auto_scan_once().await;

    assert_eq!(
        download_job_count(&svc).await,
        0,
        "chapters filtered out by rules are not enqueued"
    );
    assert_eq!(
        manga_scalar::<i64>(&svc, manga_id, "suppressed_chapter_count").await,
        2,
        "the suppressed-count signal records the filtered chapters"
    );
}

#[tokio::test]
async fn a_passing_scan_clears_the_suppressed_signal() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1/chapters", Response::html(LISTING_2));
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;
    enable_global_auto_scan(&svc).await;
    sqlx::query("UPDATE manga SET auto_download = 1, suppressed_chapter_count = 5 WHERE id = ?")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.run_auto_scan_once().await;

    assert_eq!(
        manga_scalar::<i64>(&svc, manga_id, "suppressed_chapter_count").await,
        0,
        "chapters flowing again clears the signal"
    );
}

#[tokio::test]
async fn dismissing_suppressed_chapters_zeroes_the_signal() {
    let origin = TestOrigin::start().await;
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;
    sqlx::query("UPDATE manga SET suppressed_chapter_count = 3 WHERE id = ?")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.dismiss_suppressed_chapters(manga_id).await.unwrap();

    assert_eq!(
        manga_scalar::<i64>(&svc, manga_id, "suppressed_chapter_count").await,
        0
    );
}

#[tokio::test]
async fn auto_download_skips_a_manga_with_auto_scan_off() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1/chapters", Response::html(LISTING_2));
    let svc = test_service().await;
    let manga_id = wire_source(&svc, &origin).await;
    enable_global_auto_scan(&svc).await;
    sqlx::query("UPDATE manga SET auto_download = 1, auto_scan = 0 WHERE id = ?")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.run_auto_scan_once().await;

    let chapters: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chapters")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        chapters, 0,
        "a manga with auto_scan off is not scanned at all"
    );
    assert_eq!(download_job_count(&svc).await, 0, "and nothing is enqueued");
}

use kani_app::models::{RefreshFields, RefreshOptions};

const SOURCE_TITLE: &str = "Source Title";
const SOURCE_DESC: &str = "Source description from origin";
const DETAILS_HTML: &str = r#"<html><body><div class="manga"><h1>x</h1></div></body></html>"#;

fn details_endpoint(cover_url: &str) -> ValidatedEndpoint {
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
        fields: vec![
            field("id", Expr::lit("m1")),
            field("title", Expr::lit(SOURCE_TITLE)),
            field("cover_url", Expr::lit(cover_url)),
            field("description", Expr::lit(SOURCE_DESC)),
            field("status", Expr::lit("ongoing")),
            field(
                "authors",
                Expr::list(vec![Expr::lit("Alice"), Expr::lit("Bob")]),
            ),
            field("artists", Expr::list(vec![])),
            field(
                "tags",
                Expr::list(vec![Expr::lit("Action"), Expr::lit("Drama")]),
            ),
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
    }
}

/// Register a YAML source that resolves manga_details (and chapter_list) against
/// `origin`, returning the id of the one manga row.
async fn wire_source_with_details(svc: &AppService, origin: &TestOrigin) -> MangaId {
    let source_id = insert_source(&svc.db, "fixture-source").await;
    let manga_id = insert_manga(&svc.db, source_id, "m1", "Fixture Manga").await;
    let ext = ValidatedExtension {
        id: "fixture-source".into(),
        name: "Fixture Source".into(),
        version: "1.0.0".into(),
        base_url: origin.base(),
        language: "en".into(),
        unrestricted_http: true,
        manga_details: Some(details_endpoint(&origin.url("/cover.jpg"))),
        chapter_list: Some(chapter_list_endpoint()),
        ..Default::default()
    };
    svc.sources.insert(
        source_id,
        SourceBackend::Yaml(Box::new(YamlSource::new(
            Arc::new(ext),
            kani_core::http::SmartClient::new(None)
                .unwrap()
                .with_allow_loopback_egress(true),
            Arc::new(kani_core::cache::InMemoryCache::new()),
            "test:".into(),
            HashMap::new(),
            true,
        ))),
    );
    manga_id
}

fn no_fields() -> RefreshFields {
    RefreshFields {
        cover: false,
        title: false,
        description: false,
        status: false,
        people: false,
        tags: false,
    }
}

fn refresh_opts(fields: RefreshFields, clear_overrides: bool) -> RefreshOptions {
    RefreshOptions {
        fields,
        fetch_chapters: false,
        clear_overrides,
    }
}

async fn manga_scalar<T>(svc: &AppService, id: MangaId, col: &str) -> T
where
    T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
{
    sqlx::query_scalar::<_, T>(&format!("SELECT {col} FROM manga WHERE id = ?"))
        .bind(id.0)
        .fetch_one(&svc.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn refresh_does_not_overwrite_a_pinned_cover() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    let svc = test_service().await;
    let id = wire_source_with_details(&svc, &origin).await;
    sqlx::query(
        "UPDATE manga SET cover_overridden = 1, local_cover_path = '/library/custom.png' WHERE id = ?",
    )
    .bind(id.0)
    .execute(&svc.db)
    .await
    .unwrap();

    svc.refresh_manga_with_options(
        id,
        refresh_opts(
            RefreshFields {
                cover: true,
                ..no_fields()
            },
            false,
        ),
    )
    .await
    .unwrap();

    assert_eq!(
        manga_scalar::<Option<String>>(&svc, id, "local_cover_path").await,
        Some("/library/custom.png".into()),
        "the pinned cover path is untouched"
    );
    assert!(
        manga_scalar::<bool>(&svc, id, "cover_overridden").await,
        "the override flag stays set"
    );
}

#[tokio::test]
async fn refresh_title_touches_only_the_title() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    let svc = test_service().await;
    let id = wire_source_with_details(&svc, &origin).await;
    sqlx::query("UPDATE manga SET description = 'old desc', status = 3 WHERE id = ?")
        .bind(id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.refresh_manga_with_options(
        id,
        refresh_opts(
            RefreshFields {
                title: true,
                ..no_fields()
            },
            false,
        ),
    )
    .await
    .unwrap();

    assert_eq!(manga_scalar::<String>(&svc, id, "name").await, SOURCE_TITLE);
    assert_eq!(
        manga_scalar::<Option<String>>(&svc, id, "description").await,
        Some("old desc".into()),
        "an unselected field is preserved"
    );
    assert_eq!(manga_scalar::<i64>(&svc, id, "status").await, 3);
}

#[tokio::test]
async fn refresh_description_touches_only_the_description() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    let svc = test_service().await;
    let id = wire_source_with_details(&svc, &origin).await;
    let before = manga_scalar::<String>(&svc, id, "name").await;

    svc.refresh_manga_with_options(
        id,
        refresh_opts(
            RefreshFields {
                description: true,
                ..no_fields()
            },
            false,
        ),
    )
    .await
    .unwrap();

    assert_eq!(
        manga_scalar::<Option<String>>(&svc, id, "description").await,
        Some(SOURCE_DESC.into())
    );
    assert_eq!(manga_scalar::<String>(&svc, id, "name").await, before);
}

#[tokio::test]
async fn refresh_status_touches_only_the_status() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    let svc = test_service().await;
    let id = wire_source_with_details(&svc, &origin).await;
    sqlx::query("UPDATE manga SET status = 3 WHERE id = ?")
        .bind(id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.refresh_manga_with_options(
        id,
        refresh_opts(
            RefreshFields {
                status: true,
                ..no_fields()
            },
            false,
        ),
    )
    .await
    .unwrap();

    assert_eq!(manga_scalar::<i64>(&svc, id, "status").await, 0);
}

#[tokio::test]
async fn refresh_cover_with_clear_overrides_drops_the_pin() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    let svc = test_service().await;
    let id = wire_source_with_details(&svc, &origin).await;
    sqlx::query(
        "UPDATE manga SET cover_overridden = 1, local_cover_path = '/library/custom.png' WHERE id = ?",
    )
    .bind(id.0)
    .execute(&svc.db)
    .await
    .unwrap();

    svc.refresh_manga_with_options(
        id,
        refresh_opts(
            RefreshFields {
                cover: true,
                ..no_fields()
            },
            true,
        ),
    )
    .await
    .unwrap();

    assert_eq!(
        manga_scalar::<Option<String>>(&svc, id, "local_cover_path").await,
        None,
        "the pin is cleared"
    );
    assert!(!manga_scalar::<bool>(&svc, id, "cover_overridden").await);
}

#[tokio::test]
async fn refresh_people_replaces_the_people_set() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    let svc = test_service().await;
    let id = wire_source_with_details(&svc, &origin).await;

    svc.refresh_manga_with_options(
        id,
        refresh_opts(
            RefreshFields {
                people: true,
                ..no_fields()
            },
            false,
        ),
    )
    .await
    .unwrap();

    let people: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga_people WHERE manga_id = ?")
        .bind(id.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(people, 2, "Alice and Bob are synced from the source");
}

#[tokio::test]
async fn refresh_tags_replaces_the_tag_set() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    let svc = test_service().await;
    let id = wire_source_with_details(&svc, &origin).await;

    svc.refresh_manga_with_options(
        id,
        refresh_opts(
            RefreshFields {
                tags: true,
                ..no_fields()
            },
            false,
        ),
    )
    .await
    .unwrap();

    let tags: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga_tags WHERE manga_id = ?")
        .bind(id.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(tags, 2, "Action and Drama are synced from the source");
}

#[tokio::test]
async fn refresh_with_clear_overrides_nulls_text_overrides() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    let svc = test_service().await;
    let id = wire_source_with_details(&svc, &origin).await;
    sqlx::query(
        "UPDATE manga SET local_name = 'My Title', local_description = 'My Desc', local_status = 2 WHERE id = ?",
    )
    .bind(id.0)
    .execute(&svc.db)
    .await
    .unwrap();

    svc.refresh_manga_with_options(
        id,
        refresh_opts(
            RefreshFields {
                title: true,
                description: true,
                status: true,
                ..no_fields()
            },
            true,
        ),
    )
    .await
    .unwrap();

    assert_eq!(
        manga_scalar::<Option<String>>(&svc, id, "local_name").await,
        None
    );
    assert_eq!(
        manga_scalar::<Option<String>>(&svc, id, "local_description").await,
        None
    );
    assert_eq!(
        manga_scalar::<Option<i64>>(&svc, id, "local_status").await,
        None
    );
}

#[tokio::test]
async fn a_failed_fetch_leaves_metadata_intact() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::status(500));
    let svc = test_service().await;
    let id = wire_source_with_details(&svc, &origin).await;
    sqlx::query(
        "UPDATE manga SET name = 'Keep Me', description = 'Keep Desc', status = 1 WHERE id = ?",
    )
    .bind(id.0)
    .execute(&svc.db)
    .await
    .unwrap();

    let res = svc
        .refresh_manga_with_options(id, refresh_opts(RefreshFields::default(), false))
        .await;

    assert!(res.is_err(), "the refresh surfaces the fetch failure");
    assert_eq!(manga_scalar::<String>(&svc, id, "name").await, "Keep Me");
    assert_eq!(
        manga_scalar::<Option<String>>(&svc, id, "description").await,
        Some("Keep Desc".into())
    );
    assert_eq!(manga_scalar::<i64>(&svc, id, "status").await, 1);
}

#[tokio::test]
async fn refresh_preserves_unselected_source_scalars() {
    let origin = TestOrigin::start().await;
    origin.set("/manga/m1", Response::html(DETAILS_HTML));
    let svc = test_service().await;
    let id = wire_source_with_details(&svc, &origin).await;
    sqlx::query(
        "UPDATE manga SET cover_url = 'http://old/cover.png', description = 'old', status = 3 WHERE id = ?",
    )
    .bind(id.0)
    .execute(&svc.db)
    .await
    .unwrap();

    svc.refresh_manga_with_options(
        id,
        refresh_opts(
            RefreshFields {
                title: true,
                ..no_fields()
            },
            false,
        ),
    )
    .await
    .unwrap();

    assert_eq!(
        manga_scalar::<Option<String>>(&svc, id, "cover_url").await,
        Some("http://old/cover.png".into())
    );
    assert_eq!(
        manga_scalar::<Option<String>>(&svc, id, "description").await,
        Some("old".into())
    );
    assert_eq!(manga_scalar::<i64>(&svc, id, "status").await, 3);
}
