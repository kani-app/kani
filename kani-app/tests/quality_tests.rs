#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_chapter, insert_manga, insert_source, test_service};
use kani_app::ids::{ChapterId, MangaId};
use kani_app::service::quality::UpgradeKind;
use std::io::Write;
use std::path::Path;

fn png_bytes(shade: u8) -> Vec<u8> {
    let mut img = image::GrayImage::new(16, 24);
    for (x, _y, p) in img.enumerate_pixels_mut() {
        *p = image::Luma([shade.wrapping_add((x % 255) as u8)]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn write_cbz(path: &Path, pages: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..pages {
        zip.start_file(format!("{:04}.png", i + 1), opts).unwrap();
        zip.write_all(&png_bytes(i as u8 * 9)).unwrap();
    }
    zip.finish().unwrap();
}

/// A downloaded chapter whose manifest records `pages` pages.
async fn held_chapter(
    svc: &kani_app::service::AppService,
    manga: MangaId,
    title: &str,
    num: f64,
    pages: usize,
) -> ChapterId {
    let chapter = insert_chapter(&svc.db, manga, &format!("src-{num}"), num).await;
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
    let cbz = svc.chapter_cbz_path(chapter).await.unwrap().path;
    write_cbz(&cbz, pages);
    svc.record_chapter_manifest(chapter, cbz).await;
    chapter
}

/// Chapters of `manga` currently carrying an upgrade badge.
///
/// The badge renders from `chapters.upgrade_available` on the chapter listing,
/// so this asserts the same column the UI reads rather than a parallel path.
/// Dismissal empties `candidates` while leaving the row present — the badge is
/// the candidate count, not the column's nullity.
async fn pending_badges(svc: &kani_app::service::AppService, manga: MangaId) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM chapters \
         WHERE manga_id = ? AND upgrade_available IS NOT NULL \
           AND json_array_length(upgrade_available, '$.candidates') > 0",
    )
    .bind(manga)
    .fetch_one(&svc.db)
    .await
    .unwrap()
}

async fn seed_manga(svc: &kani_app::service::AppService, title: &str) -> MangaId {
    let src = insert_source(&svc.db, &format!("s-{title}")).await;
    insert_manga(&svc.db, src, "m1", title).await
}

#[tokio::test]
async fn a_longer_relisting_is_flagged_as_a_reupload() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Reupload").await;
    let chapter = held_chapter(&svc, manga, "Reupload", 1.0, 2).await;

    sqlx::query("UPDATE chapters SET source_page_count = 5 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, UpgradeKind::QualityReupload);
    assert_eq!(found[0].held_page_count, Some(2));
    assert_eq!(found[0].candidate_page_count, Some(5));
}

#[tokio::test]
async fn a_shorter_relisting_is_reassurance_not_a_nag() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Downgrade").await;
    let chapter = held_chapter(&svc, manga, "Downgrade", 1.0, 5).await;

    sqlx::query("UPDATE chapters SET source_page_count = 2 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].kind,
        UpgradeKind::SourceDowngraded,
        "yours is the better copy; this must never read as 'replace me'"
    );
    assert_eq!(found[0].reason_key, "upgrade.reason.source_downgraded");
}

#[tokio::test]
async fn a_matching_relisting_flags_nothing() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Stable").await;
    let chapter = held_chapter(&svc, manga, "Stable", 1.0, 3).await;
    sqlx::query("UPDATE chapters SET source_page_count = 3 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    assert!(svc.evaluate_upgrades(manga).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_better_ranked_scanlator_is_flagged() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Groups").await;
    let held = held_chapter(&svc, manga, "Groups", 1.0, 3).await;
    sqlx::query("UPDATE chapters SET scanlator = 'Low', page_count = 3 WHERE id = ?")
        .bind(held.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let rival = insert_chapter(&svc.db, manga, "rival", 1.0).await;
    sqlx::query("UPDATE chapters SET scanlator = 'High', page_count = 3 WHERE id = ?")
        .bind(rival)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.set_scanlator_pref(manga, "Low", 1, false)
        .await
        .unwrap();
    svc.set_scanlator_pref(manga, "High", 10, false)
        .await
        .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    let pref: Vec<_> = found
        .iter()
        .filter(|c| c.kind == UpgradeKind::PreferredScanlator)
        .collect();
    assert_eq!(pref.len(), 1);
    assert_eq!(pref[0].held_chapter_id, held.0);
    assert_eq!(pref[0].candidate_scanlator.as_deref(), Some("High"));
}

#[tokio::test]
async fn a_worse_ranked_scanlator_is_not_flagged() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Ranked").await;
    let held = held_chapter(&svc, manga, "Ranked", 1.0, 3).await;
    sqlx::query("UPDATE chapters SET scanlator = 'High', page_count = 3 WHERE id = ?")
        .bind(held.0)
        .execute(&svc.db)
        .await
        .unwrap();
    let rival = insert_chapter(&svc.db, manga, "rival", 1.0).await;
    sqlx::query("UPDATE chapters SET scanlator = 'Low', page_count = 3 WHERE id = ?")
        .bind(rival)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.set_scanlator_pref(manga, "Low", 1, false)
        .await
        .unwrap();
    svc.set_scanlator_pref(manga, "High", 10, false)
        .await
        .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert!(
        !found
            .iter()
            .any(|c| c.kind == UpgradeKind::PreferredScanlator),
        "you already hold the preferred group's release"
    );
}

#[tokio::test]
async fn a_blocked_scanlator_is_never_a_candidate() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Blocked").await;
    let held = held_chapter(&svc, manga, "Blocked", 1.0, 3).await;
    sqlx::query("UPDATE chapters SET scanlator = 'Low', page_count = 3 WHERE id = ?")
        .bind(held.0)
        .execute(&svc.db)
        .await
        .unwrap();
    let rival = insert_chapter(&svc.db, manga, "rival", 1.0).await;
    sqlx::query("UPDATE chapters SET scanlator = 'Banned', page_count = 3 WHERE id = ?")
        .bind(rival)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.set_scanlator_pref(manga, "Low", 1, false)
        .await
        .unwrap();
    svc.set_scanlator_pref(manga, "Banned", 99, true)
        .await
        .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert!(
        !found
            .iter()
            .any(|c| c.kind == UpgradeKind::PreferredScanlator),
        "a blocked group outranking everything would otherwise be recommended"
    );
}

#[tokio::test]
async fn dismissing_prevents_the_same_candidate_returning() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Dismissed").await;
    let chapter = held_chapter(&svc, manga, "Dismissed", 1.0, 2).await;
    sqlx::query("UPDATE chapters SET source_page_count = 6 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    assert_eq!(svc.evaluate_upgrades(manga).await.unwrap().len(), 1);
    svc.dismiss_upgrade(chapter).await.unwrap();
    assert_eq!(pending_badges(&svc, manga).await, 0);

    assert!(
        svc.evaluate_upgrades(manga).await.unwrap().is_empty(),
        "a dismissed candidate that returns on the next scan is worse than \
         never offering the button"
    );
}

#[tokio::test]
async fn detection_can_be_turned_off_entirely() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Off").await;
    let chapter = held_chapter(&svc, manga, "Off", 1.0, 2).await;
    sqlx::query("UPDATE chapters SET source_page_count = 9 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();
    {
        let mut s = svc.settings.write().await;
        s.upgrade_detection_enabled = false;
    }

    assert!(svc.evaluate_upgrades(manga).await.unwrap().is_empty());
}

#[tokio::test]
async fn applying_an_upgrade_keeps_the_old_file() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Applied").await;
    let chapter = held_chapter(&svc, manga, "Applied", 1.0, 3).await;
    let old_path = svc.chapter_cbz_path(chapter).await.unwrap().path;
    let library = { svc.settings.read().await.library_path.clone() };

    let _ = svc
        .apply_upgrade(chapter, Some(kani_app::ids::UserId(1)))
        .await;

    assert!(!old_path.exists(), "the held file is moved out of the way");
    let replaced: Vec<_> = std::fs::read_dir(library.join(".replaced"))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(
        replaced.len(),
        1,
        "the old copy must survive in .replaced — this is the only thing \
         making an upgrade reversible"
    );
    assert!(
        replaced[0]
            .file_name()
            .to_string_lossy()
            .starts_with(&format!("{}-", chapter.0)),
        "the holding file must name the chapter it came from"
    );
}

#[tokio::test]
async fn applying_an_upgrade_clears_the_stale_hashes() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Cleared").await;
    let chapter = held_chapter(&svc, manga, "Cleared", 1.0, 3).await;

    let _ = svc
        .apply_upgrade(chapter, Some(kani_app::ids::UserId(1)))
        .await;

    let (status, hash, flag): (i64, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT download_status, content_hash, upgrade_available FROM chapters WHERE id = ?",
    )
    .bind(chapter.0)
    .fetch_one(&svc.db)
    .await
    .unwrap();

    assert_ne!(status, 2, "the chapter is no longer downloaded");
    assert!(
        hash.is_none(),
        "the old hash would make the next scrub call the new file corrupt"
    );
    assert!(flag.is_none(), "the acted-on flag must clear");
}

#[tokio::test]
async fn the_replaced_sweep_respects_the_retention_window() {
    let svc = test_service().await;
    let library = { svc.settings.read().await.library_path.clone() };
    let dir = library.join(".replaced");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("1-abcdef.cbz");
    std::fs::write(&f, b"old").unwrap();

    assert_eq!(
        svc.purge_replaced(30).await.unwrap(),
        0,
        "a file inside the window must stay recoverable"
    );
    assert!(f.exists());

    assert_eq!(svc.purge_replaced(0).await.unwrap(), 1);
    assert!(!f.exists());
}

#[tokio::test]
async fn a_drifting_archive_count_alone_flags_nothing() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "ArchiveDrift").await;
    let chapter = held_chapter(&svc, manga, "ArchiveDrift", 1.0, 2).await;

    sqlx::query("UPDATE chapters SET page_count = 5 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    assert!(
        svc.evaluate_upgrades(manga).await.unwrap().is_empty(),
        "only the source listing can testify that the source changed"
    );
}

#[tokio::test]
async fn a_candidate_carries_the_measurements_of_the_copy_on_disk() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Measured").await;
    let chapter = held_chapter(&svc, manga, "Measured", 1.0, 3).await;
    sqlx::query("UPDATE chapters SET source_page_count = 7 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert_eq!(found.len(), 1);
    let held = found[0]
        .held_score
        .expect("the held side is measurable from its own manifest and must be carried");
    assert_eq!(
        held.median_long_edge_px, 24,
        "the fixture pages are 16x24, so the long edge is 24"
    );
    assert_eq!(held.page_count, 3);
    assert_eq!(
        held.colour,
        kani_core::quality::ColourProfile::Monochrome,
        "the fixture pages are greyscale and the manifest now records that"
    );

    assert!(found[0].candidate_score.is_none());
    assert!(found[0].verdict.is_none());
}

#[tokio::test]
async fn a_zero_confirmation_budget_falls_back_to_page_count() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "NoBudget").await;
    let chapter = held_chapter(&svc, manga, "NoBudget", 1.0, 2).await;
    sqlx::query("UPDATE chapters SET source_page_count = 5 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();
    {
        let mut s = svc.settings.write().await;
        s.upgrade_confirm_fetches = 0;
    }

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].kind,
        UpgradeKind::QualityReupload,
        "with no budget to confirm, a longer listing is still the only signal \
         available and must not be silently discarded"
    );
}

#[tokio::test]
async fn a_confirmation_that_cannot_reach_the_source_keeps_the_count_verdict() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Unreachable").await;
    let chapter = held_chapter(&svc, manga, "Unreachable", 1.0, 2).await;
    sqlx::query("UPDATE chapters SET source_page_count = 5 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();
    {
        let mut s = svc.settings.write().await;
        s.upgrade_confirm_fetches = 3;
    }

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert_eq!(
        found.len(),
        1,
        "a source that cannot be reached must not make detection fail or drop \
         the candidate"
    );
    assert_eq!(found[0].kind, UpgradeKind::QualityReupload);
}

#[tokio::test]
async fn a_candidate_names_both_sides_of_the_comparison() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "BothSides").await;
    let held = held_chapter(&svc, manga, "BothSides", 1.0, 3).await;
    sqlx::query("UPDATE chapters SET scanlator = 'Held Group', page_count = 3 WHERE id = ?")
        .bind(held.0)
        .execute(&svc.db)
        .await
        .unwrap();
    let rival = insert_chapter(&svc.db, manga, "rival", 1.0).await;
    sqlx::query("UPDATE chapters SET scanlator = 'Rival Group', page_count = 3 WHERE id = ?")
        .bind(rival)
        .execute(&svc.db)
        .await
        .unwrap();
    svc.set_global_scanlator_pref("Held Group", 1, false)
        .await
        .unwrap();
    svc.set_global_scanlator_pref("Rival Group", 9, false)
        .await
        .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    let c = found
        .iter()
        .find(|c| c.kind == UpgradeKind::PreferredScanlator)
        .expect("expected a scanlator candidate");
    assert_eq!(
        c.held_scanlator.as_deref(),
        Some("Held Group"),
        "a comparison that cannot name what you already hold is not a comparison"
    );
    assert_eq!(c.candidate_scanlator.as_deref(), Some("Rival Group"));
}

async fn set_auto_replace_reasons(svc: &kani_app::service::AppService, reasons: &str) {
    let mut s = svc.settings.write().await;
    s.upgrade_auto_replace_reasons = reasons.to_string();
}

#[tokio::test]
async fn auto_replace_does_nothing_unless_the_series_opts_in() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Manual").await;
    let held = held_chapter(&svc, manga, "Manual", 1.0, 3).await;
    let rival = insert_chapter(&svc.db, manga, "rival-1", 1.0).await;
    sqlx::query("UPDATE chapters SET scanlator = 'Held Group' WHERE id = ?")
        .bind(held.0)
        .execute(&svc.db)
        .await
        .unwrap();
    sqlx::query("UPDATE chapters SET scanlator = 'Rival Group' WHERE id = ?")
        .bind(rival.0)
        .execute(&svc.db)
        .await
        .unwrap();
    svc.set_scanlator_pref(manga, "Rival Group", 10, false)
        .await
        .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert!(
        !found.is_empty(),
        "the candidate itself must still be raised"
    );

    let status: i64 = sqlx::query_scalar("SELECT download_status FROM chapters WHERE id = ?")
        .bind(held.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        status, 2,
        "with auto-replace off the held chapter must be left alone"
    );
}

#[tokio::test]
async fn auto_replace_acts_on_a_preferred_scanlator_when_the_series_opts_in() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Auto").await;
    let held = held_chapter(&svc, manga, "Auto", 1.0, 3).await;
    let rival = insert_chapter(&svc.db, manga, "rival-1", 1.0).await;
    sqlx::query("UPDATE chapters SET scanlator = 'Held Group' WHERE id = ?")
        .bind(held.0)
        .execute(&svc.db)
        .await
        .unwrap();
    sqlx::query("UPDATE chapters SET scanlator = 'Rival Group' WHERE id = ?")
        .bind(rival.0)
        .execute(&svc.db)
        .await
        .unwrap();
    svc.set_scanlator_pref(manga, "Rival Group", 10, false)
        .await
        .unwrap();
    svc.set_upgrade_auto_replace(manga, true).await.unwrap();
    set_auto_replace_reasons(&svc, "preferred_scanlator").await;

    svc.evaluate_upgrades(manga).await.unwrap();

    let status: i64 = sqlx::query_scalar("SELECT download_status FROM chapters WHERE id = ?")
        .bind(held.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_ne!(
        status, 2,
        "the held chapter should no longer count as downloaded — apply_upgrade \
         clears it and re-queues, so this is 0 (pending) or 1 (queued)"
    );

    let library = { svc.settings.read().await.library_path.clone() };
    let replaced = library.join(".replaced");
    let kept = std::fs::read_dir(&replaced).map(|d| d.count()).unwrap_or(0);
    assert_eq!(
        kept, 1,
        "the old file must be preserved, or automatic replacement is destructive"
    );
}

#[tokio::test]
async fn auto_replace_ignores_a_reason_that_is_not_configured() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "NotConfigured").await;
    let held = held_chapter(&svc, manga, "NotConfigured", 1.0, 3).await;
    let rival = insert_chapter(&svc.db, manga, "rival-1", 1.0).await;
    sqlx::query("UPDATE chapters SET scanlator = 'Held Group' WHERE id = ?")
        .bind(held.0)
        .execute(&svc.db)
        .await
        .unwrap();
    sqlx::query("UPDATE chapters SET scanlator = 'Rival Group' WHERE id = ?")
        .bind(rival.0)
        .execute(&svc.db)
        .await
        .unwrap();
    svc.set_scanlator_pref(manga, "Rival Group", 10, false)
        .await
        .unwrap();
    svc.set_upgrade_auto_replace(manga, true).await.unwrap();
    set_auto_replace_reasons(&svc, "resolution,colour").await;

    svc.evaluate_upgrades(manga).await.unwrap();

    let status: i64 = sqlx::query_scalar("SELECT download_status FROM chapters WHERE id = ?")
        .bind(held.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(status, 2, "an unconfigured reason must not rewrite a file");
}

#[tokio::test]
async fn auto_replace_never_acts_on_a_reassurance_entry() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Reassure").await;
    let chapter = held_chapter(&svc, manga, "Reassure", 1.0, 5).await;
    sqlx::query("UPDATE chapters SET source_page_count = 2 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();
    svc.set_upgrade_auto_replace(manga, true).await.unwrap();
    set_auto_replace_reasons(
        &svc,
        "preferred_scanlator,resolution,colour,encoder,bitrate",
    )
    .await;

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert_eq!(found[0].kind, UpgradeKind::SourceDowngraded);

    let status: i64 = sqlx::query_scalar("SELECT download_status FROM chapters WHERE id = ?")
        .bind(chapter.0)
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        status, 2,
        "replacing a better copy with a worse one is the one thing this must never do"
    );
}

#[tokio::test]
async fn the_library_wide_list_hides_reassurance_by_default() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Hidden").await;
    let chapter = held_chapter(&svc, manga, "Hidden", 1.0, 5).await;
    sqlx::query("UPDATE chapters SET source_page_count = 2 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.evaluate_upgrades(manga).await.unwrap();
    assert_eq!(
        pending_badges(&svc, manga).await,
        1,
        "the per-chapter badge keeps it — that is where a downgrade belongs"
    );
    assert!(
        svc.all_upgrades().await.unwrap().is_empty(),
        "the library-wide list is for deciding what to replace, and there is \
         nothing to decide here"
    );

    {
        let mut s = svc.settings.write().await;
        s.upgrade_show_downgrades = true;
    }
    assert_eq!(
        svc.all_upgrades().await.unwrap().len(),
        1,
        "and it comes back when asked for"
    );
}

#[tokio::test]
async fn the_stored_quality_columns_are_populated_by_a_download() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Stored").await;
    let chapter = held_chapter(&svc, manga, "Stored", 1.0, 3).await;

    struct Row {
        quality_long_edge: Option<i64>,
        quality_bytes_per_mp: Option<f64>,
        quality_encoder: Option<i64>,
        quality_colour: Option<String>,
        page_count: Option<i64>,
    }
    let row: Row = sqlx::query_as!(
        Row,
        "SELECT quality_long_edge, quality_bytes_per_mp, quality_encoder, quality_colour, \
         page_count FROM chapters WHERE id = ?",
        chapter
    )
    .fetch_one(&svc.db)
    .await
    .unwrap();

    assert_eq!(
        row.quality_long_edge,
        Some(24),
        "the fixture pages are 16x24"
    );
    assert_eq!(row.page_count, Some(3));
    assert!(row.quality_bytes_per_mp.is_some());
    assert_eq!(
        row.quality_colour.as_deref(),
        Some("monochrome"),
        "colour must be stored, not just computed — the comparator needs it and \
         cannot get it from the older columns"
    );
    assert!(row.quality_encoder.is_none());
}

#[tokio::test]
async fn detection_reads_the_columns_rather_than_the_manifest() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "FastPath").await;
    let chapter = held_chapter(&svc, manga, "FastPath", 1.0, 3).await;
    sqlx::query("UPDATE chapters SET source_page_count = 9 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    sqlx::query("UPDATE chapters SET manifest_json = 'not json at all' WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert_eq!(found.len(), 1);
    let held = found[0]
        .held_score
        .expect("the stored columns must answer without the manifest");
    assert_eq!(held.median_long_edge_px, 24);
    assert_eq!(held.colour, kani_core::quality::ColourProfile::Monochrome);
}

#[tokio::test]
async fn a_row_without_the_columns_falls_back_to_the_manifest() {
    let svc = test_service().await;
    let manga = seed_manga(&svc, "Legacy").await;
    let chapter = held_chapter(&svc, manga, "Legacy", 1.0, 3).await;
    sqlx::query("UPDATE chapters SET source_page_count = 9 WHERE id = ?")
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();

    sqlx::query(
        "UPDATE chapters SET quality_long_edge = NULL, quality_bytes_per_mp = NULL, \
         quality_encoder = NULL, quality_colour = NULL WHERE id = ?",
    )
    .bind(chapter.0)
    .execute(&svc.db)
    .await
    .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0]
            .held_score
            .expect("the manifest is still there and must be used")
            .median_long_edge_px,
        24,
        "a library that predates the columns must keep working"
    );
}
