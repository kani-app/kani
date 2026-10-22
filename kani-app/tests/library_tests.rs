#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_manga, insert_source, test_service};
use kani_app::ids::{MangaId, UserId};
use kani_app::service::library::LibraryFilter;
use kani_shared::types::MangaSortOrder;

#[tokio::test]
async fn get_manga_by_id_returns_not_found_for_missing_id() {
    let svc = test_service().await;
    let result = svc.get_manga_by_id(MangaId(99999)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_manga_by_id_returns_manga_after_insert() {
    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, source_id, "m1", "Test Manga").await;

    let manga = svc.get_manga_by_id(manga_id).await.unwrap();
    assert_eq!(manga.name, "Test Manga");
    assert_eq!(manga.id, manga_id);
    assert_eq!(manga.source_id, source_id);
}

#[tokio::test]
async fn get_library_returns_empty_list_on_fresh_db() {
    let svc = test_service().await;
    let list = svc.get_library(1, 0).await.unwrap();
    assert!(list.is_empty());
}

#[tokio::test]
async fn get_library_returns_inserted_manga() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Alpha").await;
    insert_manga(&svc.db, src, "m2", "Beta").await;

    let list = svc.get_library(1, 0).await.unwrap();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn get_library_filtered_empty_db_returns_empty() {
    let svc = test_service().await;
    let (rows, has_next, _total) = svc
        .get_library_filtered(
            UserId(1),
            &LibraryFilter {
                page: 1,
                page_size: 20,
                sort_by: MangaSortOrder::UpdatedAsc,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(rows.is_empty());
    assert!(!has_next);
}

#[tokio::test]
async fn get_library_filtered_returns_matching_manga() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Dragon Quest").await;
    insert_manga(&svc.db, src, "m2", "Naruto").await;

    let (rows, _, _) = svc
        .get_library_filtered(
            UserId(1),
            &LibraryFilter {
                page: 1,
                page_size: 20,
                search: Some("Dragon".to_string()),
                sort_by: MangaSortOrder::UpdatedAsc,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Dragon Quest");
}

#[tokio::test]
async fn a_search_reports_which_field_matched() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;

    let by_title = insert_manga(&svc.db, src, "m1", "Gomyang Chronicles").await;
    let by_author = insert_manga(&svc.db, src, "m2", "Harbour Lights").await;
    let by_desc = insert_manga(&svc.db, src, "m3", "Paper Cranes").await;

    let person: i64 =
        sqlx::query_scalar("INSERT INTO people (name) VALUES ('Gomyang') RETURNING id")
            .fetch_one(&svc.db)
            .await
            .unwrap();
    sqlx::query("INSERT INTO manga_people (manga_id, person_id, role) VALUES (?, ?, 'author')")
        .bind(by_author.0)
        .bind(person)
        .execute(&svc.db)
        .await
        .unwrap();

    // The FTS row is rebuilt from the manga row and its people whenever an
    // indexed column changes, so this also pulls the author in.
    for (id, description) in [
        (by_title, "nothing to see"),
        (by_author, "nothing to see"),
        (by_desc, "a quiet Gomyang afternoon by the river"),
    ] {
        sqlx::query("UPDATE manga SET description = ? WHERE id = ?")
            .bind(description)
            .bind(id.0)
            .execute(&svc.db)
            .await
            .unwrap();
    }

    let (rows, _, _) = svc
        .get_library_filtered(
            UserId(1),
            &LibraryFilter {
                page: 1,
                page_size: 20,
                search: Some("Gomyang".to_string()),
                sort_by: MangaSortOrder::UpdatedAsc,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let field = |id: MangaId| {
        rows.iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("row {id} missing from results"))
    };

    assert_eq!(rows.len(), 3, "all three should match somewhere");
    assert_eq!(
        field(by_title).match_field,
        None,
        "a title hit needs no caption"
    );
    assert_eq!(field(by_author).match_field.as_deref(), Some("author"));
    assert!(
        field(by_author)
            .match_text
            .as_deref()
            .unwrap_or("")
            .contains("Gomyang"),
        "author caption should quote the match: {:?}",
        field(by_author).match_text
    );
    assert_eq!(field(by_desc).match_field.as_deref(), Some("description"));
    assert!(
        field(by_desc)
            .match_text
            .as_deref()
            .unwrap_or("")
            .contains("Gomyang"),
        "description caption should quote the match: {:?}",
        field(by_desc).match_text
    );
}

#[tokio::test]
async fn a_listing_without_a_search_has_no_match_captions() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Alpha").await;

    let (rows, _, _) = svc
        .get_library_filtered(
            UserId(1),
            &LibraryFilter {
                page: 1,
                page_size: 20,
                sort_by: MangaSortOrder::UpdatedAsc,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].match_field, None);
    assert_eq!(rows[0].match_text, None);
}

#[tokio::test]
async fn delete_manga_removes_row() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "To Delete").await;

    svc.delete_manga(manga_id, UserId(1)).await.unwrap();

    let result = svc.get_manga_by_id(manga_id).await;
    assert!(result.is_err(), "manga should be gone after delete");
}

#[tokio::test]
async fn delete_manga_returns_not_found_for_missing_id() {
    let svc = test_service().await;
    let result = svc.delete_manga(MangaId(99999), UserId(1)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn a_cover_whose_directory_is_missing_is_not_found_not_a_server_error() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Gone Cover").await;

    sqlx::query("UPDATE manga SET local_cover_path = 'no-such-dir/cover.jpg' WHERE id = ?")
        .bind(manga_id)
        .execute(&svc.db)
        .await
        .unwrap();

    let err = svc.get_manga_cover_path(manga_id).await.unwrap_err();
    assert!(
        matches!(err, kani_app::ServiceError::NotFound(_)),
        "a missing cover directory must be NotFound (404), not Internal (500) — \
         got {err:?}"
    );
}

async fn seed_continue_reading(svc: &kani_app::service::AppService) -> (UserId, MangaId) {
    let user_id = common::insert_user(&svc.db, "reader").await;
    let src = insert_source(&svc.db, "shelf-src").await;
    let manga_id = insert_manga(&svc.db, src, "shelf-m", "Shelf Manga").await;
    let read = common::insert_chapter(&svc.db, manga_id, "ch1", 1.0).await;
    let unread = common::insert_chapter(&svc.db, manga_id, "ch2", 2.0).await;
    for ch in [read, unread] {
        sqlx::query("UPDATE chapters SET download_status = 2, page_count = 10 WHERE id = ?")
            .bind(ch)
            .execute(&svc.db)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO user_chapter_tracking (user_id, chapter_id, is_read, last_page_read, last_read_at) \
         VALUES (?, ?, 1, 10, '2026-01-01 00:00:00')",
    )
    .bind(user_id)
    .bind(read)
    .execute(&svc.db)
    .await
    .unwrap();
    (user_id, manga_id)
}

#[tokio::test]
async fn continue_reading_shelf_offers_the_next_unread_chapter() {
    let svc = test_service().await;
    let (user_id, manga_id) = seed_continue_reading(&svc).await;

    let shelf = svc.get_continue_reading_shelf(user_id, 12).await.unwrap();

    assert_eq!(shelf.len(), 1);
    assert_eq!(shelf[0].manga_id, manga_id);
    assert_eq!(shelf[0].chapter_number, 2.0);
}

#[tokio::test]
async fn continue_reading_shelf_excludes_trashed_manga() {
    let svc = test_service().await;
    let (user_id, manga_id) = seed_continue_reading(&svc).await;

    sqlx::query("UPDATE manga SET deleted_at = '2026-02-01 00:00:00' WHERE id = ?")
        .bind(manga_id)
        .execute(&svc.db)
        .await
        .unwrap();

    let shelf = svc.get_continue_reading_shelf(user_id, 12).await.unwrap();

    assert!(
        shelf.is_empty(),
        "a trashed manga must not appear on the continue-reading shelf, got {:?}",
        shelf.iter().map(|i| i.manga_id).collect::<Vec<_>>()
    );
}
