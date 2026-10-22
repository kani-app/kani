#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_chapter, insert_manga, insert_source, test_service};
use kani_app::ids::UserId;
use kani_shared::types::ChapterSortOrder;

#[tokio::test]
async fn get_local_chapters_empty_returns_empty_list() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;

    let (chapters, has_next, _, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            ChapterSortOrder::ChapterDesc,
            UserId(1),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(chapters.is_empty());
    assert!(!has_next);
}

#[tokio::test]
async fn get_local_chapters_returns_inserted_chapters() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;
    insert_chapter(&svc.db, manga_id, "ch2", 2.0).await;

    let (chapters, _, _, total) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            ChapterSortOrder::ChapterDesc,
            UserId(1),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(chapters.len(), 2);
    assert_eq!(total, 2);
    assert!((chapters[0].number - 2.0).abs() < f64::EPSILON);
    assert!((chapters[1].number - 1.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn get_local_chapters_paging_works() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    for i in 1..=5u32 {
        insert_chapter(&svc.db, manga_id, &format!("ch{i}"), f64::from(i)).await;
    }

    let (page1, has_next1, total1, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            3,
            ChapterSortOrder::ChapterAsc,
            UserId(1),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(page1.len(), 3);
    assert!(has_next1);
    assert_eq!(total1, Some(2));

    let (page2, has_next2, _, _) = svc
        .get_local_chapters(
            manga_id,
            2,
            3,
            ChapterSortOrder::ChapterAsc,
            UserId(1),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(page2.len(), 2);
    assert!(!has_next2);
}

#[tokio::test]
async fn download_status_filter_works() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    let ch_id = insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;

    sqlx::query("UPDATE chapters SET download_status = 2 WHERE id = ?")
        .bind(ch_id)
        .execute(&svc.db)
        .await
        .unwrap();

    let (downloaded, _, _, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            ChapterSortOrder::ChapterDesc,
            UserId(1),
            Some(true),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(downloaded.len(), 1);

    let (not_downloaded, _, _, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            ChapterSortOrder::ChapterDesc,
            UserId(1),
            Some(false),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(not_downloaded.is_empty());
}

#[tokio::test]
async fn orphaned_chapters_are_excluded() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    let ch_id = insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;

    sqlx::query("UPDATE chapters SET is_orphaned = TRUE WHERE id = ?")
        .bind(ch_id)
        .execute(&svc.db)
        .await
        .unwrap();

    let (chapters, _, _, total) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            ChapterSortOrder::ChapterDesc,
            UserId(1),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(chapters.is_empty(), "orphaned chapters should not appear");
    assert_eq!(
        total, 0,
        "the count must agree with the rows — a total that includes hidden \
         orphans inflates the pagination"
    );
}

/// Marks a chapter as kept-from-a-previous-source, the way a migration does.
async fn orphan_chapter(db: &sqlx::SqlitePool, chapter_id: kani_app::ids::ChapterId) {
    sqlx::query("UPDATE chapters SET is_orphaned = 1 WHERE id = ?")
        .bind(chapter_id)
        .execute(db)
        .await
        .unwrap();
}

#[tokio::test]
async fn the_orphaned_filter_returns_only_the_kept_chapters() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;
    let kept = insert_chapter(&svc.db, manga_id, "ch2", 2.0).await;
    orphan_chapter(&svc.db, kept).await;

    let (chapters, _, _, total) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            ChapterSortOrder::ChapterDesc,
            UserId(1),
            None,
            None,
            None,
            Some(true),
        )
        .await
        .unwrap();

    assert_eq!(chapters.len(), 1);
    assert!((chapters[0].number - 2.0).abs() < f64::EPSILON);
    assert!(
        chapters[0].is_orphaned,
        "the flag has to reach the client — the row renders its badge from it"
    );
    assert_eq!(total, 1);
}

#[tokio::test]
async fn an_orphan_and_a_live_chapter_can_share_a_number() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    let old = insert_chapter(&svc.db, manga_id, "old-ch5", 5.0).await;
    insert_chapter(&svc.db, manga_id, "new-ch5", 5.0).await;
    orphan_chapter(&svc.db, old).await;

    let (live, _, _, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            ChapterSortOrder::ChapterDesc,
            UserId(1),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let (orphans, _, _, _) = svc
        .get_local_chapters(
            manga_id,
            1,
            20,
            ChapterSortOrder::ChapterDesc,
            UserId(1),
            None,
            None,
            None,
            Some(true),
        )
        .await
        .unwrap();

    assert_eq!(live.len(), 1, "the live listing shows the target's chapter");
    assert_eq!(orphans.len(), 1, "the kept copy is still reachable");
    assert!(!live[0].is_orphaned);
    assert!(orphans[0].is_orphaned);
    assert_ne!(
        live[0].id, orphans[0].id,
        "two distinct rows at the same chapter number"
    );
}

#[tokio::test]
async fn chapter_ids_expose_orphans_only_on_request() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;
    let live = insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;
    let kept = insert_chapter(&svc.db, manga_id, "ch2", 2.0).await;
    orphan_chapter(&svc.db, kept).await;

    let default_ids = svc
        .get_chapter_ids(
            manga_id,
            UserId(1),
            ChapterSortOrder::ChapterDesc,
            None,
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        default_ids,
        vec![live],
        "bulk actions must not reach orphans"
    );

    let orphan_ids = svc
        .get_chapter_ids(
            manga_id,
            UserId(1),
            ChapterSortOrder::ChapterDesc,
            None,
            None,
            None,
            false,
            Some(true),
        )
        .await
        .unwrap();
    assert_eq!(
        orphan_ids,
        vec![kept],
        "selecting orphans for deletion needs exactly them"
    );
}

#[tokio::test]
async fn chapter_ids_are_scoped_to_the_manga_not_the_user() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let first = insert_manga(&svc.db, src, "m1", "First").await;
    let second = insert_manga(&svc.db, src, "m2", "Second").await;
    insert_chapter(&svc.db, first, "a1", 1.0).await;
    insert_chapter(&svc.db, first, "a2", 2.0).await;
    insert_chapter(&svc.db, second, "b1", 1.0).await;

    let ids = svc
        .get_chapter_ids(
            second,
            UserId(7),
            ChapterSortOrder::ChapterDesc,
            None,
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        ids.len(),
        1,
        "no scanlator filter must still return the manga's chapters"
    );
}
