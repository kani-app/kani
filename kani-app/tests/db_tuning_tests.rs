#![allow(clippy::unwrap_used)]

mod common;
use common::test_service;
use kani_app::jobs::maintenance::{AnalyzeJob, VacuumJob};
use std::time::Duration;

async fn wait_job(svc: &kani_app::service::AppService, job_id: uuid::Uuid) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let status = svc.job_manager.status(job_id).await.unwrap();
        if status.status == "completed" || status.status == "failed" {
            return status.status;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out waiting for job {job_id}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn analyze_job_completes() {
    let svc = test_service().await;
    let job_id = svc.job_manager.submit(AnalyzeJob::new()).await.unwrap();
    assert_eq!(wait_job(&svc, job_id).await, "completed");
}

#[tokio::test]
async fn vacuum_job_completes() {
    let svc = test_service().await;
    let job_id = svc.job_manager.submit(VacuumJob::new()).await.unwrap();
    assert_eq!(wait_job(&svc, job_id).await, "completed");
}

#[tokio::test]
async fn write_succeeds_on_read_write_split() {
    let svc = test_service().await;

    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO categories (name, sort_order) VALUES ('test-split', 1) RETURNING id",
    )
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert!(id > 0, "write to svc.db must succeed");

    let found: Option<(String,)> = sqlx::query_as("SELECT name FROM categories WHERE id = ?")
        .bind(id)
        .fetch_optional(&svc.db_read)
        .await
        .unwrap();
    assert_eq!(
        found.as_ref().map(|r| r.0.as_str()),
        Some("test-split"),
        "read on db_read must see the committed write"
    );
}
