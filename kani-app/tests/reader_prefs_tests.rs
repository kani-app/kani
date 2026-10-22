#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_manga, insert_source, insert_user, test_service};

#[tokio::test]
async fn set_reader_prefs_persists_and_round_trips() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "alice").await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Manga").await;

    svc.set_reader_prefs(user_id, manga_id, r#"{"mode":"webtoon","bg":"white"}"#)
        .await
        .unwrap();

    let tracking = svc.get_manga_tracking(user_id, manga_id).await.unwrap();
    assert_eq!(
        tracking.reader_prefs.as_deref(),
        Some(r#"{"mode":"webtoon","bg":"white"}"#)
    );
}

#[tokio::test]
async fn set_reader_prefs_rejects_non_object_json() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "bob").await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m2", "Manga2").await;

    let err = svc.set_reader_prefs(user_id, manga_id, r#"[1,2,3]"#).await;
    assert!(err.is_err(), "array JSON should be rejected");

    let err2 = svc.set_reader_prefs(user_id, manga_id, r#""hello""#).await;
    assert!(err2.is_err(), "string JSON should be rejected");
}

#[tokio::test]
async fn set_reader_prefs_upserts_on_existing_tracking_row() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "carol").await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m3", "Manga3").await;

    svc.set_reading_direction(user_id, manga_id, "ltr")
        .await
        .unwrap();

    svc.set_reader_prefs(user_id, manga_id, r#"{"fit":"width"}"#)
        .await
        .unwrap();

    let tracking = svc.get_manga_tracking(user_id, manga_id).await.unwrap();
    assert_eq!(
        tracking.reading_direction, "ltr",
        "direction must be preserved"
    );
    assert_eq!(tracking.reader_prefs.as_deref(), Some(r#"{"fit":"width"}"#));
}

#[tokio::test]
async fn get_manga_tracking_reader_prefs_is_none_for_fresh_row() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "dave").await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m4", "Manga4").await;

    let tracking = svc.get_manga_tracking(user_id, manga_id).await.unwrap();
    assert!(
        tracking.reader_prefs.is_none(),
        "reader_prefs must be None when no prefs have been saved"
    );
}
