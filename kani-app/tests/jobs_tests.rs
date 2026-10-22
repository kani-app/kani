#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_chapter, insert_manga, insert_source, start_mock_page_server, test_service};
use kani_app::jobs::BackgroundJob;
use kani_app::jobs::JobId;
use kani_app::jobs::audit_prune::AuditPruneJob;
use kani_app::jobs::download::MangaDownloadAllJob;
use kani_app::jobs::import_dedup::ImportDedupJob;
use kani_app::jobs::pending_delete_retry::PendingDeleteRetryJob;
use kani_app::jobs::test_jobs::{FailingDownloadJob, SlowTestJob, TestJob};
use kani_app::jobs::trash_purge::TrashPurgeJob;
use kani_app::jobs::v8_reap::V8ReapJob;
use kani_app::jobs::webhook_delivery::WebhookDeliveryJob;
use kani_app::service::AppService;
use kani_core::downloader::MockPageListFetcher;
use std::time::Duration;

#[tokio::test]
async fn test_job_submits_and_completes() {
    let svc = test_service().await;
    let job = TestJob::new("hello");
    let job_id = svc.job_manager.submit(job).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = svc.job_manager.status(job_id).await.unwrap();
        if status.status == "completed" {
            assert!(status.result.is_some());
            return;
        }
        if status.status == "failed" || status.status == "cancelled" {
            panic!("job ended in unexpected state: {}", status.status);
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timed out waiting for job to complete; last status: {}",
                status.status
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn slow_job_cancel_transitions_to_cancelled() {
    let svc = test_service().await;
    let job = SlowTestJob::new(Duration::from_secs(60));
    let job_id = svc.job_manager.submit(job).await.unwrap();

    let start_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = svc.job_manager.status(job_id).await.unwrap();
        if status.status == "running" {
            break;
        }
        if tokio::time::Instant::now() >= start_deadline {
            panic!(
                "timed out waiting for slow job to start; status: {}",
                status.status
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    svc.job_manager.cancel(job_id).await.unwrap();

    let cancel_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = svc.job_manager.status(job_id).await.unwrap();
        if status.status == "cancelled" {
            return;
        }
        if status.status == "completed" || status.status == "failed" {
            panic!(
                "job ended in unexpected state after cancel: {}",
                status.status
            );
        }
        if tokio::time::Instant::now() >= cancel_deadline {
            panic!(
                "timed out waiting for cancel; last status: {}",
                status.status
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn crashed_running_job_recovers_to_pending_on_startup() {
    let db = common::test_db().await;

    let job = TestJob::new("recovery-payload");
    let job_id = job.id().to_string();
    let params = serde_json::to_string(&job).unwrap();
    sqlx::query(
        "INSERT INTO jobs (id, job_type, status, priority, description, params_json, created_at, started_at) \
         VALUES (?, 'test_job', 'running', 50, 'Test job: recovery-payload', ?, 1000000, 1000001)",
    )
    .bind(&job_id)
    .bind(&params)
    .execute(&db)
    .await
    .unwrap();

    let svc = kani_app::service::AppService::new_for_test(db).await;

    let recovered_id = uuid::Uuid::parse_str(&job_id).unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = svc.job_manager.status(recovered_id).await.unwrap();
        if status.status == "completed" {
            return;
        }
        if status.status == "failed" || status.status == "cancelled" {
            panic!("recovered job ended in unexpected state: {}", status.status);
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timed out waiting for recovered job; last status: {}",
                status.status
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn circuit_breaker_opens_after_repeated_failures() {
    let svc = test_service().await;
    let source_id = 999i64;

    for _ in 0..5 {
        let job = FailingDownloadJob::network(source_id);
        let job_id = svc.job_manager.submit(job).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            let status = svc.job_manager.status(job_id).await.unwrap();
            if status.status == "failed" {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "timed out waiting for job to fail; status: {}",
                    status.status
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    let state = svc.job_manager.circuit_state(source_id);
    assert_eq!(
        state.as_deref(),
        Some("open"),
        "circuit should be open after threshold failures"
    );
}

#[tokio::test]
async fn circuit_breaker_not_found_does_not_open_circuit() {
    let svc = test_service().await;
    let source_id = 998i64;

    for _ in 0..10 {
        let job = FailingDownloadJob::not_found(source_id);
        let job_id = svc.job_manager.submit(job).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            let status = svc.job_manager.status(job_id).await.unwrap();
            if status.status == "failed" {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("timed out; status: {}", status.status);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    let state = svc.job_manager.circuit_state(source_id);
    assert!(
        state.is_none() || state.as_deref() == Some("closed"),
        "NotFound errors should not open the circuit; got {state:?}"
    );
}

#[tokio::test]
async fn retry_policy_delay_scaling() {
    use kani_app::jobs::DownloadErrorKind;
    let kind = DownloadErrorKind::Network { retryable: true };
    let policy = kind.retry_policy().unwrap();
    assert_eq!(policy.max_attempts, 3);
    let d0 = policy.delay_for_attempt(0);
    let d1 = policy.delay_for_attempt(1);
    assert!(d1 > d0, "backoff delay should increase with attempt count");
}

#[tokio::test]
async fn manga_download_all_job_no_pending_chapters_completes_immediately() {
    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, source_id, "m1", "Manga").await;

    let job = MangaDownloadAllJob::new(manga_id.0, "Manga".to_string(), source_id, false);
    let job_id = svc.job_manager.submit(job).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = svc.job_manager.status(job_id).await.unwrap();
        if status.status == "completed" {
            return;
        }
        if status.status == "failed" || status.status == "cancelled" {
            panic!("unexpected terminal state: {}", status.status);
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out; last status: {}", status.status);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn manga_download_all_job_claims_all_pending_chapters() {
    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, source_id, "m1", "Manga").await;

    let ch1 = insert_chapter(&svc.db, manga_id, "c1", 1.0).await;
    let ch2 = insert_chapter(&svc.db, manga_id, "c2", 2.0).await;
    let ch3 = insert_chapter(&svc.db, manga_id, "c3", 3.0).await;

    let job = MangaDownloadAllJob::new(manga_id.0, "Manga".to_string(), source_id, false);
    let job_id = svc.job_manager.submit(job).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = svc.job_manager.status(job_id).await.unwrap();
        if matches!(status.status.as_str(), "completed" | "failed" | "cancelled") {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out waiting for job; last status: {}", status.status);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    for ch_id in [ch1, ch2, ch3] {
        let dl_status: i64 =
            sqlx::query_scalar!("SELECT download_status FROM chapters WHERE id = ?", ch_id,)
                .fetch_one(&svc.db)
                .await
                .unwrap();
        assert_ne!(
            dl_status, 0,
            "chapter {} should have been claimed (status 0 = still Pending)",
            ch_id
        );
    }
}

#[tokio::test]
async fn manga_download_all_job_retryable_failure_submits_chapter_job() {
    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, source_id, "m1", "Manga").await;
    let _chapter_id = insert_chapter(&svc.db, manga_id, "ch-1", 1.0).await;

    svc.register_mock_source(
        source_id,
        MockPageListFetcher::failing("simulated extension error"),
    );

    let job = MangaDownloadAllJob::new(manga_id.0, "Manga".to_string(), source_id, false);
    let job_id = svc.job_manager.submit(job).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = svc.job_manager.status(job_id).await.unwrap();
        if matches!(status.status.as_str(), "completed" | "failed" | "cancelled") {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timed out waiting for aggregate job; last status: {}",
                status.status
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let retry_job_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE job_type = 'chapter_download'")
            .fetch_one(&svc.db)
            .await
            .unwrap();
    assert!(
        retry_job_count > 0,
        "a retryable chapter failure should have submitted a standalone chapter_download job"
    );
}

#[tokio::test]
async fn manga_download_all_job_cancel_reverts_chapters_to_pending() {
    let svc = test_service().await;
    let source_id = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, source_id, "m1", "Manga").await;
    let ch1 = insert_chapter(&svc.db, manga_id, "ch-1", 1.0).await;
    let ch2 = insert_chapter(&svc.db, manga_id, "ch-2", 2.0).await;
    let ch3 = insert_chapter(&svc.db, manga_id, "ch-3", 3.0).await;

    svc.register_mock_source(source_id, MockPageListFetcher::slow(600, 1, 0));

    let job = MangaDownloadAllJob::new(manga_id.0, "Manga".to_string(), source_id, false);
    let job_id = svc.job_manager.submit(job).await.unwrap();

    let claim_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let statuses: Vec<i64> =
            sqlx::query_scalar("SELECT download_status FROM chapters WHERE manga_id = ?")
                .bind(manga_id.0)
                .fetch_all(&svc.db)
                .await
                .unwrap();
        if statuses.iter().all(|&s| s != 0) {
            break;
        }
        if tokio::time::Instant::now() >= claim_deadline {
            panic!("timed out waiting for chapters to be claimed");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    svc.job_manager.cancel(job_id).await.unwrap();

    let terminal_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = svc.job_manager.status(job_id).await.unwrap();
        if matches!(status.status.as_str(), "completed" | "failed" | "cancelled") {
            break;
        }
        if tokio::time::Instant::now() >= terminal_deadline {
            panic!(
                "timed out waiting for job to finish after cancel; status: {}",
                status.status
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    for ch_id in [ch1, ch2, ch3] {
        let dl_status: i64 =
            sqlx::query_scalar!("SELECT download_status FROM chapters WHERE id = ?", ch_id,)
                .fetch_one(&svc.db)
                .await
                .unwrap();
        assert_eq!(
            dl_status, 0,
            "chapter {} should have been reverted to Pending after cancel",
            ch_id
        );
    }
}

#[tokio::test]
async fn chapter_download_full_pipeline_with_mock() {
    let svc = test_service().await;
    let port = start_mock_page_server().await;

    let source_id = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, source_id, "manga-001", "TestManga").await;
    let chapter_id = insert_chapter(&svc.db, manga_id, "ch-001", 1.0).await;

    svc.register_mock_source(source_id, MockPageListFetcher::succeeding(3, port));

    let job_id = svc.download_chapter(chapter_id).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = svc.job_manager.status(job_id).await.unwrap();
        if status.status == "completed" {
            break;
        }
        if status.status == "failed" || status.status == "cancelled" {
            panic!(
                "chapter download ended in unexpected state '{}': {:?}",
                status.status, status.error
            );
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timed out waiting for chapter download; last status: {}",
                status.status
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let dl_status: i64 = sqlx::query_scalar!(
        "SELECT download_status FROM chapters WHERE id = ?",
        chapter_id
    )
    .fetch_one(&svc.db)
    .await
    .unwrap();
    assert_eq!(
        dl_status, 2,
        "chapter should be marked Complete (download_status = 2)"
    );
}

/// Polls a submitted job until it reaches a terminal state, returning that state.
async fn await_terminal(svc: &AppService, job_id: JobId) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = svc.job_manager.status(job_id).await.unwrap();
        if matches!(status.status.as_str(), "completed" | "failed" | "cancelled") {
            return status.status;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out; last status: {}", status.status);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn audit_prune_job_runs_to_completion() {
    let svc = test_service().await;
    let job_id = svc.job_manager.submit(AuditPruneJob::new()).await.unwrap();
    assert_eq!(await_terminal(&svc, job_id).await, "completed");
}

#[tokio::test]
async fn v8_reap_job_runs_to_completion() {
    let svc = test_service().await;
    let job_id = svc.job_manager.submit(V8ReapJob::new()).await.unwrap();
    assert_eq!(await_terminal(&svc, job_id).await, "completed");
}

#[tokio::test]
async fn trash_purge_job_runs_to_completion() {
    let svc = test_service().await;
    let job_id = svc
        .job_manager
        .submit(TrashPurgeJob::new(30))
        .await
        .unwrap();
    assert_eq!(await_terminal(&svc, job_id).await, "completed");
}

#[tokio::test]
async fn pending_delete_retry_job_runs_to_completion() {
    let svc = test_service().await;
    let job_id = svc
        .job_manager
        .submit(PendingDeleteRetryJob::new())
        .await
        .unwrap();
    assert_eq!(await_terminal(&svc, job_id).await, "completed");
}

#[tokio::test]
async fn import_dedup_job_runs_to_completion() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga = insert_manga(&svc.db, src, "m1", "Manga").await;
    let job_id = svc
        .job_manager
        .submit(ImportDedupJob::new(vec![manga.0]))
        .await
        .unwrap();
    assert_eq!(await_terminal(&svc, job_id).await, "completed");
}

#[tokio::test]
async fn webhook_delivery_job_completes_on_2xx() {
    let svc = test_service().await;
    svc.webhook_service.allow_private_egress_for_test();
    let port = start_mock_page_server().await;
    let url = format!("http://127.0.0.1:{port}/");
    let webhook_id: i64 =
        sqlx::query_scalar("INSERT INTO webhooks (url, enabled) VALUES (?, 1) RETURNING id")
            .bind(&url)
            .fetch_one(&svc.db)
            .await
            .unwrap();
    let job_id = svc
        .job_manager
        .submit(WebhookDeliveryJob::new(
            webhook_id,
            "chapter.new".to_string(),
            "{}".to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(await_terminal(&svc, job_id).await, "completed");
}

/// The reader re-fetches pages the moment it sees `ChapterCompleted`, and that
/// fetch 404s while `download_status` is still Pending. The event must therefore
/// not become observable until the row has been committed.
#[tokio::test]
async fn chapter_completed_is_not_broadcast_before_the_row_is_marked_downloaded() {
    use kani_shared::DownloadProgressEvent;

    let svc = test_service().await;
    let port = start_mock_page_server().await;

    let source_id = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, source_id, "manga-001", "TestManga").await;
    let chapter_id = insert_chapter(&svc.db, manga_id, "ch-001", 1.0).await;
    svc.register_mock_source(source_id, MockPageListFetcher::succeeding(3, port));

    let mut events = svc.downloader.subscribe();
    svc.download_chapter(chapter_id).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let remaining = deadline - tokio::time::Instant::now();
        let event = tokio::time::timeout(remaining, events.recv())
            .await
            .expect("timed out waiting for ChapterCompleted")
            .expect("event channel closed");
        if let DownloadProgressEvent::ChapterCompleted { chapter_id: id, .. } = event
            && id == i64::from(chapter_id)
        {
            break;
        }
    }

    let dl_status: i64 = sqlx::query_scalar!(
        "SELECT download_status FROM chapters WHERE id = ?",
        chapter_id
    )
    .fetch_one(&svc.db)
    .await
    .unwrap();

    assert_eq!(
        dl_status, 2,
        "ChapterCompleted was observable while download_status was still {dl_status}; \
         a reader re-fetching on this event gets a 404"
    );
}
