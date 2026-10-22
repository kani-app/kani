#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_manga, insert_source, test_service};
use kani_app::ids::UserId;
use kani_app::models::LocalMetadataUpdate;
use kani_app::service::library::LibraryFilter;

async fn search(svc: &kani_app::service::AppService, query: &str) -> Vec<String> {
    let (rows, _, _) = svc
        .get_library_filtered(
            UserId(1),
            &LibraryFilter {
                page: 1,
                page_size: 20,
                search: Some(query.to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    rows.into_iter().map(|r| r.name).collect()
}

#[tokio::test]
async fn fts_partial_title_match() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Dragon Quest").await;
    insert_manga(&svc.db, src, "m2", "Naruto").await;

    let hits = search(&svc, "dragon").await;
    assert_eq!(hits, vec!["Dragon Quest"]);
}

#[tokio::test]
async fn fts_no_match_returns_empty() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "One Piece").await;

    let hits = search(&svc, "naruto").await;
    assert!(hits.is_empty());
}

#[tokio::test]
async fn fts_author_found_after_update_manga_fts() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "One Piece").await;

    let person_id: i64 =
        sqlx::query_scalar("INSERT INTO people (name) VALUES ('Oda Eiichiro') RETURNING id")
            .fetch_one(&svc.db)
            .await
            .unwrap();
    sqlx::query("INSERT INTO manga_people (manga_id, role, person_id) VALUES (?, 'author', ?)")
        .bind(manga_id)
        .bind(person_id)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.update_manga_fts(manga_id).await.unwrap();

    let hits = search(&svc, "oda").await;
    assert_eq!(hits, vec!["One Piece"]);
}

#[tokio::test]
async fn fts_local_author_found_after_update_local_metadata() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Some Manga").await;

    svc.update_local_metadata(
        manga_id,
        LocalMetadataUpdate {
            local_name: None,
            local_description: None,
            local_status: None,
            authors: Some(vec!["Kubo Tite".to_string()]),
            artists: None,
            tags: None,
        },
        UserId(1),
    )
    .await
    .unwrap();

    let hits = search(&svc, "kubo").await;
    assert_eq!(hits, vec!["Some Manga"]);
}

#[tokio::test]
async fn fts_local_name_searchable_after_update_local_metadata() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Bleach").await;

    svc.update_local_metadata(
        manga_id,
        LocalMetadataUpdate {
            local_name: Some("Brilliant Bleach".to_string()),
            local_description: None,
            local_status: None,
            authors: None,
            artists: None,
            tags: None,
        },
        UserId(1),
    )
    .await
    .unwrap();

    let hits = search(&svc, "brilliant").await;
    assert_eq!(hits.len(), 1);
}

#[tokio::test]
async fn fts_delete_manga_removes_from_search() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Fairy Tail").await;

    let before = search(&svc, "fairy").await;
    assert_eq!(before.len(), 1);

    svc.delete_manga(manga_id, UserId(1)).await.unwrap();

    let after = search(&svc, "fairy").await;
    assert!(after.is_empty());
}
