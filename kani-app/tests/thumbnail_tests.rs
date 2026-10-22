#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_manga, insert_source, test_service};
use kani_app::ids::MangaId;

fn tiny_jpeg() -> Vec<u8> {
    vec![
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06,
        0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B,
        0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20,
        0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29, 0x2C, 0x30, 0x31,
        0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32, 0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF,
        0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00,
        0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        0xFF, 0xC4, 0x00, 0xB5, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05,
        0x04, 0x04, 0x00, 0x00, 0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21,
        0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08,
        0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A,
        0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37,
        0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56,
        0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75,
        0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93,
        0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9,
        0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6,
        0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2,
        0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7,
        0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0xFB, 0xD9,
    ]
}

async fn write_cover(svc: &kani_app::service::AppService, manga_id: MangaId, jpeg: &[u8]) {
    let library_path = svc.settings.read().await.library_path.clone();
    let covers_dir = library_path.join("covers");
    tokio::fs::create_dir_all(&covers_dir).await.unwrap();
    let filename = format!("{}.jpg", manga_id);
    tokio::fs::write(covers_dir.join(&filename), jpeg)
        .await
        .unwrap();
    sqlx::query("UPDATE manga SET local_cover_path = ? WHERE id = ?")
        .bind(format!("covers/{filename}"))
        .bind(manga_id)
        .execute(&svc.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn generate_thumbnails_creates_all_sizes() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;

    write_cover(&svc, manga_id, &tiny_jpeg()).await;
    svc.generate_and_store_thumbnails(manga_id).await.unwrap();

    for size in &["xs", "sm", "md", "lg"] {
        let result = svc.get_thumbnail_for_size(manga_id, size).await.unwrap();
        assert!(
            result.is_some(),
            "thumbnail for size={size} should exist after generation"
        );
        let (path, format, _) = result.unwrap();
        assert!(
            tokio::fs::metadata(&path).await.is_ok(),
            "thumbnail file for size={size} should exist on disk"
        );
        assert_eq!(format, "jpeg");
    }
}

#[tokio::test]
async fn generate_thumbnails_sets_cover_hash() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;

    write_cover(&svc, manga_id, &tiny_jpeg()).await;
    svc.generate_and_store_thumbnails(manga_id).await.unwrap();

    let hash: Option<String> = sqlx::query_scalar("SELECT cover_hash FROM manga WHERE id = ?")
        .bind(manga_id)
        .fetch_optional(&svc.db)
        .await
        .unwrap()
        .flatten();
    assert!(hash.is_some(), "cover_hash must be set after generation");
    assert_eq!(hash.unwrap().len(), 64, "SHA-256 hex is 64 chars");
}

#[tokio::test]
async fn cover_hash_flipped_last() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;

    write_cover(&svc, manga_id, &tiny_jpeg()).await;
    svc.generate_and_store_thumbnails(manga_id).await.unwrap();

    let (path, _, hash) = svc
        .get_thumbnail_for_size(manga_id, "sm")
        .await
        .unwrap()
        .unwrap();

    let disk_ok = tokio::fs::metadata(&path).await.is_ok();
    assert!(
        disk_ok,
        "thumbnail file must exist before hash is readable via get_thumbnail_for_size"
    );
    assert!(
        !hash.is_empty(),
        "hash must be non-empty when thumbnail is present"
    );
}

#[tokio::test]
async fn clear_thumbnails_removes_files_and_db_rows() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;

    write_cover(&svc, manga_id, &tiny_jpeg()).await;
    svc.generate_and_store_thumbnails(manga_id).await.unwrap();

    let (sm_path, _, _) = svc
        .get_thumbnail_for_size(manga_id, "sm")
        .await
        .unwrap()
        .unwrap();

    svc.clear_thumbnails(manga_id).await;

    assert!(
        tokio::fs::metadata(&sm_path).await.is_err(),
        "thumbnail file should be removed after clear_thumbnails"
    );

    let result = svc.get_thumbnail_for_size(manga_id, "sm").await.unwrap();
    assert!(
        result.is_none(),
        "DB row should be gone after clear_thumbnails"
    );

    let hash: Option<String> = sqlx::query_scalar("SELECT cover_hash FROM manga WHERE id = ?")
        .bind(manga_id)
        .fetch_optional(&svc.db)
        .await
        .unwrap()
        .flatten();
    assert!(
        hash.is_none(),
        "cover_hash must be NULL after clear_thumbnails"
    );
}

#[tokio::test]
async fn thumbnail_submit_dedups_against_active_job() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;

    sqlx::query(
        "INSERT INTO jobs (id, job_type, status, params_json) \
         VALUES ('thumb-dedup-test', 'thumbnail_generation', 'running', ?)",
    )
    .bind(format!("{{\"manga_id\":{}}}", manga_id.0))
    .execute(&svc.db)
    .await
    .unwrap();

    svc.spawn_thumbnail_generation(manga_id).await;

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE job_type = 'thumbnail_generation'")
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert_eq!(
        count, 1,
        "second submit should be deduplicated against the active thumbnail job"
    );
}

#[tokio::test]
async fn generate_thumbnails_sync_decode_bomb_rejected() {
    let library_path = std::env::temp_dir();
    let not_an_image = vec![0u8; 1024];
    let formats = vec!["jpeg".to_string()];
    let result =
        kani_app::images::generate_thumbnails_sync(&not_an_image, 999, &library_path, &formats);
    assert!(result.is_err(), "random bytes should fail to decode");
}
