//! Persistent, observable background jobs with retry, cancellation, and concurrency control.

pub mod archive_export;
pub mod audit_prune;
pub mod backup;
pub mod circuit_breaker;
pub mod download;
pub mod error;
pub mod framework;
pub mod import_dedup;
pub mod import_resolve;
pub mod maintenance;
pub mod manager;
pub mod manifest_backfill;
pub mod migration;
pub mod pending_delete_retry;
pub mod recurring;
pub mod refresh;
pub mod scan;
pub mod scrub;
pub mod storage;
pub mod thumbnail;
pub mod tracker_sync;
pub mod trash_purge;
pub mod update_check;
pub mod v8_reap;
pub mod webhook_delivery;

pub use error::{DownloadErrorKind, JobError, RetryPolicy};
pub use framework::{
    BackgroundJob, JobConcurrencySnapshot, JobContext, JobId, JobPriority, JobProgress,
    JobProgressReporter,
};
pub use manager::{
    ActiveJobHandle, ConcurrencyConfig, JobListFilter, JobManager, JobManagerConfig, JobRegistry,
    JobStatus, JobSummary, JobTypeConfig,
};

#[cfg(any(test, feature = "test-util"))]
pub mod test_jobs {
    use super::*;
    use crate::error::Result;
    use std::time::Duration;

    /// A job that completes immediately, returning its payload string.
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct TestJob {
        id: JobId,
        pub payload: String,
    }

    impl TestJob {
        pub fn new(payload: impl Into<String>) -> Self {
            Self {
                id: JobId::new_v4(),
                payload: payload.into(),
            }
        }
    }

    #[async_trait::async_trait]
    impl BackgroundJob for TestJob {
        const JOB_TYPE: &'static str = "test_job";
        type Output = String;

        fn id(&self) -> JobId {
            self.id
        }

        fn description(&self) -> String {
            format!("Test job: {}", self.payload)
        }

        async fn run(self: Box<Self>, _ctx: JobContext) -> Result<String, JobError> {
            Ok(self.payload)
        }
    }

    /// A job that sleeps until cancelled, for testing cancellation.
    #[derive(serde::Serialize, serde::Deserialize)]
    pub struct SlowTestJob {
        id: JobId,
        sleep_secs: u64,
    }

    impl SlowTestJob {
        pub fn new(duration: Duration) -> Self {
            Self {
                id: JobId::new_v4(),
                sleep_secs: duration.as_secs(),
            }
        }
    }

    #[async_trait::async_trait]
    impl BackgroundJob for SlowTestJob {
        const JOB_TYPE: &'static str = "test_slow_job";
        type Output = ();

        fn id(&self) -> JobId {
            self.id
        }

        fn description(&self) -> String {
            format!("Slow test job ({}s)", self.sleep_secs)
        }

        async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
            tokio::select! {
                _ = ctx.cancel.cancelled() => Err(JobError::Cancelled),
                _ = tokio::time::sleep(Duration::from_secs(self.sleep_secs)) => Ok(()),
            }
        }
    }

    /// A download job that always fails, exercising the source circuit breaker.
    #[derive(serde::Serialize, serde::Deserialize, Clone)]
    pub struct FailingDownloadJob {
        id: JobId,
        pub source_id: i64,
        pub attempt: u32,
        pub kind: String,
    }

    impl FailingDownloadJob {
        pub fn new(source_id: i64) -> Self {
            Self {
                id: JobId::new_v4(),
                source_id,
                attempt: 0,
                kind: "network".into(),
            }
        }

        pub fn network(source_id: i64) -> Self {
            Self::new(source_id)
        }

        pub fn not_found(source_id: i64) -> Self {
            Self {
                kind: "not_found".into(),
                ..Self::new(source_id)
            }
        }
    }

    #[async_trait::async_trait]
    impl BackgroundJob for FailingDownloadJob {
        const JOB_TYPE: &'static str = "test_failing_download_job";
        type Output = ();

        fn id(&self) -> JobId {
            self.id
        }

        fn description(&self) -> String {
            format!("Failing download test job (source {})", self.source_id)
        }

        fn source_id(&self) -> Option<i64> {
            Some(self.source_id)
        }

        fn attempt_count(&self) -> u32 {
            self.attempt
        }

        fn retry_params(&self) -> Option<String> {
            let next = Self {
                id: JobId::new_v4(),
                attempt: self.attempt + 1,
                ..self.clone()
            };
            serde_json::to_string(&next).ok()
        }

        async fn run(self: Box<Self>, _ctx: JobContext) -> Result<(), JobError> {
            let kind = match self.kind.as_str() {
                "not_found" => DownloadErrorKind::NotFound,
                "auth" => DownloadErrorKind::AuthRequired,
                _ => DownloadErrorKind::Network { retryable: true },
            };
            Err(JobError::Download(kind))
        }
    }
}
