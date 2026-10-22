#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_chapter, insert_manga, insert_source, insert_user, test_service};

#[tokio::test]
async fn set_chapter_progress_creates_tracking_row() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "alice").await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    let ch_id = insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;

    svc.set_chapter_progress(user_id, ch_id, 5).await.unwrap();
    svc.flush_progress_buffer().await;

    let (chapters, _, _, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            kani_shared::types::ChapterSortOrder::ChapterDesc,
            user_id,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(chapters.len(), 1);
    assert_eq!(chapters[0].last_page_read, Some(5));
}

#[tokio::test]
async fn set_chapter_progress_higher_page_marks_read() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "bob").await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    let ch_id = insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;

    sqlx::query("UPDATE chapters SET page_count = 10 WHERE id = ?")
        .bind(ch_id)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.set_chapter_progress(user_id, ch_id, 9).await.unwrap();
    svc.flush_progress_buffer().await;

    let (chapters, _, _, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            kani_shared::types::ChapterSortOrder::ChapterDesc,
            user_id,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(
        chapters[0].is_read,
        "chapter at last page should be marked read"
    );
}

#[tokio::test]
async fn set_chapter_read_status_marks_chapters_as_read() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "carol").await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    let ch1 = insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;
    let ch2 = insert_chapter(&svc.db, manga_id, "ch2", 2.0).await;

    svc.set_chapter_read_status(user_id, vec![ch1, ch2], true)
        .await
        .unwrap();

    let (chapters, _, _, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            kani_shared::types::ChapterSortOrder::ChapterDesc,
            user_id,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(chapters.iter().all(|c| c.is_read));
}

#[tokio::test]
async fn set_chapter_read_status_can_mark_as_unread() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "dave").await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    let ch_id = insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;

    svc.set_chapter_read_status(user_id, vec![ch_id], true)
        .await
        .unwrap();
    svc.set_chapter_read_status(user_id, vec![ch_id], false)
        .await
        .unwrap();

    let (chapters, _, _, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            kani_shared::types::ChapterSortOrder::ChapterDesc,
            user_id,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(!chapters[0].is_read);
}

#[tokio::test]
async fn filter_unread_only_returns_unread_chapters() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "eve").await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    let ch1 = insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;
    insert_chapter(&svc.db, manga_id, "ch2", 2.0).await;

    svc.set_chapter_read_status(user_id, vec![ch1], true)
        .await
        .unwrap();

    let (unread, _, _, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            kani_shared::types::ChapterSortOrder::ChapterDesc,
            user_id,
            None,
            Some(true),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(unread.len(), 1, "only the unread chapter should appear");
    assert!((unread[0].number - 2.0).abs() < f64::EPSILON);
}
