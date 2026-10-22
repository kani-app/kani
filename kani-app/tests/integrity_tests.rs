#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_chapter, insert_manga, insert_source, test_service};
use kani_app::ids::ChapterId;
use std::io::Write;
use std::path::Path;

fn png_bytes(shade: u8) -> Vec<u8> {
    let mut img = image::GrayImage::new(24, 32);
    for (x, _y, p) in img.enumerate_pixels_mut() {
        *p = image::Luma([shade.wrapping_add((x % 255) as u8)]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn write_cbz(path: &Path, shades: &[u8]) {
    write_cbz_stamped(path, shades, zip::DateTime::default());
}

/// The entry timestamp is explicit because the default is the current time,
/// which makes two archives of identical pages differ in bytes — and
/// `archive_hash` is deliberately a hash of the file as it sits on disk.
fn write_cbz_stamped(path: &Path, shades: &[u8], stamp: zip::DateTime) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(stamp);
    for (i, shade) in shades.iter().enumerate() {
        zip.start_file(format!("{:04}.png", i + 1), opts).unwrap();
        zip.write_all(&png_bytes(*shade)).unwrap();
    }
    zip.finish().unwrap();
}

/// Seeds a downloaded chapter whose CBZ sits at the title-derived path, which
/// is where the pre-backfill derivation expects to find it.
async fn seed_downloaded_chapter(
    svc: &kani_app::service::AppService,
    manga_name: &str,
) -> (ChapterId, std::path::PathBuf) {
    let src = insert_source(&svc.db, "src").await;
    let manga = insert_manga(&svc.db, src, "m1", manga_name).await;
    let chapter = insert_chapter(&svc.db, manga, "c1", 1.0).await;

    sqlx::query("UPDATE chapters SET download_status = 2 WHERE id = ?")
        .bind(chapter)
        .execute(&svc.db)
        .await
        .unwrap();

    let library = { svc.settings.read().await.library_path.clone() };
    std::fs::create_dir_all(library.join(format!(
        "{} - {}",
        kani_core::utilities::sanitize_filename(manga_name),
        manga.0
    )))
    .unwrap();

    let cbz = svc.chapter_cbz_path(chapter).await.unwrap().path;
    write_cbz(&cbz, &[10, 90]);
    (chapter, cbz)
}

#[tokio::test]
async fn recording_a_manifest_populates_every_content_column() {
    let svc = test_service().await;
    let (chapter, _) = seed_downloaded_chapter(&svc, "Test Manga").await;

    let path = svc.chapter_cbz_path(chapter).await.unwrap().path;
    svc.record_chapter_manifest(chapter, path).await;

    let row = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>, Option<i64>, Option<i64>, Option<i64>)>(
        "SELECT file_path, content_hash, manifest_json, file_verified_at, quality_long_edge, page_count \
         FROM chapters WHERE id = ?",
    )
    .bind(chapter)
    .fetch_one(&svc.db)
    .await
    .unwrap();

    assert!(row.0.is_some(), "file_path should be stored");
    assert_eq!(
        row.1.as_ref().map(|h| h.len()),
        Some(64),
        "content_hash is blake3 hex"
    );
    assert!(row.2.is_some(), "manifest_json should be stored");
    assert!(row.3.is_some(), "file_verified_at should be set");
    assert_eq!(row.4, Some(32), "long edge of a 24x32 page");
    assert_eq!(row.5, Some(2), "page_count measured from the archive");

    let manifest: kani_core::manifest::ChapterManifest =
        serde_json::from_str(&row.2.unwrap()).unwrap();
    assert_eq!(manifest.page_count, 2);
    assert_eq!(manifest.pages.len(), 2);
}

#[tokio::test]
async fn stored_path_is_relative_to_the_library_root() {
    let svc = test_service().await;
    let (chapter, _) = seed_downloaded_chapter(&svc, "Relative Check").await;

    let path = svc.chapter_cbz_path(chapter).await.unwrap().path;
    svc.record_chapter_manifest(chapter, path).await;

    let stored: Option<String> = sqlx::query_scalar("SELECT file_path FROM chapters WHERE id = ?")
        .bind(chapter)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let stored = stored.unwrap();

    assert!(
        !stored.starts_with('/') && !stored.contains(':'),
        "path must be library-relative so relocation cannot invalidate it, got {stored}"
    );
    assert!(stored.ends_with(".cbz"), "got {stored}");
}

/// The configured `library_path` is not always the canonical form of the path a
/// resolved CBZ carries — the shipped default is the relative `./library`, and a
/// root can be a symlink. A stored path derived by naive prefix-stripping is then
/// silently NULL, which is how this shipped: every native install kept re-running
/// the backfill and never gained rename safety.
#[cfg(unix)]
#[tokio::test]
async fn a_library_root_that_is_not_canonical_still_stores_the_path() {
    let svc = test_service().await;
    let real_root = { svc.settings.read().await.library_path.clone() };
    let link = real_root
        .parent()
        .unwrap()
        .join(format!("kani-library-link-{}", std::process::id()));
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&real_root, &link).unwrap();
    {
        svc.settings.write().await.library_path = link.clone();
    }

    let (chapter, _) = seed_downloaded_chapter(&svc, "Symlinked Root").await;
    let path = svc.chapter_cbz_path(chapter).await.unwrap().path;
    svc.record_chapter_manifest(chapter, path).await;

    let stored: Option<String> = sqlx::query_scalar("SELECT file_path FROM chapters WHERE id = ?")
        .bind(chapter)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let stored = stored.expect("file_path must be stored even when the root needs canonicalising");
    assert!(!stored.starts_with('/'), "got {stored}");
    assert!(stored.ends_with(".cbz"), "got {stored}");

    sqlx::query("UPDATE manga SET name = 'Renamed After Storing' WHERE id = (SELECT manga_id FROM chapters WHERE id = ?)")
        .bind(chapter)
        .execute(&svc.db)
        .await
        .unwrap();
    let resolved = svc.chapter_cbz_path(chapter).await.unwrap().path;
    assert!(
        resolved.exists(),
        "the stored path must still resolve after a rename, got {}",
        resolved.display()
    );

    let _ = std::fs::remove_file(&link);
}

#[tokio::test]
async fn a_chapter_outside_the_library_root_stores_no_path() {
    let svc = test_service().await;
    let (chapter, _) = seed_downloaded_chapter(&svc, "Outside Root").await;

    let elsewhere = tempfile::Builder::new()
        .prefix("kani-out-")
        .tempdir()
        .unwrap();
    let stray = elsewhere.path().join("stray.cbz");
    write_cbz(&stray, &[10, 90]);

    svc.record_chapter_manifest(chapter, stray).await;

    let stored: Option<String> = sqlx::query_scalar("SELECT file_path FROM chapters WHERE id = ?")
        .bind(chapter)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        stored, None,
        "a path outside the library must not be stored"
    );

    let hash: Option<String> = sqlx::query_scalar("SELECT content_hash FROM chapters WHERE id = ?")
        .bind(chapter)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert!(
        hash.is_some(),
        "the manifest itself is still worth recording"
    );
}

#[tokio::test]
async fn renaming_a_manga_does_not_orphan_its_chapter() {
    let svc = test_service().await;
    let (chapter, cbz) = seed_downloaded_chapter(&svc, "Original Title").await;

    let path = svc.chapter_cbz_path(chapter).await.unwrap().path;
    svc.record_chapter_manifest(chapter, path).await;

    sqlx::query("UPDATE manga SET name = 'Completely Different Title' WHERE id = (SELECT manga_id FROM chapters WHERE id = ?)")
        .bind(chapter)
        .execute(&svc.db)
        .await
        .unwrap();

    let resolved = svc.chapter_cbz_path(chapter).await.unwrap().path;
    assert_eq!(resolved, cbz, "must still resolve to the real file");
    assert!(resolved.exists(), "resolved path should exist on disk");
}

#[tokio::test]
async fn a_chapter_without_a_stored_path_still_resolves_by_derivation() {
    let svc = test_service().await;
    let (chapter, cbz) = seed_downloaded_chapter(&svc, "Pre Backfill").await;

    let resolved = svc.chapter_cbz_path(chapter).await.unwrap().path;
    assert_eq!(
        resolved, cbz,
        "the derivation fallback must keep working for un-backfilled rows"
    );
}

#[tokio::test]
async fn clearing_a_manifest_removes_the_content_columns() {
    let svc = test_service().await;
    let (chapter, _) = seed_downloaded_chapter(&svc, "Clear Me").await;
    let path = svc.chapter_cbz_path(chapter).await.unwrap().path;
    svc.record_chapter_manifest(chapter, path).await;

    svc.clear_chapter_manifest(chapter).await.unwrap();

    let row = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>)>(
        "SELECT file_path, content_hash, manifest_json FROM chapters WHERE id = ?",
    )
    .bind(chapter)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(
        (row.0, row.1, row.2),
        (None, None, None),
        "a deleted file must not leave a stale hash behind"
    );
}

#[tokio::test]
async fn identical_chapters_share_a_content_hash() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;

    let mut hashes = Vec::new();
    for (i, title) in ["Series A", "Series B"].iter().enumerate() {
        let manga = insert_manga(&svc.db, src, &format!("m{i}"), title).await;
        let chapter = insert_chapter(&svc.db, manga, "c1", 1.0).await;
        sqlx::query("UPDATE chapters SET download_status = 2 WHERE id = ?")
            .bind(chapter)
            .execute(&svc.db)
            .await
            .unwrap();

        let library = { svc.settings.read().await.library_path.clone() };
        std::fs::create_dir_all(library.join(format!(
            "{} - {}",
            kani_core::utilities::sanitize_filename(title),
            manga.0
        )))
        .unwrap();

        let path = svc.chapter_cbz_path(chapter).await.unwrap().path;
        write_cbz(&path, &[10, 90]);
        svc.record_chapter_manifest(chapter, path).await;

        let h: Option<String> =
            sqlx::query_scalar("SELECT content_hash FROM chapters WHERE id = ?")
                .bind(chapter)
                .fetch_one(&svc.db)
                .await
                .unwrap();
        hashes.push(h.unwrap());
    }

    assert_eq!(
        hashes[0], hashes[1],
        "byte-identical chapters under different manga must hash the same — \
         this is what exact-duplicate detection keys on"
    );
}

#[tokio::test]
async fn migrating_a_manga_repoints_stored_chapter_paths() {
    let svc = test_service().await;
    let (chapter, cbz) = seed_downloaded_chapter(&svc, "Old Source Title").await;

    let path = svc.chapter_cbz_path(chapter).await.unwrap().path;
    svc.record_chapter_manifest(chapter, path).await;

    let manga_id: i64 = sqlx::query_scalar("SELECT manga_id FROM chapters WHERE id = ?")
        .bind(chapter)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let library = { svc.settings.read().await.library_path.clone() };
    let old_dir = library.join(format!("Old Source Title - {manga_id}"));
    let new_dir = library.join(format!("New Source Title - {manga_id}"));

    std::fs::rename(&old_dir, &new_dir).unwrap();
    sqlx::query("UPDATE manga SET name = 'New Source Title' WHERE id = ?")
        .bind(manga_id)
        .execute(&svc.db)
        .await
        .unwrap();

    let old_prefix = format!("Old Source Title - {manga_id}/");
    let new_prefix = format!("New Source Title - {manga_id}/");
    sqlx::query(
        "UPDATE chapters SET file_path = ? || substr(file_path, length(?) + 1) \
         WHERE manga_id = ? AND substr(file_path, 1, length(?)) = ?",
    )
    .bind(&new_prefix)
    .bind(&old_prefix)
    .bind(manga_id)
    .bind(&old_prefix)
    .bind(&old_prefix)
    .execute(&svc.db)
    .await
    .unwrap();

    let resolved = svc.chapter_cbz_path(chapter).await.unwrap().path;
    assert_ne!(
        resolved, cbz,
        "the file moved, so the path must have changed"
    );
    assert!(
        resolved.starts_with(&new_dir),
        "stored path should follow the directory rename, got {resolved:?}"
    );
    assert!(resolved.exists(), "repointed path must exist on disk");
}

#[tokio::test]
async fn deleting_a_chapter_clears_its_content_columns() {
    let svc = test_service().await;
    let (chapter, cbz) = seed_downloaded_chapter(&svc, "Delete Me").await;
    let path = svc.chapter_cbz_path(chapter).await.unwrap().path;
    svc.record_chapter_manifest(chapter, path).await;

    let before: Option<String> =
        sqlx::query_scalar("SELECT content_hash FROM chapters WHERE id = ?")
            .bind(chapter)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert!(before.is_some(), "precondition: hash was recorded");

    svc.delete_downloaded(chapter).await.unwrap();

    let row = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>, Option<i64>)>(
        "SELECT file_path, content_hash, manifest_json, file_verified_at \
         FROM chapters WHERE id = ?",
    )
    .bind(chapter)
    .fetch_one(&svc.db)
    .await
    .unwrap();

    assert_eq!(
        (row.0, row.1, row.2, row.3),
        (None, None, None, None),
        "delete must not leave content addressing behind"
    );
    assert!(!cbz.exists(), "the file itself should be gone");
}

#[tokio::test]
async fn stale_stored_path_does_not_survive_a_redownload() {
    let svc = test_service().await;
    let (chapter, old_cbz) = seed_downloaded_chapter(&svc, "Before Rename").await;
    let path = svc.chapter_cbz_path(chapter).await.unwrap().path;
    svc.record_chapter_manifest(chapter, path).await;

    let manga_id: i64 = sqlx::query_scalar("SELECT manga_id FROM chapters WHERE id = ?")
        .bind(chapter)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    sqlx::query("UPDATE manga SET name = 'After Rename' WHERE id = ?")
        .bind(manga_id)
        .execute(&svc.db)
        .await
        .unwrap();

    let library = { svc.settings.read().await.library_path.clone() };
    let new_dir = library.join(format!("After Rename - {manga_id}"));
    std::fs::create_dir_all(&new_dir).unwrap();
    svc.clear_chapter_manifest(chapter).await.unwrap();
    let fresh = svc.chapter_cbz_path(chapter).await.unwrap().path;
    write_cbz(&fresh, &[7, 7]);
    svc.record_chapter_manifest(chapter, fresh.clone()).await;

    assert!(
        fresh.starts_with(&new_dir),
        "a re-download resolves to the current title, got {fresh:?}"
    );
    assert_ne!(fresh, old_cbz);

    let resolved = svc.chapter_cbz_path(chapter).await.unwrap().path;
    assert_eq!(resolved, fresh, "stored path must describe the new file");
}

use kani_app::service::integrity::ScrubDepth;

async fn seed_and_record(
    svc: &kani_app::service::AppService,
    manga_name: &str,
) -> (ChapterId, std::path::PathBuf) {
    let (chapter, cbz) = seed_downloaded_chapter(svc, manga_name).await;
    svc.record_chapter_manifest(chapter, cbz.clone()).await;
    (chapter, cbz)
}

#[tokio::test]
async fn a_healthy_chapter_reports_no_drift() {
    let svc = test_service().await;
    let (chapter, _) = seed_and_record(&svc, "Undrifted").await;
    sqlx::query("UPDATE chapters SET file_verified_at = NULL WHERE id = ?")
        .bind(chapter)
        .execute(&svc.db)
        .await
        .unwrap();

    let report = svc
        .scrub_library(ScrubDepth::Quick, false, None)
        .await
        .unwrap();

    assert_eq!(report.ok, 1);
    assert!(
        report.path_drift.is_empty(),
        "a file sitting exactly where the DB says must not be drift: {:?}",
        report.path_drift
    );
    assert!(report.missing_files.is_empty());
    assert!(report.orphaned_files.is_empty());

    let stored: Option<String> = sqlx::query_scalar("SELECT file_path FROM chapters WHERE id = ?")
        .bind(chapter)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert!(
        stored.is_some_and(|p| !p.starts_with('/')),
        "the scrub must leave the relative form alone"
    );
}

#[tokio::test]
async fn scrub_reports_a_missing_file() {
    let svc = test_service().await;
    let (chapter, cbz) = seed_and_record(&svc, "Gone").await;
    std::fs::remove_file(&cbz).unwrap();

    let report = svc
        .scrub_library(ScrubDepth::Quick, false, None)
        .await
        .unwrap();

    assert_eq!(report.missing_files, vec![chapter.0]);
    assert_eq!(report.ok, 0);
}

#[tokio::test]
async fn scrub_reports_corruption_and_deep_names_the_page() {
    let svc = test_service().await;
    let (chapter, cbz) = seed_and_record(&svc, "Rotten").await;

    let mut bytes = std::fs::read(&cbz).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    std::fs::write(&cbz, &bytes).unwrap();

    let quick = svc
        .scrub_library_inner(ScrubDepth::Quick, false, true, None)
        .await
        .unwrap();
    assert_eq!(quick.corrupt.len(), 1, "quick must notice the file changed");
    assert_eq!(quick.corrupt[0].0, chapter.0);
    assert_eq!(quick.corrupt[0].1, "ArchiveHashMismatch");

    let deep = svc
        .scrub_library_inner(ScrubDepth::Deep, false, true, None)
        .await
        .unwrap();
    assert_eq!(deep.corrupt.len(), 1);
    assert!(
        deep.corrupt[0].1.contains("Page") || deep.corrupt[0].1.contains("Unreadable"),
        "deep must localise the damage rather than just repeating the archive \
         hash verdict; got {}",
        deep.corrupt[0].1
    );
}

#[tokio::test]
async fn scrub_marks_verified_chapters() {
    let svc = test_service().await;
    let (chapter, _) = seed_and_record(&svc, "Healthy").await;
    sqlx::query("UPDATE chapters SET file_verified_at = NULL WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let report = svc
        .scrub_library(ScrubDepth::Quick, false, None)
        .await
        .unwrap();
    assert_eq!(report.ok, 1);
    assert!(report.corrupt.is_empty());

    let verified: Option<i64> =
        sqlx::query_scalar("SELECT file_verified_at FROM chapters WHERE id = ?")
            .bind(chapter.0)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert!(
        verified.is_some(),
        "a passing check must record when it passed"
    );
}

#[tokio::test]
async fn scrub_fix_repoints_path_drift() {
    let svc = test_service().await;
    let (chapter, cbz) = seed_and_record(&svc, "Drifter").await;
    sqlx::query("UPDATE chapters SET file_verified_at = NULL WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    sqlx::query("UPDATE chapters SET file_path = 'Drifter - 1/moved-away.cbz' WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let report = svc
        .scrub_library(ScrubDepth::Quick, true, None)
        .await
        .unwrap();
    assert_eq!(report.path_drift.len(), 1, "drift should be detected");
    assert!(
        report.missing_files.is_empty(),
        "a findable file is drift, not loss"
    );

    let stored: Option<String> = sqlx::query_scalar("SELECT file_path FROM chapters WHERE id = ?")
        .bind(chapter.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    let stored = stored.unwrap();
    assert!(
        !stored.starts_with('/') && !stored.contains(':'),
        "a repair must store the library-relative form, got {stored}"
    );

    assert_eq!(svc.chapter_cbz_path(chapter).await.unwrap().path, cbz);
    let again = svc
        .scrub_library(ScrubDepth::Quick, false, None)
        .await
        .unwrap();
    assert!(again.path_drift.is_empty(), "{again:?}");
}

#[tokio::test]
async fn scrub_fix_makes_a_missing_chapter_redownloadable() {
    let svc = test_service().await;
    let (chapter, cbz) = seed_and_record(&svc, "Vanished").await;
    std::fs::remove_file(&cbz).unwrap();

    svc.scrub_library(ScrubDepth::Quick, true, None)
        .await
        .unwrap();

    let (status, hash): (i64, Option<String>) =
        sqlx::query_as("SELECT download_status, content_hash FROM chapters WHERE id = ?")
            .bind(chapter.0)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert_eq!(status, 0, "a gone file must not still read as downloaded");
    assert!(hash.is_none(), "a stale hash would fail every later scrub");
}

#[tokio::test]
async fn scrub_never_deletes_orphans_even_when_fixing() {
    let svc = test_service().await;
    let (_, cbz) = seed_and_record(&svc, "Keeper").await;
    let orphan = cbz.with_file_name("not-in-the-db.cbz");
    std::fs::copy(&cbz, &orphan).unwrap();

    let report = svc
        .scrub_library(ScrubDepth::Quick, true, None)
        .await
        .unwrap();

    assert_eq!(report.orphaned_files.len(), 1);
    assert!(
        orphan.exists(),
        "fix must never remove a file — a scheduled scrub runs unattended, and \
         an orphan may be the only copy of something"
    );
}

#[tokio::test]
async fn scrub_groups_byte_identical_chapters() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "dupes").await;
    let library = { svc.settings.read().await.library_path.clone() };

    let mut ids = Vec::new();
    let mut paths = Vec::new();
    for (slug, title) in [("m1", "Twin A"), ("m2", "Twin B")] {
        let manga = insert_manga(&svc.db, src, slug, title).await;
        let chapter = insert_chapter(&svc.db, manga, "c1", 1.0).await;
        sqlx::query("UPDATE chapters SET download_status = 2 WHERE id = ?")
            .bind(chapter)
            .execute(&svc.db)
            .await
            .unwrap();
        std::fs::create_dir_all(library.join(format!(
            "{} - {}",
            kani_core::utilities::sanitize_filename(title),
            manga.0
        )))
        .unwrap();
        paths.push(svc.chapter_cbz_path(chapter).await.unwrap().path);
        ids.push(chapter.0);
    }

    write_cbz(&paths[0], &[10, 90]);
    std::fs::copy(&paths[0], &paths[1]).unwrap();
    for (chapter, path) in ids.iter().zip(paths.iter()) {
        svc.record_chapter_manifest(ChapterId(*chapter), path.clone())
            .await;
    }

    let report = svc
        .scrub_library(ScrubDepth::Quick, false, None)
        .await
        .unwrap();

    assert_eq!(report.exact_duplicates.len(), 1, "same bytes, one group");
    let mut group = report.exact_duplicates[0].clone();
    group.sort();
    let mut want = ids.clone();
    want.sort();
    assert_eq!(group, want);
}

#[tokio::test]
async fn the_same_pages_re_zipped_are_still_reported_as_duplicates() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let library = { svc.settings.read().await.library_path.clone() };

    let mut ids = Vec::new();
    let mut paths = Vec::new();
    for (slug, title) in [("m1", "Repack A"), ("m2", "Repack B")] {
        let manga = insert_manga(&svc.db, src, slug, title).await;
        let chapter = insert_chapter(&svc.db, manga, "c1", 1.0).await;
        sqlx::query("UPDATE chapters SET download_status = 2 WHERE id = ?")
            .bind(chapter)
            .execute(&svc.db)
            .await
            .unwrap();
        std::fs::create_dir_all(library.join(format!(
            "{} - {}",
            kani_core::utilities::sanitize_filename(title),
            manga.0
        )))
        .unwrap();
        paths.push(svc.chapter_cbz_path(chapter).await.unwrap().path);
        ids.push(chapter.0);
    }

    // Same pages, different containers: this is what a re-download or a repack
    // produces, and it is exactly what an archive hash cannot see through.
    write_cbz_stamped(&paths[0], &[10, 90], zip::DateTime::default());
    write_cbz_stamped(
        &paths[1],
        &[10, 90],
        zip::DateTime::from_date_and_time(1999, 6, 1, 12, 0, 0).unwrap(),
    );
    for (chapter, path) in ids.iter().zip(paths.iter()) {
        svc.record_chapter_manifest(ChapterId(*chapter), path.clone())
            .await;
    }

    let hashes: Vec<String> =
        sqlx::query_scalar("SELECT content_hash FROM chapters WHERE id IN (?, ?) ORDER BY id")
            .bind(ids[0])
            .bind(ids[1])
            .fetch_all(&svc.db)
            .await
            .unwrap();
    assert_ne!(
        hashes[0], hashes[1],
        "sanity: the archives must differ in bytes, or this proves nothing"
    );

    let report = svc
        .scrub_library(ScrubDepth::Quick, false, None)
        .await
        .unwrap();

    assert_eq!(
        report.exact_duplicates.len(),
        1,
        "identical pages in differently packed archives are still duplicates"
    );
    let mut group = report.exact_duplicates[0].clone();
    group.sort();
    let mut want = ids.clone();
    want.sort();
    assert_eq!(group, want);
}

#[tokio::test]
async fn a_chapter_without_a_hash_is_counted_not_condemned() {
    let svc = test_service().await;
    let (chapter, _) = seed_downloaded_chapter(&svc, "Unhashed").await;

    let report = svc
        .scrub_library(ScrubDepth::Quick, true, None)
        .await
        .unwrap();

    assert_eq!(report.unhashed, 1);
    assert!(
        report.corrupt.is_empty(),
        "a pre-backfill chapter has nothing to compare against; calling it \
         corrupt would delete a healthy download's status"
    );
    let status: i64 = sqlx::query_scalar("SELECT download_status FROM chapters WHERE id = ?")
        .bind(chapter.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        status, 2,
        "fix must leave an unhashed-but-present file alone"
    );
}

#[tokio::test]
async fn the_last_report_survives_for_later_reading() {
    let svc = test_service().await;
    seed_and_record(&svc, "Persisted").await;

    svc.scrub_library(ScrubDepth::Quick, false, None)
        .await
        .unwrap();
    let (depth, report, created_at) = svc.last_scrub_report().await.unwrap().unwrap();

    assert_eq!(depth, "quick");
    assert_eq!(report.checked, 1);
    assert!(created_at > 0);
}

#[tokio::test]
async fn delete_orphans_honours_dry_run_and_stays_inside_the_library() {
    let svc = test_service().await;
    let (_, cbz) = seed_and_record(&svc, "Bounded").await;
    let orphan = cbz.with_file_name("orphan.cbz");
    std::fs::copy(&cbz, &orphan).unwrap();
    let orphan_s = orphan.to_string_lossy().to_string();

    let dry = svc
        .delete_orphans(std::slice::from_ref(&orphan_s), true)
        .await
        .unwrap();
    assert_eq!(dry.removed_count, 1);
    assert!(orphan.exists(), "a dry run must not touch the disk");

    let outside = svc
        .delete_orphans(&["/etc/passwd".to_string()], false)
        .await
        .unwrap();
    assert_eq!(outside.removed_count, 0);
    assert_eq!(
        outside.failed_count, 1,
        "a path outside the library must be refused, not obeyed"
    );

    let real = svc.delete_orphans(&[orphan_s], false).await.unwrap();
    assert_eq!(real.removed_count, 1);
    assert!(!orphan.exists());
}

#[tokio::test]
async fn a_scheduled_scrub_skips_recently_verified_chapters() {
    use kani_app::service::integrity::ScrubDepth;
    let svc = test_service().await;
    let (chapter, path) = seed_downloaded_chapter(&svc, "Verified").await;

    svc.record_chapter_manifest(chapter, path).await;
    let verified: Option<i64> =
        sqlx::query_scalar("SELECT file_verified_at FROM chapters WHERE id = ?")
            .bind(chapter.0)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert!(
        verified.is_some(),
        "a download records the verification time"
    );

    let second = svc
        .scrub_library(ScrubDepth::Quick, false, None)
        .await
        .unwrap();
    assert_eq!(
        second.skipped_recently_verified, 1,
        "a chapter verified moments ago must not be re-hashed on the next \
         scheduled run"
    );
    assert_eq!(second.ok, 0, "nothing was hashed");
    assert_eq!(
        second.checked, 1,
        "it is still examined — existence and drift checks are cheap and must \
         not be skipped, or a file deleted after a scrub goes unnoticed"
    );

    let manual = svc
        .scrub_library_inner(ScrubDepth::Quick, false, true, None)
        .await
        .unwrap();
    assert_eq!(
        manual.ok, 1,
        "having clicked 'scrub now', hashing nothing would be a surprising answer"
    );
}

#[tokio::test]
async fn a_zero_revalidation_window_disables_the_skip() {
    use kani_app::service::integrity::ScrubDepth;
    let svc = test_service().await;
    let (chapter, path) = seed_downloaded_chapter(&svc, "Always").await;
    svc.record_chapter_manifest(chapter, path).await;
    {
        let mut s = svc.settings.write().await;
        s.integrity_revalidate_after_days = 0;
    }

    let again = svc
        .scrub_library(ScrubDepth::Quick, false, None)
        .await
        .unwrap();
    assert_eq!(again.ok, 1, "0 means hash every time");
    assert_eq!(again.skipped_recently_verified, 0);
}
