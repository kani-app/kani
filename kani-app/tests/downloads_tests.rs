#![allow(clippy::unwrap_used)]
//! Download lifecycle error classification.

mod common;
use common::{insert_chapter, insert_manga, insert_source, test_service};
use kani_app::ServiceError;

#[tokio::test]
async fn download_chapter_already_in_progress_is_conflict() {
    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, source_id, "m1", "Manga").await;
    let chapter_id = insert_chapter(&svc.db, manga_id, "c1", 1.0).await;

    sqlx::query("UPDATE chapters SET download_status = 1 WHERE id = ?")
        .bind(chapter_id)
        .execute(&svc.db)
        .await
        .unwrap();

    let err = svc.download_chapter(chapter_id).await.unwrap_err();
    assert!(
        matches!(err, ServiceError::Conflict(_)),
        "expected Conflict, got {err:?}"
    );
}
