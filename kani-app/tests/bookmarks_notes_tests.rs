#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_chapter, insert_manga, insert_source, insert_user, test_service};

#[tokio::test]
async fn bookmark_toggle_adds_and_removes() {
    let svc = test_service().await;
    let uid = insert_user(&svc.db, "alice").await;
    let src = insert_source(&svc.db, "src").await;
    let mid = insert_manga(&svc.db, src, "m1", "Manga").await;
    let cid = insert_chapter(&svc.db, mid, "c1", 1.0).await;

    let added = svc.toggle_bookmark(uid, cid, 5).await.unwrap();
    assert!(added, "first toggle should add bookmark");

    let pages = svc.get_bookmarks(uid, cid).await.unwrap();
    assert_eq!(pages, vec![5]);

    let removed = svc.toggle_bookmark(uid, cid, 5).await.unwrap();
    assert!(!removed, "second toggle should remove bookmark");

    let pages = svc.get_bookmarks(uid, cid).await.unwrap();
    assert!(pages.is_empty());
}

#[tokio::test]
async fn get_bookmarks_returns_sorted_pages() {
    let svc = test_service().await;
    let uid = insert_user(&svc.db, "bob").await;
    let src = insert_source(&svc.db, "src").await;
    let mid = insert_manga(&svc.db, src, "m2", "Manga").await;
    let cid = insert_chapter(&svc.db, mid, "c2", 1.0).await;

    for pg in [10i64, 2, 7] {
        svc.toggle_bookmark(uid, cid, pg).await.unwrap();
    }
    let pages = svc.get_bookmarks(uid, cid).await.unwrap();
    assert_eq!(pages, vec![2, 7, 10]);
}

#[tokio::test]
async fn chapter_note_round_trips() {
    let svc = test_service().await;
    let uid = insert_user(&svc.db, "carol").await;
    let src = insert_source(&svc.db, "src").await;
    let mid = insert_manga(&svc.db, src, "m3", "Manga").await;
    let cid = insert_chapter(&svc.db, mid, "c3", 1.0).await;

    let empty = svc.get_chapter_note(uid, cid).await.unwrap();
    assert!(empty.is_none(), "note should be None before any save");

    svc.set_chapter_note(uid, cid, "great chapter")
        .await
        .unwrap();
    let note = svc.get_chapter_note(uid, cid).await.unwrap();
    assert_eq!(note.as_deref(), Some("great chapter"));

    svc.set_chapter_note(uid, cid, "updated").await.unwrap();
    let note2 = svc.get_chapter_note(uid, cid).await.unwrap();
    assert_eq!(note2.as_deref(), Some("updated"));
}

#[tokio::test]
async fn get_noted_chapter_ids_excludes_empty_notes() {
    let svc = test_service().await;
    let uid = insert_user(&svc.db, "dave").await;
    let src = insert_source(&svc.db, "src").await;
    let mid = insert_manga(&svc.db, src, "m4", "Manga").await;
    let c1 = insert_chapter(&svc.db, mid, "c4a", 1.0).await;
    let c2 = insert_chapter(&svc.db, mid, "c4b", 2.0).await;

    svc.set_chapter_note(uid, c1, "note here").await.unwrap();
    svc.set_chapter_note(uid, c2, "").await.unwrap();

    let noted = svc
        .get_manga_chapter_notes_with_text(uid, mid)
        .await
        .unwrap();
    let ids: Vec<_> = noted.into_iter().map(|(id, _, _)| id).collect();
    assert_eq!(ids, vec![c1], "an empty note must not count as a note");
}

#[tokio::test]
async fn get_manga_chapter_notes_with_text_happy_path() {
    let svc = test_service().await;
    let uid = insert_user(&svc.db, "eve").await;
    let src = insert_source(&svc.db, "src").await;
    let mid = insert_manga(&svc.db, src, "m5", "Manga").await;
    let c1 = insert_chapter(&svc.db, mid, "c5a", 1.0).await;
    let c2 = insert_chapter(&svc.db, mid, "c5b", 2.0).await;

    svc.set_chapter_note(uid, c1, "first note").await.unwrap();
    svc.set_chapter_note(uid, c2, "second note").await.unwrap();

    let notes = svc
        .get_manga_chapter_notes_with_text(uid, mid)
        .await
        .unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].0, c1);
    assert_eq!(notes[0].2, "first note");
    assert_eq!(notes[1].0, c2);
    assert_eq!(notes[1].2, "second note");
}

#[tokio::test]
async fn get_manga_chapter_notes_with_text_excludes_empty() {
    let svc = test_service().await;
    let uid = insert_user(&svc.db, "frank").await;
    let src = insert_source(&svc.db, "src").await;
    let mid = insert_manga(&svc.db, src, "m6", "Manga").await;
    let c1 = insert_chapter(&svc.db, mid, "c6a", 1.0).await;
    let c2 = insert_chapter(&svc.db, mid, "c6b", 2.0).await;

    svc.set_chapter_note(uid, c1, "has a note").await.unwrap();
    svc.set_chapter_note(uid, c2, "").await.unwrap();

    let notes = svc
        .get_manga_chapter_notes_with_text(uid, mid)
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].0, c1);
}
