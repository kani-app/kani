#![allow(clippy::unwrap_used)]
//! Verifies the migration set applies cleanly from an empty schema and that
//! re-running it is a no-op (idempotent).

use sqlx::sqlite::SqlitePoolOptions;

/// A single-connection in-memory pool, so repeated `migrate` calls hit the same
/// database (a fresh connection would get its own empty `:memory:` db).
async fn empty_pool() -> sqlx::SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap()
}

#[tokio::test]
async fn all_migrations_apply_to_empty_schema() {
    let pool = empty_pool().await;
    sqlx::migrate!("../migrations").run(&pool).await.unwrap();

    let manga_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'manga'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(manga_tables, 1, "expected the `manga` table to exist");
}

#[tokio::test]
async fn migrations_are_idempotent_on_rerun() {
    let pool = empty_pool().await;
    sqlx::migrate!("../migrations").run(&pool).await.unwrap();
    sqlx::migrate!("../migrations").run(&pool).await.unwrap();
}
