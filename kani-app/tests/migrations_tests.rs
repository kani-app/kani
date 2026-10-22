#![allow(clippy::unwrap_used)]

use sqlx::SqlitePool;

async fn fresh_pool() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("../migrations").run(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn all_migrations_apply_to_an_empty_database() {
    let pool = fresh_pool().await;

    let tables: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert!(
        tables > 10,
        "expected a populated schema after migrating, found {tables} tables"
    );
}

#[tokio::test]
async fn migrations_are_idempotent_across_a_second_run() {
    let pool = fresh_pool().await;

    sqlx::migrate!("../migrations").run(&pool).await.unwrap();
}

#[tokio::test]
async fn settings_singleton_exists_with_recent_columns() {
    let pool = fresh_pool().await;

    let row: (bool, i64) = sqlx::query_as(
        "SELECT update_check_enabled, v8_idle_timeout_s \
         FROM settings WHERE id = 'singleton'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(row.0, "update checking defaults on");
    assert_eq!(
        row.1, 300,
        "the V8 idle timeout keeps its default across the rename"
    );
}

#[tokio::test]
async fn recurring_job_kinds_can_be_persisted() {
    let pool = fresh_pool().await;
    kani_app::jobs::recurring::ensure_recurring_rows(&pool)
        .await
        .unwrap();

    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM recurring_jobs ORDER BY kind")
        .fetch_all(&pool)
        .await
        .unwrap();

    for expected in ["update_check", "v8_process_reap", "db_maintenance"] {
        assert!(
            kinds.iter().any(|k| k == expected),
            "{expected} should be seeded, got {kinds:?}"
        );
    }
}
