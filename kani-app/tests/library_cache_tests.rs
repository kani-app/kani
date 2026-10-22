#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_manga, insert_source, test_service};
use kani_app::ids::UserId;
use kani_app::service::library::LibraryFilter;
use kani_shared::types::MangaSortOrder;

async fn list_names(svc: &kani_app::service::AppService) -> Vec<String> {
    svc.get_library_filtered(
        UserId(1),
        &LibraryFilter {
            page: 1,
            page_size: 20,
            sort_by: MangaSortOrder::NameAsc,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .0
    .into_iter()
    .map(|r| r.name)
    .collect()
}

#[tokio::test]
async fn library_listing_served_from_cache_on_second_call() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Alpha").await;

    let first = list_names(&svc).await;
    assert_eq!(first, vec!["Alpha"]);

    sqlx::query(
        "INSERT INTO manga (source_id, source_manga_id, name, status) VALUES (?, 'm2', 'Beta', 0)",
    )
    .bind(src)
    .execute(&svc.db)
    .await
    .unwrap();

    let second = list_names(&svc).await;
    assert_eq!(
        second,
        vec!["Alpha"],
        "second call should return cached result"
    );
}

#[tokio::test]
async fn invalidate_library_clears_cache_and_emits_sse_event() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let m1 = insert_manga(&svc.db, src, "m1", "Alpha").await;

    let _ = list_names(&svc).await;

    sqlx::query(
        "INSERT INTO manga (source_id, source_manga_id, name, status) VALUES (?, 'm2', 'Beta', 0)",
    )
    .bind(src)
    .execute(&svc.db)
    .await
    .unwrap();

    let mut rx = svc.subscribe_refresh();

    svc.update_local_metadata(
        m1,
        kani_app::models::LocalMetadataUpdate {
            local_name: Some("Alpha Updated".to_string()),
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

    let after = list_names(&svc).await;
    assert!(
        after.contains(&"Alpha Updated".to_string()),
        "updated name must appear"
    );
    assert!(
        after.contains(&"Beta".to_string()),
        "Beta must appear after cache cleared"
    );

    let event = rx
        .try_recv()
        .expect("LibraryInvalidated event should be present");
    assert_eq!(event, kani_app::events::AppEvent::LibraryInvalidated);
}

#[tokio::test]
async fn library_cache_miss_after_ttl() {
    let cache = kani_app::cache::RequestCache::new_with_library_ttl(1);
    let val = std::sync::Arc::new((vec![], false, None::<u32>));
    cache.insert_library_listing(1, 0, 1, 20, val).await;

    assert!(
        cache.get_library_listing(1, 0, 1, 20).await.is_some(),
        "should be cached immediately"
    );

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    assert!(
        cache.get_library_listing(1, 0, 1, 20).await.is_none(),
        "should be evicted after TTL"
    );
}

#[tokio::test]
async fn library_cache_disabled_when_ttl_zero() {
    let cache = kani_app::cache::RequestCache::new_with_library_ttl(0);
    let val = std::sync::Arc::new((vec![], false, None::<u32>));
    cache.insert_library_listing(1, 0, 1, 20, val).await;

    assert!(
        cache.get_library_listing(1, 0, 1, 20).await.is_none(),
        "TTL=0 should disable caching"
    );
}
