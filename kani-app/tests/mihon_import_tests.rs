#![allow(clippy::unwrap_used)]

mod common;

use kani_app::ids::UserId;
use kani_app::service::AppService;
use kani_app::service::import::tachiyomi::{Backup, TachiyomiImportOptions};
use prost::Message as _;
use sqlx::SqlitePool;
use std::io::Read as _;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mihon")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn decode(data: &[u8]) -> Backup {
    let mut gz = flate2::read::GzDecoder::new(data);
    let mut buf = Vec::new();
    gz.read_to_end(&mut buf).unwrap();
    Backup::decode(buf.as_slice()).unwrap()
}

async fn register_source(pool: &SqlitePool, name: &str, mihon_id: i64) -> i64 {
    let id = common::insert_source(pool, name).await;
    sqlx::query("UPDATE sources SET mihon_source_id = ? WHERE id = ?")
        .bind(mihon_id)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn register_every_source(pool: &SqlitePool, backup: &Backup) {
    let mut seen = std::collections::BTreeSet::new();
    for m in &backup.backup_manga {
        if seen.insert(m.source) {
            register_source(pool, &format!("Source {}", m.source), m.source).await;
        }
    }
}

fn options(chapter_progress: bool) -> TachiyomiImportOptions {
    TachiyomiImportOptions {
        import_manga: true,
        import_categories: true,
        import_tracking: true,
        import_chapter_progress: chapter_progress,
    }
}

async fn user(svc: &AppService) -> UserId {
    common::insert_user(&svc.db, "reader").await
}

#[tokio::test]
async fn a_real_mihon_backup_imports_its_series() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    assert!(
        backup.backup_manga.len() >= 4,
        "the fixture must carry several series"
    );

    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    let result = svc
        .import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    assert_eq!(result.imported_manga as usize, backup.backup_manga.len());
    assert_eq!(result.skipped_manga, 0);
    assert_eq!(result.pending_imports_added, 0);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);

    for m in &backup.backup_manga {
        let row = sqlx::query_as::<_, (String, Option<String>, Option<String>, i64)>(
            "SELECT name, description, cover_url, status FROM manga WHERE source_manga_id = ?",
        )
        .bind(&m.url)
        .fetch_one(&svc.db)
        .await
        .unwrap_or_else(|e| panic!("series '{}' was not stored: {e}", m.title));
        assert_eq!(row.0, m.title);
        assert_eq!(row.1.as_deref().unwrap_or_default(), m.description);
        assert_eq!(row.2.as_deref().unwrap_or_default(), m.thumbnail_url);
        let expected_status = match m.status {
            1 => 1,
            2 => 2,
            _ => 0,
        };
        assert_eq!(row.3, expected_status, "status drifted for '{}'", m.title);
    }

    let first = &backup.backup_manga[0];
    let genres: Vec<String> = sqlx::query_scalar(
        "SELECT t.name FROM tags t \
         JOIN manga_tags mt ON mt.tag_id = t.id \
         JOIN manga m ON m.id = mt.manga_id \
         WHERE m.source_manga_id = ? ORDER BY t.name",
    )
    .bind(&first.url)
    .fetch_all(&svc.db)
    .await
    .unwrap();
    let mut expected = first.genre.clone();
    expected.sort();
    assert_eq!(genres, expected, "genres did not survive");

    let authors: Vec<String> = sqlx::query_scalar(
        "SELECT p.name FROM people p \
         JOIN manga_people mp ON mp.person_id = p.id \
         JOIN manga m ON m.id = mp.manga_id \
         WHERE m.source_manga_id = ? AND mp.role = 'author'",
    )
    .bind(&first.url)
    .fetch_all(&svc.db)
    .await
    .unwrap();
    assert_eq!(authors, vec![first.author.clone()]);
}

#[tokio::test]
async fn the_preview_describes_the_backup_without_importing_it() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    register_every_source(&svc.db, &backup).await;

    let preview = svc.preview_tachiyomi_backup(&data).await.unwrap();

    assert_eq!(preview.total_manga as usize, backup.backup_manga.len());
    assert_eq!(
        preview.category_count as usize,
        backup.backup_categories.len()
    );
    assert!(
        preview.has_chapter_progress,
        "the fixture has read chapters"
    );
    assert!(preview.has_tracking, "the fixture has a tracking entry");
    assert_eq!(preview.pending_import_estimate, 0);
    assert_eq!(
        preview.sources.iter().map(|s| s.manga_count).sum::<u32>() as usize,
        backup.backup_manga.len()
    );
    assert!(preview.sources.iter().all(|s| s.found));

    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(stored, 0, "a preview must not write to the library");
}

#[tokio::test]
async fn categories_survive_the_import() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    svc.import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    for cat in &backup.backup_categories {
        let found: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories WHERE name = ?")
            .bind(&cat.name)
            .fetch_one(&svc.db)
            .await
            .unwrap();
        assert_eq!(found, 1, "category '{}' was not created", cat.name);
    }

    let assigned = backup
        .backup_manga
        .iter()
        .find(|m| !m.categories.is_empty())
        .expect("the fixture must have a series in a category");
    let expected_name = &backup.backup_categories[assigned.categories[0] as usize].name;

    let names: Vec<String> = sqlx::query_scalar(
        "SELECT c.name FROM categories c \
         JOIN manga_categories mc ON mc.category_id = c.id \
         JOIN manga m ON m.id = mc.manga_id \
         WHERE m.source_manga_id = ?",
    )
    .bind(&assigned.url)
    .fetch_all(&svc.db)
    .await
    .unwrap();
    assert_eq!(names, vec![expected_name.clone()], "membership was lost");
}

#[tokio::test]
async fn tracker_links_survive_the_import() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    svc.import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    let tracked = backup
        .backup_manga
        .iter()
        .find(|m| !m.tracking.is_empty())
        .expect("the fixture must have a tracked series");
    let entry = &tracked.tracking[0];

    let (tracker_name, remote_id): (String, String) = sqlx::query_as(
        "SELECT t.name, tmm.tracker_manga_id FROM tracker_manga_mappings tmm \
         JOIN trackers t ON t.id = tmm.tracker_id \
         JOIN manga m ON m.id = tmm.manga_id \
         WHERE m.source_manga_id = ?",
    )
    .bind(&tracked.url)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(tracker_name, "AniList", "syncId 2 is AniList");
    assert_eq!(remote_id, entry.media_id.to_string());

    let status: i64 = sqlx::query_scalar(
        "SELECT umt.status FROM user_manga_tracking umt \
         JOIN manga m ON m.id = umt.manga_id \
         WHERE m.source_manga_id = ? AND umt.user_id = ?",
    )
    .bind(&tracked.url)
    .bind(uid)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(status, 0, "Tachiyomi status 1 (Reading) maps to Kani 0");
}

#[tokio::test]
async fn read_progress_and_chapter_state_survive() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    let series = backup
        .backup_manga
        .iter()
        .find(|m| m.chapters.iter().any(|c| c.read || c.last_page_read > 0))
        .expect("the fixture must carry read state");

    let source_id: i64 = sqlx::query_scalar("SELECT id FROM sources WHERE mihon_source_id = ?")
        .bind(series.source)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let manga_id = common::insert_manga(&svc.db, source_id, &series.url, &series.title).await;
    for (idx, ch) in series.chapters.iter().enumerate() {
        common::insert_chapter(&svc.db, manga_id, &ch.url, idx as f64 + 1.0).await;
    }

    svc.import_tachiyomi_backup(uid, &data, options(true))
        .await
        .unwrap();

    let mut checked = 0;
    for ch in &series.chapters {
        let row: Option<(bool, i64)> = sqlx::query_as(
            "SELECT uct.is_read, uct.last_page_read FROM user_chapter_tracking uct \
             JOIN chapters c ON c.id = uct.chapter_id \
             WHERE c.manga_id = ? AND c.source_chapter_id = ? AND uct.user_id = ?",
        )
        .bind(manga_id)
        .bind(&ch.url)
        .bind(uid)
        .fetch_optional(&svc.db)
        .await
        .unwrap();

        if ch.read || ch.last_page_read > 0 {
            let (is_read, last_page) =
                row.unwrap_or_else(|| panic!("no progress row for '{}'", ch.name));
            assert_eq!(is_read, ch.read, "read flag drifted for '{}'", ch.name);
            assert_eq!(
                last_page,
                i64::from(ch.last_page_read),
                "last page drifted for '{}'",
                ch.name
            );
            checked += 1;
        } else {
            assert!(
                row.is_none(),
                "an unread chapter must not gain a progress row: '{}'",
                ch.name
            );
        }
    }
    assert!(checked > 0, "the assertions above never ran");
}

#[tokio::test]
async fn progress_against_a_differently_shaped_chapter_id_is_counted() {
    // The sibling test above seeds its chapter rows with the backup's own ids,
    // so a match is true by construction. A row from a real scan carries the
    // id that source's extension produced, which need not be the backup's.
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    let series = backup
        .backup_manga
        .iter()
        .find(|m| m.chapters.iter().any(|c| c.read || c.last_page_read > 0))
        .expect("the fixture must carry read state");
    let with_progress = series
        .chapters
        .iter()
        .filter(|c| c.read || c.last_page_read > 0)
        .count();

    let source_id: i64 = sqlx::query_scalar("SELECT id FROM sources WHERE mihon_source_id = ?")
        .bind(series.source)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let manga_id = common::insert_manga(&svc.db, source_id, &series.url, &series.title).await;
    for (idx, ch) in series.chapters.iter().enumerate() {
        let scanned_id = format!("kani::{}", ch.url);
        assert_ne!(scanned_id, ch.url);
        common::insert_chapter(&svc.db, manga_id, &scanned_id, idx as f64 + 1.0).await;
    }

    let result = svc
        .import_tachiyomi_backup(uid, &data, options(true))
        .await
        .unwrap();

    let progress_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_chapter_tracking uct \
         JOIN chapters c ON c.id = uct.chapter_id WHERE c.manga_id = ?",
    )
    .bind(manga_id)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(progress_rows, 0, "ids differ, so nothing should match");

    assert!(
        result.unmatched_progress as usize >= with_progress,
        "unmatched progress went uncounted: {}",
        result.unmatched_progress
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains(&series.title) && w.contains("chapters")),
        "the drop was silent: {:?}",
        result.warnings
    );
}

#[tokio::test]
async fn progress_waits_for_the_chapter_list_rather_than_being_dropped() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    let result = svc
        .import_tachiyomi_backup(uid, &data, options(true))
        .await
        .unwrap();

    assert_eq!(result.imported_manga as usize, backup.backup_manga.len());
    let progress_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_chapter_tracking")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        progress_rows, 0,
        "there are no chapter rows to attach progress to yet"
    );

    let expected: i64 = backup
        .backup_manga
        .iter()
        .map(|m| {
            m.chapters
                .iter()
                .filter(|c| c.read || c.last_page_read > 0)
                .count() as i64
        })
        .sum();
    assert!(expected > 0);

    let parked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga_import_progress")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        parked, expected,
        "progress must wait for the chapter list, not be dropped for arriving first"
    );
    assert_eq!(
        result.unmatched_progress, 0,
        "nothing was lost, so nothing should be reported lost"
    );
}

#[tokio::test]
async fn a_series_resembling_one_already_in_the_library_is_parked_for_review() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    let incoming = &backup.backup_manga[0];
    let other_source = common::insert_source(&svc.db, "Existing Source").await;
    let existing =
        common::insert_manga(&svc.db, other_source, "already-here", &incoming.title).await;

    let result = svc
        .import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    assert_eq!(result.possible_duplicates, 1, "{:?}", result.warnings);
    assert_eq!(
        result.imported_manga as usize,
        backup.backup_manga.len() - 1
    );

    let (title, duplicate_of): (String, Option<i64>) = sqlx::query_as(
        "SELECT title, possible_duplicate_of FROM pending_imports WHERE source_manga_id = ?",
    )
    .bind(&incoming.url)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(title, incoming.title);
    assert_eq!(
        duplicate_of,
        Some(existing.0),
        "the pending row must point at the series it resembles"
    );

    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga WHERE source_manga_id = ?")
        .bind(&incoming.url)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(stored, 0, "a parked series must not also be imported");
}

#[tokio::test]
async fn an_import_queues_linking_instead_of_fetching_with_the_backups_ids() {
    // A backup identifies manga by Mihon's own URL, which addresses nothing on
    // the Kani source, so the chapter fetch has to wait for the resolve job.
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    let result = svc
        .import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();
    assert_eq!(result.imported_manga as usize, backup.backup_manga.len());

    let queued: Vec<(String, String)> =
        sqlx::query_as("SELECT job_type, description FROM jobs WHERE job_type = 'import_resolve'")
            .fetch_all(&svc.db)
            .await
            .unwrap();
    assert!(
        !queued.is_empty(),
        "an import must queue linking work for its manga"
    );
    assert!(
        queued.iter().all(|(_, d)| d.contains("Link")),
        "the queued job should say what it is doing: {queued:?}"
    );
}

#[tokio::test]
async fn an_import_marks_its_manga_as_awaiting_linking() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    svc.import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    // The resolve job runs concurrently and moves rows on from 'pending', so the
    // durable invariant is that no imported id is left marked as trusted.
    let untrusted: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga_import_links")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        untrusted as usize,
        backup.backup_manga.len(),
        "an imported id must never be treated as one the source accepts"
    );
}

#[tokio::test]
async fn a_manga_whose_source_is_not_installed_stays_queued() {
    let svc = common::test_service().await;
    let src = common::insert_source(&svc.db, "src").await;
    let manga_id =
        common::insert_manga(&svc.db, src, "/manga/mihon-shaped-id", "Paper Cranes").await;
    sqlx::query("INSERT INTO manga_import_links (manga_id, status) VALUES (?, 'pending')")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.resolve_imported_manga(src, manga_id, "/manga/mihon-shaped-id", "Paper Cranes")
        .await
        .unwrap();

    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM manga_import_links WHERE manga_id = ?")
            .bind(manga_id.0)
            .fetch_optional(&svc.db)
            .await
            .unwrap();
    assert_eq!(
        status.as_deref(),
        Some("pending"),
        "a source that never answered proves nothing about the id"
    );
}

#[tokio::test]
async fn linking_reports_why_it_could_not_resolve_a_manga() {
    use kani_app::service::import::resolve::Resolution;

    let svc = common::test_service().await;
    let src = common::insert_source(&svc.db, "src").await;
    let manga_id =
        common::insert_manga(&svc.db, src, "/manga/mihon-shaped-id", "Paper Cranes").await;

    // No backend is registered, so nothing can be proved against the source.
    let outcome = svc
        .resolve_imported_manga(src, manga_id, "/manga/mihon-shaped-id", "Paper Cranes")
        .await
        .unwrap();
    assert!(
        matches!(outcome, Resolution::Deferred(_)),
        "an unreachable source must not invent an id: {outcome:?}"
    );

    let stored: String = sqlx::query_scalar("SELECT source_manga_id FROM manga WHERE id = ?")
        .bind(manga_id.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        stored, "/manga/mihon-shaped-id",
        "an unresolved manga keeps the id it came in with"
    );
}

#[tokio::test]
async fn a_source_is_matched_by_the_name_the_backup_carries() {
    // Mihon backups name every source they reference. An extension Kani knows
    // under that name is the same source, so a new extension does not need an
    // entry in the hardcoded id table before its backups can be imported.
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;

    // Installed under the backup's own name, with an id that matches nothing.
    let source_id = common::insert_source(&svc.db, "Third Source").await;

    let result = svc
        .import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    assert_eq!(
        result.imported_manga as usize,
        backup.backup_manga.len(),
        "every series should resolve by name: {:?}",
        result.warnings
    );
    assert_eq!(result.pending_imports_added, 0);

    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga WHERE source_id = ?")
        .bind(source_id)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(stored as usize, backup.backup_manga.len());
}

#[tokio::test]
async fn an_unknown_source_id_is_reported_not_silently_dropped() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;

    let known = backup.backup_manga[0].source;
    register_source(&svc.db, "Known Source", known).await;
    let unknown_count = backup
        .backup_manga
        .iter()
        .filter(|m| m.source != known)
        .count();
    assert!(unknown_count > 0, "the fixture must span several sources");

    let result = svc
        .import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    assert_eq!(result.skipped_manga as usize, unknown_count);
    assert_eq!(result.pending_imports_added as usize, unknown_count);
    assert_eq!(result.warnings.len(), unknown_count);

    for m in backup.backup_manga.iter().filter(|m| m.source != known) {
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pending_imports \
             WHERE origin = 'tachiyomi' AND source_manga_id = ? AND title = ?",
        )
        .bind(&m.url)
        .bind(&m.title)
        .fetch_one(&svc.db)
        .await
        .unwrap();
        assert_eq!(pending, 1, "'{}' was dropped instead of parked", m.title);
        assert!(
            result.warnings.iter().any(|w| w.contains(&m.title)),
            "no warning named '{}': {:?}",
            m.title,
            result.warnings
        );
    }

    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        stored as usize,
        backup.backup_manga.len() - unknown_count,
        "only the resolvable series belong in the library"
    );
}

#[tokio::test]
async fn a_truncated_backup_is_rejected_cleanly() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    let half = &data[..data.len() / 2];
    let err = svc
        .import_tachiyomi_backup(uid, half, options(false))
        .await
        .expect_err("half a gzip stream must not import");
    let message = err.to_string();
    assert!(
        message.contains("decompress") || message.contains("decode"),
        "unhelpful error: {message}"
    );

    assert!(
        svc.preview_tachiyomi_backup(half).await.is_err(),
        "the preview must reject it too"
    );

    for table in ["manga", "categories", "pending_imports"] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&svc.db)
            .await
            .unwrap();
        assert_eq!(count, 0, "a rejected backup wrote to {table}");
    }
}

#[tokio::test]
async fn a_backup_with_hostile_titles_is_stored_intact() {
    let data = fixture("hostile-titles.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    let titles: Vec<String> = backup
        .backup_manga
        .iter()
        .map(|m| m.title.clone())
        .collect();
    assert!(
        titles.iter().any(|t| t.contains("../"))
            && titles.iter().any(|t| t.contains('\0'))
            && titles.iter().any(|t| t.contains('\u{202e}')),
        "the fixture lost its hostile cases: {titles:?}"
    );

    svc.import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    for m in &backup.backup_manga {
        let stored: String = sqlx::query_scalar("SELECT name FROM manga WHERE source_manga_id = ?")
            .bind(&m.url)
            .fetch_one(&svc.db)
            .await
            .unwrap_or_else(|e| panic!("series {:?} was not stored: {e}", m.title));
        assert_eq!(stored, m.title, "the title was mangled in the DB");
    }

    let library = svc.settings.read().await.library_path.clone();
    for m in &backup.backup_manga {
        let manga_id: i64 = sqlx::query_scalar("SELECT id FROM manga WHERE source_manga_id = ?")
            .bind(&m.url)
            .fetch_one(&svc.db)
            .await
            .unwrap();
        let safe_name = format!(
            "{} - {}",
            kani_core::utilities::sanitize_filename(&m.title),
            manga_id
        );
        let dir = library.join(&safe_name);
        assert!(
            !safe_name.contains(".."),
            "traversal survived sanitisation: {safe_name:?}"
        );
        assert!(
            !safe_name.contains('\0'),
            "a NUL survived into a path: {safe_name:?}"
        );
        assert!(
            !safe_name.contains('/') && !safe_name.contains('\\'),
            "a separator survived sanitisation: {safe_name:?}"
        );
        assert_eq!(
            dir.parent(),
            Some(library.as_path()),
            "'{}' resolved outside the library: {}",
            m.title,
            dir.display()
        );
        assert!(
            dir.components().count() == library.components().count() + 1,
            "the stored title added a path level: {}",
            dir.display()
        );
    }
}

#[tokio::test]
async fn importing_twice_is_idempotent() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    svc.import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();
    let after_first: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga")
        .fetch_one(&svc.db)
        .await
        .unwrap();

    let second = svc
        .import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    let after_second: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        after_second, after_first,
        "the second import duplicated rows"
    );
    assert_eq!(second.pending_imports_added, 0);
    assert_eq!(second.possible_duplicates, 0);

    for table in ["categories", "manga_categories", "tracker_manga_mappings"] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&svc.db)
            .await
            .unwrap();
        let expected: i64 = match table {
            "categories" => backup.backup_categories.len() as i64,
            "manga_categories" => backup
                .backup_manga
                .iter()
                .map(|m| m.categories.len() as i64)
                .sum(),
            _ => backup
                .backup_manga
                .iter()
                .filter(|m| !m.tracking.is_empty())
                .count() as i64,
        };
        assert_eq!(count, expected, "{table} grew on the second import");
    }
}

#[tokio::test]
async fn parked_progress_is_applied_once_the_chapters_arrive() {
    use kani_app::service::import::progress::ImportedProgress;

    let svc = common::test_service().await;
    let uid = user(&svc).await;
    let src = common::insert_source(&svc.db, "src").await;
    let manga_id = common::insert_manga(&svc.db, src, "/manga/mihon-shaped-id", "Tidewalker").await;

    // Mihon stores the chapter number as a 32-bit float, so 1.1 reaches us as
    // 1.100000023841858 and never equals the 1.1 a source parsed.
    let entries = vec![
        ImportedProgress {
            source_chapter_id: "/chapter/mihon-1".to_string(),
            chapter_number: f64::from(1.0f32),
            is_read: true,
            last_page_read: 0,
        },
        ImportedProgress {
            source_chapter_id: "/chapter/mihon-1-1".to_string(),
            chapter_number: f64::from(1.1f32),
            is_read: false,
            last_page_read: 7,
        },
    ];
    svc.store_import_progress(manga_id, uid, &entries)
        .await
        .unwrap();

    let applied_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_chapter_tracking")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(applied_before, 0, "there is nothing to attach progress to");

    // The chapter list arrives with the source's own ids, which share nothing
    // with the backup's.
    let ch1 = common::insert_chapter(&svc.db, manga_id, "kani::abc", 1.0).await;
    let ch11 = common::insert_chapter(&svc.db, manga_id, "kani::def", 1.1).await;

    let unmatched = svc.apply_stored_import_progress(manga_id).await.unwrap();
    assert_eq!(unmatched, 0, "both entries name a chapter that now exists");

    let read: Vec<(i64, bool, i64)> = sqlx::query_as(
        "SELECT chapter_id, is_read, last_page_read FROM user_chapter_tracking \
         WHERE user_id = ? ORDER BY chapter_id",
    )
    .bind(uid)
    .fetch_all(&svc.db)
    .await
    .unwrap();
    assert_eq!(read, vec![(ch1.0, true, 0), (ch11.0, false, 7)]);

    let parked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga_import_progress")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(parked, 0, "applied progress is consumed");
}

#[tokio::test]
async fn the_preview_counts_what_it_found_so_an_empty_one_is_distinguishable() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;

    let preview = svc.preview_tachiyomi_backup(&data).await.unwrap();

    let chapters: usize = backup.backup_manga.iter().map(|m| m.chapters.len()).sum();
    let with_progress: usize = backup
        .backup_manga
        .iter()
        .map(|m| {
            m.chapters
                .iter()
                .filter(|c| c.read || c.last_page_read > 0)
                .count()
        })
        .sum();
    assert!(
        chapters > 0 && with_progress > 0,
        "fixture carries no progress"
    );

    let tracking: usize = backup.backup_manga.iter().map(|m| m.tracking.len()).sum();
    assert!(tracking > 0, "fixture carries no tracker links");

    assert_eq!(preview.chapter_count as usize, chapters);
    assert_eq!(preview.progress_entry_count as usize, with_progress);
    assert_eq!(preview.tracking_entry_count as usize, tracking);
    assert!(
        preview.has_tracking,
        "reading status comes from tracker links, so it must be offered when there are some"
    );
    assert!(
        preview.has_chapter_progress,
        "the option must be offered when the file has progress to import"
    );
}

#[tokio::test]
async fn re_importing_requeues_a_manga_that_gave_up_and_keeps_its_progress() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    // The state a user is left in after a first import whose linking failed:
    // in the library, no chapters, and marked as given up on.
    let series = backup
        .backup_manga
        .iter()
        .find(|m| m.chapters.iter().any(|c| c.read || c.last_page_read > 0))
        .expect("fixture has a series with progress");
    let source_id: i64 = sqlx::query_scalar("SELECT id FROM sources WHERE mihon_source_id = ?")
        .bind(series.source)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let manga_id = common::insert_manga(&svc.db, source_id, &series.url, &series.title).await;
    sqlx::query("INSERT INTO manga_import_links (manga_id, status) VALUES (?, 'unlinked')")
        .bind(manga_id.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let result = svc
        .import_tachiyomi_backup(uid, &data, options(true))
        .await
        .unwrap();

    let expected = series
        .chapters
        .iter()
        .filter(|c| c.read || c.last_page_read > 0)
        .count() as i64;
    let parked: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM manga_import_progress WHERE manga_id = ?")
            .bind(manga_id.0)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert_eq!(
        parked, expected,
        "progress for a manga with no chapters must wait, not be dropped"
    );

    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM manga_import_links WHERE manga_id = ?")
            .bind(manga_id.0)
            .fetch_optional(&svc.db)
            .await
            .unwrap();
    assert_eq!(
        status.as_deref(),
        Some("pending"),
        "re-importing must put a given-up manga back in the queue that drains it"
    );
    assert_eq!(
        result.unmatched_progress, 0,
        "nothing was lost, so nothing should be reported lost"
    );
}

#[tokio::test]
async fn overwriting_progress_keeps_the_timestamp_the_shelf_orders_by() {
    use kani_app::service::import::progress::ImportedProgress;

    let svc = common::test_service().await;
    let uid = user(&svc).await;
    let src = common::insert_source(&svc.db, "src").await;
    let manga_id = common::insert_manga(&svc.db, src, "/manga/mihon-shaped-id", "Tidewalker").await;
    let ch = common::insert_chapter(&svc.db, manga_id, "kani::abc", 1.0).await;

    sqlx::query(
        "INSERT INTO user_chapter_tracking (user_id, chapter_id, is_read, last_page_read, last_read_at) \
         VALUES (?, ?, 0, 3, '2026-01-02 03:04:05')",
    )
    .bind(uid)
    .bind(ch.0)
    .execute(&svc.db)
    .await
    .unwrap();

    svc.store_import_progress(
        manga_id,
        uid,
        &[ImportedProgress {
            source_chapter_id: "kani::abc".to_string(),
            chapter_number: 1.0,
            is_read: true,
            last_page_read: 12,
        }],
    )
    .await
    .unwrap();
    assert_eq!(svc.apply_stored_import_progress(manga_id).await.unwrap(), 0);

    let (is_read, last_page_read, last_read_at): (bool, i64, Option<String>) = sqlx::query_as(
        "SELECT is_read, last_page_read, last_read_at FROM user_chapter_tracking \
         WHERE user_id = ? AND chapter_id = ?",
    )
    .bind(uid)
    .bind(ch.0)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert!(is_read, "the import's read flag must win");
    assert_eq!(last_page_read, 12, "the import's page position must win");
    assert_eq!(
        last_read_at.as_deref(),
        Some("2026-01-02 03:04:05"),
        "continue-reading orders by this, so an import must not erase it"
    );
}

#[tokio::test]
async fn a_relinked_manga_is_recognised_rather_than_imported_again() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    // The state the resolve job leaves a manga in: the same series, on the same
    // source, under the id the source actually accepts rather than the backup's.
    let series = backup
        .backup_manga
        .iter()
        .find(|m| m.chapters.iter().any(|c| c.read || c.last_page_read != 0))
        .expect("fixture has a series with progress");
    let source_id: i64 = sqlx::query_scalar("SELECT id FROM sources WHERE mihon_source_id = ?")
        .bind(series.source)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let manga_id = common::insert_manga(&svc.db, source_id, "kani::linked-id", &series.title).await;
    for ch in &series.chapters {
        common::insert_chapter(&svc.db, manga_id, &ch.url, f64::from(ch.chapter_number)).await;
    }

    let result = svc
        .import_tachiyomi_backup(uid, &data, options(true))
        .await
        .unwrap();

    assert_eq!(
        result.possible_duplicates, 0,
        "a manga already in the library is not a duplicate of itself"
    );

    let expected = series
        .chapters
        .iter()
        .filter(|c| c.read || c.last_page_read != 0)
        .count() as i64;
    let applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_chapter_tracking uct \
         JOIN chapters c ON c.id = uct.chapter_id WHERE c.manga_id = ?",
    )
    .bind(manga_id.0)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(
        applied, expected,
        "progress must land on the manga already in the library"
    );
    assert_eq!(result.unmatched_progress, 0);
}

#[tokio::test]
async fn re_importing_over_a_manga_keeps_that_users_reader_settings() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    let series = backup
        .backup_manga
        .iter()
        .find(|m| !m.tracking.is_empty())
        .expect("fixture has a series with a tracker link");
    let source_id: i64 = sqlx::query_scalar("SELECT id FROM sources WHERE mihon_source_id = ?")
        .bind(series.source)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let manga_id = common::insert_manga(&svc.db, source_id, &series.url, &series.title).await;
    sqlx::query(
        "INSERT INTO user_manga_tracking (user_id, manga_id, status, notify_new_chapters, reader_prefs) \
         VALUES (?, ?, 0, 0, '{\"zoom\":\"fit-width\"}')",
    )
    .bind(uid)
    .bind(manga_id.0)
    .execute(&svc.db)
    .await
    .unwrap();

    svc.import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    let (notify, prefs): (bool, Option<String>) = sqlx::query_as(
        "SELECT notify_new_chapters, reader_prefs FROM user_manga_tracking \
         WHERE user_id = ? AND manga_id = ?",
    )
    .bind(uid)
    .bind(manga_id.0)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert!(
        !notify,
        "an import must not re-enable notifications this user turned off"
    );
    assert_eq!(
        prefs.as_deref(),
        Some("{\"zoom\":\"fit-width\"}"),
        "an import carries no reader preferences, so it must not clear them"
    );
}

#[tokio::test]
async fn progress_only_import_applies_to_the_library_that_is_already_there() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);
    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    let series = backup
        .backup_manga
        .iter()
        .find(|m| m.chapters.iter().any(|c| c.read || c.last_page_read != 0))
        .expect("fixture has a series with progress");
    let source_id: i64 = sqlx::query_scalar("SELECT id FROM sources WHERE mihon_source_id = ?")
        .bind(series.source)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let manga_id = common::insert_manga(&svc.db, source_id, &series.url, &series.title).await;
    for ch in &series.chapters {
        common::insert_chapter(&svc.db, manga_id, &ch.url, f64::from(ch.chapter_number)).await;
    }

    let result = svc
        .import_tachiyomi_backup(
            uid,
            &data,
            TachiyomiImportOptions {
                import_manga: false,
                import_categories: false,
                import_tracking: false,
                import_chapter_progress: true,
            },
        )
        .await
        .unwrap();

    let expected = series
        .chapters
        .iter()
        .filter(|c| c.read || c.last_page_read != 0)
        .count() as i64;
    let applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_chapter_tracking uct \
         JOIN chapters c ON c.id = uct.chapter_id WHERE c.manga_id = ?",
    )
    .bind(manga_id.0)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(
        applied, expected,
        "progress-only must reach the manga already in the library"
    );
    assert_eq!(
        result.pending_imports_added, 0,
        "a progress-only run must not add manga by another name"
    );
    let manga_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(manga_count, 1, "a progress-only run must not add manga");
}

/// A trashed series is not restored by an import, so counting it as imported
/// tells the user something landed in a library that still hides it.
#[tokio::test]
async fn a_trashed_series_is_reported_rather_than_counted_as_imported() {
    let data = fixture("suwayomi-anonymised.tachibk");
    let backup = decode(&data);

    let svc = common::test_service().await;
    let uid = user(&svc).await;
    register_every_source(&svc.db, &backup).await;

    svc.import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    let trashed_url = &backup.backup_manga[0].url;
    let trashed_id: i64 = sqlx::query_scalar("SELECT id FROM manga WHERE source_manga_id = ?")
        .bind(trashed_url)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    sqlx::query("UPDATE manga SET deleted_at = unixepoch() WHERE id = ?")
        .bind(trashed_id)
        .execute(&svc.db)
        .await
        .unwrap();

    let result = svc
        .import_tachiyomi_backup(uid, &data, options(false))
        .await
        .unwrap();

    assert_eq!(
        result.imported_manga as usize,
        backup.backup_manga.len() - 1,
        "the trashed series was counted as imported"
    );
    assert_eq!(
        result.trashed_manga, 1,
        "the trashed series was not reported"
    );

    let still_trashed: Option<i64> =
        sqlx::query_scalar("SELECT deleted_at FROM manga WHERE id = ?")
            .bind(trashed_id)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert!(
        still_trashed.is_some(),
        "the import silently resurrected a trashed series"
    );
}
