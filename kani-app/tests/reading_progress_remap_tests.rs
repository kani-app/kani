#![allow(clippy::unwrap_used)]

//! A re-download that splits a chapter differently moves every saved reading
//! position onto the new page count, rather than leaving readers on a page that
//! no longer holds what they read.

mod common;
use common::{insert_chapter, insert_manga, insert_source, insert_user, test_service};
use kani_app::ids::{ChapterId, MangaId, UserId};
use kani_app::service::AppService;
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
        zip.write_all(&png_bytes((i as u8).wrapping_mul(9)))
            .unwrap();
    }
    zip.finish().unwrap();
}

/// Captures a manifest for `pages`, which is what a download or re-download does.
async fn capture(svc: &AppService, manga: MangaId, chapter: ChapterId, pages: usize) {
    let library = { svc.settings.read().await.library_path.clone() };
    std::fs::create_dir_all(library.join(format!("Remap - {}", manga.0))).unwrap();
    let cbz = svc.chapter_cbz_path(chapter).await.unwrap().path;
    write_cbz(&cbz, pages);
    svc.record_chapter_manifest(chapter, cbz).await;
}

async fn set_progress(svc: &AppService, user: UserId, chapter: ChapterId, page: i64) {
    sqlx::query(
        "INSERT INTO user_chapter_tracking (user_id, chapter_id, is_read, last_page_read) \
         VALUES (?, ?, 0, ?)",
    )
    .bind(user.0)
    .bind(chapter)
    .bind(page)
    .execute(&svc.db)
    .await
    .unwrap();
}

async fn progress(svc: &AppService, user: UserId, chapter: ChapterId) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT last_page_read FROM user_chapter_tracking WHERE user_id = ? AND chapter_id = ?",
    )
    .bind(user.0)
    .bind(chapter)
    .fetch_one(&svc.db)
    .await
    .unwrap()
}

async fn fixture(pages: usize) -> (AppService, MangaId, ChapterId, UserId) {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga = insert_manga(&svc.db, src, "m1", "Remap").await;
    let chapter = insert_chapter(&svc.db, manga, "c1", 1.0).await;
    sqlx::query("UPDATE chapters SET download_status = 2 WHERE id = ?")
        .bind(chapter)
        .execute(&svc.db)
        .await
        .unwrap();
    capture(&svc, manga, chapter, pages).await;
    let user = insert_user(&svc.db, "reader").await;
    (svc, manga, chapter, user)
}

#[tokio::test]
async fn a_position_scales_onto_a_longer_re_upload() {
    let (svc, manga, chapter, user) = fixture(10).await;
    set_progress(&svc, user, chapter, 5).await;

    capture(&svc, manga, chapter, 20).await;

    assert_eq!(
        progress(&svc, user, chapter).await,
        10,
        "halfway through ten pages is halfway through twenty"
    );
}

#[tokio::test]
async fn a_position_past_the_new_end_is_clamped_rather_than_left_dangling() {
    let (svc, manga, chapter, user) = fixture(20).await;
    set_progress(&svc, user, chapter, 19).await;

    capture(&svc, manga, chapter, 5).await;

    let page = progress(&svc, user, chapter).await;
    assert!(
        (0..5).contains(&page),
        "a position must land inside the new page count, got {page}"
    );
    assert_eq!(
        page, 4,
        "the last page of ten maps to the last page of five"
    );
}

#[tokio::test]
async fn an_unchanged_page_count_leaves_every_position_alone() {
    let (svc, manga, chapter, user) = fixture(10).await;
    set_progress(&svc, user, chapter, 7).await;

    capture(&svc, manga, chapter, 10).await;

    assert_eq!(progress(&svc, user, chapter).await, 7);
}

#[tokio::test]
async fn every_reader_of_the_chapter_is_remapped() {
    let (svc, manga, chapter, first) = fixture(10).await;
    let second = insert_user(&svc.db, "other").await;
    set_progress(&svc, first, chapter, 2).await;
    set_progress(&svc, second, chapter, 8).await;

    capture(&svc, manga, chapter, 5).await;

    assert_eq!(progress(&svc, first, chapter).await, 1);
    assert_eq!(progress(&svc, second, chapter).await, 4);
}

#[tokio::test]
async fn a_reader_who_never_started_is_not_given_a_position() {
    let (svc, manga, chapter, user) = fixture(10).await;
    set_progress(&svc, user, chapter, 0).await;

    capture(&svc, manga, chapter, 40).await;

    assert_eq!(
        progress(&svc, user, chapter).await,
        0,
        "page zero means unstarted and must stay there"
    );
}
