#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_chapter, insert_manga, insert_source, insert_user, test_service};
use kani_app::ids::UserId;

async fn setup_chapter(
    svc: &kani_app::service::AppService,
    page_count: i64,
) -> kani_app::ids::ChapterId {
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    let chapter_id = insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;
    sqlx::query("UPDATE chapters SET page_count = ? WHERE id = ?")
        .bind(page_count)
        .bind(chapter_id)
        .execute(&svc.db)
        .await
        .unwrap();
    chapter_id
}

async fn fetch_progress(
    svc: &kani_app::service::AppService,
    user_id: UserId,
    chapter_id: kani_app::ids::ChapterId,
) -> Option<(i64, bool)> {
    sqlx::query_as(
        "SELECT last_page_read, is_read FROM user_chapter_tracking WHERE user_id = ? AND chapter_id = ?",
    )
    .bind(user_id)
    .bind(chapter_id)
    .fetch_optional(&svc.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn progress_coalesces_to_latest_page() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "alice").await;
    let chapter_id = setup_chapter(&svc, 10).await;

    svc.set_chapter_progress(user_id, chapter_id, 3)
        .await
        .unwrap();
    svc.set_chapter_progress(user_id, chapter_id, 7)
        .await
        .unwrap();
    svc.set_chapter_progress(user_id, chapter_id, 5)
        .await
        .unwrap();

    assert!(
        fetch_progress(&svc, user_id, chapter_id).await.is_none(),
        "should not be in DB before flush"
    );

    svc.flush_progress_buffer().await;

    let (page, is_read) = fetch_progress(&svc, user_id, chapter_id)
        .await
        .expect("row should exist after flush");
    assert_eq!(page, 5, "latest page wins (not max)");
    assert!(!is_read, "page 5 < page_count-1 = 9, not read");
}

#[tokio::test]
async fn flush_sets_is_read_at_end_of_chapter() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "alice").await;
    let chapter_id = setup_chapter(&svc, 10).await;

    svc.set_chapter_progress(user_id, chapter_id, 9)
        .await
        .unwrap();
    svc.flush_progress_buffer().await;

    let (page, is_read) = fetch_progress(&svc, user_id, chapter_id)
        .await
        .expect("row should exist after flush");
    assert_eq!(page, 9);
    assert!(is_read, "page 9 >= page_count-1 = 9, should be read");
}

#[tokio::test]
async fn is_read_sticky_after_backward_flip() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "alice").await;
    let chapter_id = setup_chapter(&svc, 10).await;

    svc.set_chapter_progress(user_id, chapter_id, 9)
        .await
        .unwrap();
    svc.flush_progress_buffer().await;

    svc.set_chapter_progress(user_id, chapter_id, 3)
        .await
        .unwrap();
    svc.flush_progress_buffer().await;

    let (page, is_read) = fetch_progress(&svc, user_id, chapter_id)
        .await
        .expect("row should exist after flush");
    assert_eq!(page, 3, "backward flip updates page");
    assert!(is_read, "once read stays read (OR semantics)");
}

#[tokio::test]
async fn flush_noop_when_buffer_empty() {
    let svc = test_service().await;
    svc.flush_progress_buffer().await;
}

#[tokio::test]
async fn set_read_status_wins_over_buffered_progress() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "alice").await;
    let chapter_id = setup_chapter(&svc, 10).await;

    svc.set_chapter_progress(user_id, chapter_id, 5)
        .await
        .unwrap();
    svc.set_chapter_read_status(user_id, vec![chapter_id], false)
        .await
        .unwrap();
    svc.flush_progress_buffer().await;

    let row = fetch_progress(&svc, user_id, chapter_id).await;
    assert_eq!(
        row,
        Some((0, false)),
        "explicit unread must win over buffered page flip"
    );
}
