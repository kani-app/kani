#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_manga, insert_source, test_service};

#[tokio::test]
async fn list_categories_empty_on_fresh_db() {
    let svc = test_service().await;
    let cats = svc.list_categories().await.unwrap();
    assert!(cats.is_empty());
}

#[tokio::test]
async fn list_categories_returns_created_categories() {
    let svc = test_service().await;
    svc.create_category("Action", 0).await.unwrap();
    svc.create_category("Romance", 1).await.unwrap();

    let cats = svc.list_categories().await.unwrap();
    assert_eq!(cats.len(), 2);
    let names: Vec<&str> = cats.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Action"));
    assert!(names.contains(&"Romance"));
}

#[tokio::test]
async fn create_category_rejects_empty_name() {
    let svc = test_service().await;
    let result = svc.create_category("", 0).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn rename_category_changes_name() {
    let svc = test_service().await;
    let id = svc.create_category("Old Name", 0).await.unwrap();

    svc.rename_category(id, "New Name").await.unwrap();

    let cats = svc.list_categories().await.unwrap();
    assert_eq!(cats[0].name, "New Name");
}

#[tokio::test]
async fn rename_category_rejects_empty_name() {
    let svc = test_service().await;
    let id = svc.create_category("Cat", 0).await.unwrap();
    let result = svc.rename_category(id, "").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn delete_category_removes_it() {
    let svc = test_service().await;
    let id = svc.create_category("Temp", 0).await.unwrap();
    assert_eq!(svc.list_categories().await.unwrap().len(), 1);

    svc.delete_category(id).await.unwrap();
    assert!(svc.list_categories().await.unwrap().is_empty());
}

#[tokio::test]
async fn set_manga_categories_assigns_categories() {
    let svc = test_service().await;
    let cat_a = svc.create_category("Action", 0).await.unwrap();
    let cat_b = svc.create_category("Romance", 1).await.unwrap();
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Test Manga").await;

    svc.set_manga_categories(manga_id, vec![cat_a, cat_b])
        .await
        .unwrap();

    let cats = svc.get_manga_categories(manga_id).await.unwrap();
    assert_eq!(cats.len(), 2);
}

#[tokio::test]
async fn set_manga_categories_replaces_existing() {
    let svc = test_service().await;
    let cat_a = svc.create_category("Action", 0).await.unwrap();
    let cat_b = svc.create_category("Romance", 1).await.unwrap();
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Test Manga").await;

    svc.set_manga_categories(manga_id, vec![cat_a, cat_b])
        .await
        .unwrap();
    svc.set_manga_categories(manga_id, vec![cat_b])
        .await
        .unwrap();

    let cats = svc.get_manga_categories(manga_id).await.unwrap();
    assert_eq!(cats.len(), 1);
    assert_eq!(cats[0].id, cat_b);
}

#[tokio::test]
async fn set_manga_categories_empty_clears_categories() {
    let svc = test_service().await;
    let cat_id = svc.create_category("Reading", 0).await.unwrap();
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Test Manga").await;

    svc.set_manga_categories(manga_id, vec![cat_id])
        .await
        .unwrap();
    svc.set_manga_categories(manga_id, vec![]).await.unwrap();

    let cats = svc.get_manga_categories(manga_id).await.unwrap();
    assert!(cats.is_empty());
}

#[tokio::test]
async fn reorder_categories_updates_sort_order() {
    let svc = test_service().await;
    let id_a = svc.create_category("A", 0).await.unwrap();
    let id_b = svc.create_category("B", 1).await.unwrap();

    svc.reorder_categories(vec![id_b, id_a]).await.unwrap();

    let cats = svc.list_categories().await.unwrap();
    assert_eq!(cats[0].id, id_b);
    assert_eq!(cats[1].id, id_a);
}
