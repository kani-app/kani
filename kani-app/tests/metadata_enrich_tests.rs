#![allow(clippy::unwrap_used)]

//! Metadata-provider enrichment fills gaps from a
//! provider; it must never overwrite what the user typed, and a provider that
//! fails must leave the manga exactly as it was.

mod common;
use common::{insert_manga, insert_source, insert_user, test_service};

async fn description(
    svc: &kani_app::service::AppService,
    id: kani_app::ids::MangaId,
) -> Option<String> {
    sqlx::query_scalar("SELECT description FROM manga WHERE id = ?")
        .bind(id.0)
        .fetch_one(&svc.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_metadata_provider_enrichment_preserves_local_overrides() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let source = insert_source(&svc.db, "src").await;
    let manga = insert_manga(&svc.db, source, "m1", "Enriched").await;

    sqlx::query("UPDATE manga SET description = ?, local_description = ? WHERE id = ?")
        .bind("mine, hands off")
        .bind("mine, hands off")
        .bind(manga.0)
        .execute(&svc.db)
        .await
        .unwrap();

    let result = svc
        .enrich_manga_metadata(manga, "stub", user)
        .await
        .unwrap();

    assert_eq!(
        description(&svc, manga).await.as_deref(),
        Some("mine, hands off"),
        "the user's own description survives enrichment"
    );
    assert!(
        !result.fields_updated.iter().any(|f| f == "description"),
        "and enrichment does not claim to have updated it: {:?}",
        result.fields_updated
    );
}

#[tokio::test]
async fn enrichment_fills_a_description_that_has_no_local_override() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let source = insert_source(&svc.db, "src").await;
    let manga = insert_manga(&svc.db, source, "m1", "Enriched").await;

    let result = svc
        .enrich_manga_metadata(manga, "stub", user)
        .await
        .unwrap();

    assert!(
        result.fields_updated.iter().any(|f| f == "description"),
        "enrichment fills an empty description: {:?}",
        result.fields_updated
    );
    assert!(
        description(&svc, manga)
            .await
            .is_some_and(|d| d.contains("Enriched")),
        "and the fetched value is stored"
    );
}

#[tokio::test]
async fn a_metadata_provider_failure_leaves_the_manga_unchanged() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    let source = insert_source(&svc.db, "src").await;
    let manga = insert_manga(&svc.db, source, "m1", "Untouched").await;

    let before = description(&svc, manga).await;

    let res = svc
        .enrich_manga_metadata(manga, "no-such-provider", user)
        .await;
    assert!(res.is_err(), "an unknown provider is an error");

    assert_eq!(
        description(&svc, manga).await,
        before,
        "a failed enrichment writes nothing"
    );
    let audits: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = 'manga.enrich_metadata'")
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert_eq!(audits, 0, "and records no enrichment in the audit log");
}
