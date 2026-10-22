#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_manga, insert_source, test_service};
use kani_app::ids::MangaId;
use kani_app::service::dedup::{
    find_similar_manga, normalise_title, record_duplicates_for_manga, scan_and_persist_duplicates,
};

#[tokio::test]
async fn normalise_title_strips_leading_the() {
    assert_eq!(normalise_title("The Dragon"), "dragon");
    assert_eq!(normalise_title("A New Hope"), "new hope");
    assert_eq!(normalise_title("An Example"), "example");
}

#[tokio::test]
async fn normalise_title_removes_volume_suffix() {
    assert_eq!(normalise_title("Dragon Ball, Vol. 3"), "dragon ball");
    assert_eq!(normalise_title("Dragon Ball Vol. 3"), "dragon ball");
    assert_eq!(normalise_title("Berserk, Volume 1"), "berserk");
    assert_eq!(normalise_title("Berserk, Ch. 1"), "berserk");
}

#[tokio::test]
async fn normalise_title_collapses_punctuation_and_whitespace() {
    assert_eq!(normalise_title("Dragon-Ball!"), "dragon ball");
    assert_eq!(normalise_title("One.Piece"), "one piece");
    assert_eq!(normalise_title("Attack  on  Titan"), "attack on titan");
}

#[tokio::test]
async fn find_similar_manga_returns_empty_on_fresh_db() {
    let svc = test_service().await;
    let hits = find_similar_manga(&svc.db, "Dragon Ball", &[], None)
        .await
        .unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn find_similar_manga_finds_close_title_match() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let id1 = insert_manga(&svc.db, src, "m1", "Dragon Ball").await;
    let _id2 = insert_manga(&svc.db, src, "m2", "Dragon Ball Z").await;

    let hits = find_similar_manga(&svc.db, "Dragon Ball Z", &[], None)
        .await
        .unwrap();
    assert!(
        !hits.is_empty(),
        "at least 'Dragon Ball' should match 'Dragon Ball Z'"
    );
    let hit_ids: Vec<MangaId> = hits.iter().map(|h| h.id).collect();
    assert!(
        hit_ids.contains(&id1),
        "'Dragon Ball' should be a hit for 'Dragon Ball Z'"
    );
}

#[tokio::test]
async fn find_similar_manga_returns_nothing_for_dissimilar_title() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "One Piece").await;

    let hits = find_similar_manga(&svc.db, "Naruto", &[], None)
        .await
        .unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn find_similar_manga_excludes_the_given_id() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let id1 = insert_manga(&svc.db, src, "m1", "Dragon Ball").await;
    let id2 = insert_manga(&svc.db, src, "m2", "Dragon Ball Z").await;

    let hits = find_similar_manga(&svc.db, "Dragon Ball", &[], Some(id1))
        .await
        .unwrap();
    let hit_ids: Vec<MangaId> = hits.iter().map(|h| h.id).collect();
    assert!(
        !hit_ids.contains(&id1),
        "the excluded manga should not appear in results"
    );
    assert!(hit_ids.contains(&id2));
}

#[tokio::test]
async fn scan_and_persist_duplicates_empty_db_returns_zero() {
    let svc = test_service().await;
    let new_pairs = scan_and_persist_duplicates(&svc.db).await.unwrap();
    assert_eq!(new_pairs, 0);
}

#[tokio::test]
async fn scan_and_persist_duplicates_records_similar_pair() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Dragon Ball").await;
    insert_manga(&svc.db, src, "m2", "Dragon Ball Z").await;

    let new_pairs = scan_and_persist_duplicates(&svc.db).await.unwrap();
    assert!(
        new_pairs >= 1,
        "at least one similar pair should be detected"
    );

    let pair_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM duplicate_pairs")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert!(pair_count >= 1);
}

#[tokio::test]
async fn scan_and_persist_duplicates_is_idempotent() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Dragon Ball").await;
    insert_manga(&svc.db, src, "m2", "Dragon Ball Z").await;

    let first_run = scan_and_persist_duplicates(&svc.db).await.unwrap();
    assert!(first_run >= 1);

    let second_run = scan_and_persist_duplicates(&svc.db).await.unwrap();
    assert_eq!(
        second_run, 0,
        "re-running should not insert duplicate pairs"
    );
}

#[tokio::test]
async fn record_duplicates_for_manga_creates_pair_with_similar_title() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let existing = insert_manga(&svc.db, src, "m1", "Dragon Ball").await;
    let new_id = insert_manga(&svc.db, src, "m2", "Dragon Ball Z").await;

    record_duplicates_for_manga(&svc.db, new_id).await.unwrap();

    let pair_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM duplicate_pairs WHERE manga_a_id = ? OR manga_b_id = ?",
    )
    .bind(existing)
    .bind(new_id)
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert!(pair_count >= 1, "a duplicate pair should be recorded");
}

#[tokio::test]
async fn record_duplicates_for_manga_no_match_inserts_no_pairs() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "One Piece").await;
    let new_id = insert_manga(&svc.db, src, "m2", "Naruto").await;

    record_duplicates_for_manga(&svc.db, new_id).await.unwrap();

    let pair_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM duplicate_pairs")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(pair_count, 0);
}
