#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_chapter, insert_manga, insert_source, insert_user, test_service};
use kani_app::ids::ChapterId;
use kani_app::service::AppService;
use std::io::Write;

fn make_png(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let img = image::ImageBuffer::from_pixel(w, h, image::Rgb([r, g, b]));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
}

fn write_cbz(path: &std::path::Path, pages: &[Vec<u8>]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    for (i, data) in pages.iter().enumerate() {
        w.start_file(format!("{:04}.png", i + 1), opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap();
}

/// Seeds a downloaded chapter backed by a real fixture CBZ (three distinct pages).
/// Returns the chapter id and the raw bytes of each page (as written into the CBZ).
async fn seed_downloaded_chapter(svc: &AppService) -> (ChapterId, Vec<Vec<u8>>) {
    let src = insert_source(&svc.db, "src").await;
    let manga = insert_manga(&svc.db, src, "m1", "Test Manga").await;
    let ch = insert_chapter(&svc.db, manga, "c1", 1.0).await;
    sqlx::query("UPDATE chapters SET download_status = 2 WHERE id = ?")
        .bind(ch)
        .execute(&svc.db)
        .await
        .unwrap();

    let library_path = svc.settings.read().await.library_path.clone();
    let manga_dir = library_path.join(format!(
        "{} - {}",
        kani_core::utilities::sanitize_filename("Test Manga"),
        manga.0
    ));
    std::fs::create_dir_all(&manga_dir).unwrap();

    let pages = vec![
        make_png(4, 6, 10, 20, 30),
        make_png(4, 8, 40, 50, 60),
        make_png(400, 300, 70, 80, 90),
    ];
    let info = svc.chapter_cbz_path(ch).await.unwrap();
    write_cbz(&info.path, &pages);
    (ch, pages)
}

#[tokio::test]
async fn chapter_feed_advertises_pse() {
    let svc = test_service().await;
    let (ch, _pages) = seed_downloaded_chapter(&svc).await;
    let user = insert_user(&svc.db, "alice").await;

    let feed = svc
        .opds_chapter_feed(ch, user, "http://host")
        .await
        .unwrap();

    assert!(feed.contains(r#"xmlns:pse="http://vaemendis.net/opds-pse/2017""#));
    assert!(feed.contains(r#"pse:count="3""#));
    assert!(
        feed.contains("page?page={pageNumber}"),
        "stream href must keep the literal placeholder: {feed}"
    );
    assert!(feed.contains(&format!("/opds/chapters/{}/file", ch.0)));
}

#[tokio::test]
async fn chapter_feed_reflects_progress() {
    let svc = test_service().await;
    let (ch, _pages) = seed_downloaded_chapter(&svc).await;
    let user = insert_user(&svc.db, "bob").await;

    let feed = svc
        .opds_chapter_feed(ch, user, "http://host")
        .await
        .unwrap();
    assert!(!feed.contains("pse:lastRead"));

    svc.set_chapter_progress(user, ch, 1).await.unwrap();
    svc.flush_progress_buffer().await;

    let feed = svc
        .opds_chapter_feed(ch, user, "http://host")
        .await
        .unwrap();
    assert!(feed.contains(r#"pse:lastRead="2""#), "feed: {feed}");
    assert!(feed.contains("pse:lastReadDate="));
}

#[tokio::test]
async fn page_one_is_the_first_page() {
    let svc = test_service().await;
    let (ch, pages) = seed_downloaded_chapter(&svc).await;

    let (bytes, ct) = svc.opds_chapter_page(ch, 1, 0, None).await.unwrap();
    assert_eq!(bytes, pages[0], "page=1 must serve the first page");
    assert_eq!(ct, "image/png");
}

#[tokio::test]
async fn page_two_is_the_second_page() {
    let svc = test_service().await;
    let (ch, pages) = seed_downloaded_chapter(&svc).await;

    let (bytes, _) = svc.opds_chapter_page(ch, 2, 0, None).await.unwrap();
    assert_eq!(bytes, pages[1]);
}

#[tokio::test]
async fn the_last_page_by_pse_count_is_reachable() {
    let svc = test_service().await;
    let (ch, pages) = seed_downloaded_chapter(&svc).await;

    let (bytes, _) = svc
        .opds_chapter_page(ch, pages.len(), 0, None)
        .await
        .unwrap();
    assert_eq!(bytes, pages[pages.len() - 1]);
}

#[tokio::test]
async fn page_zero_is_rejected_when_one_based() {
    let svc = test_service().await;
    let (ch, _pages) = seed_downloaded_chapter(&svc).await;

    let err = svc.opds_chapter_page(ch, 0, 0, None).await.unwrap_err();
    assert!(
        matches!(err, kani_app::error::ServiceError::Validation(_)),
        "page 0 under 1-based numbering is a bad request, not a 404: {err:?}"
    );
}

#[tokio::test]
async fn zero_based_mode_restores_the_old_indexing() {
    let svc = test_service().await;
    let (ch, pages) = seed_downloaded_chapter(&svc).await;
    svc.settings.write().await.opds_page_index_zero_based = true;

    let (bytes, _) = svc.opds_chapter_page(ch, 0, 0, None).await.unwrap();
    assert_eq!(bytes, pages[0], "page=0 is the first page in 0-based mode");

    let (bytes, _) = svc.opds_chapter_page(ch, 1, 0, None).await.unwrap();
    assert_eq!(bytes, pages[1]);
}

#[tokio::test]
async fn zero_based_mode_reports_last_read_unshifted() {
    let svc = test_service().await;
    let (ch, _pages) = seed_downloaded_chapter(&svc).await;
    let user = insert_user(&svc.db, "zb").await;
    svc.settings.write().await.opds_page_index_zero_based = true;

    svc.set_chapter_progress(user, ch, 1).await.unwrap();
    svc.flush_progress_buffer().await;

    let feed = svc
        .opds_chapter_feed(ch, user, "http://host")
        .await
        .unwrap();
    assert!(feed.contains(r#"pse:lastRead="1""#), "feed: {feed}");
}

#[tokio::test]
async fn chapter_page_out_of_range_is_not_found() {
    let svc = test_service().await;
    let (ch, _pages) = seed_downloaded_chapter(&svc).await;

    let err = svc.opds_chapter_page(ch, 99, 0, None).await.unwrap_err();
    assert!(matches!(err, kani_app::error::ServiceError::NotFound(_)));
}

#[tokio::test]
async fn feed_on_non_downloaded_chapter_is_not_found() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga = insert_manga(&svc.db, src, "m1", "Test Manga").await;
    let ch = insert_chapter(&svc.db, manga, "c1", 1.0).await;
    let user = insert_user(&svc.db, "carol").await;

    let err = svc
        .opds_chapter_feed(ch, user, "http://host")
        .await
        .unwrap_err();
    assert!(matches!(err, kani_app::error::ServiceError::NotFound(_)));
}

#[tokio::test]
async fn chapter_page_transcode_downscales() {
    let svc = test_service().await;
    let (ch, _pages) = seed_downloaded_chapter(&svc).await;

    let (bytes, ct) = svc.opds_chapter_page(ch, 3, 2, None).await.unwrap();
    assert_eq!(ct, "image/jpeg");
    let out = image::load_from_memory(&bytes).unwrap();
    assert!(out.width() <= 2, "width should be clamped: {}", out.width());
}

#[tokio::test]
async fn chapter_page_width_above_clamp_does_not_upscale() {
    let svc = test_service().await;
    let (ch, _pages) = seed_downloaded_chapter(&svc).await;

    let (bytes, _ct) = svc.opds_chapter_page(ch, 3, 100_000, None).await.unwrap();
    let out = image::load_from_memory(&bytes).unwrap();
    assert_eq!(out.width(), 400);
}

#[tokio::test]
async fn cbz_page_index_is_cached() {
    let svc = test_service().await;
    let (ch, _pages) = seed_downloaded_chapter(&svc).await;
    let info = svc.chapter_cbz_path(ch).await.unwrap();

    let a = svc.cbz_page_index(ch, &info.path).await.unwrap();
    let b = svc.cbz_page_index(ch, &info.path).await.unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&a, &b),
        "second call should hit cache"
    );
    assert_eq!(a.len(), 3);
}
