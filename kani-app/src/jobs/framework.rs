//! Job implementation contract and the execution context supplied by [`super::JobManager`].

use std::sync::Arc;

use crate::events::AppEvent;
use crate::jobs::error::JobError;

pub type JobId = uuid::Uuid;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[repr(i64)]
/// Scheduling priority persisted as a stable numeric value in the jobs table.
pub enum JobPriority {
    Low = 0,
    Normal = 50,
    High = 100,
}

impl JobPriority {
    pub(crate) fn from_i64(v: i64) -> Self {
        match v {
            100 => Self::High,
            50 => Self::Normal,
            _ => Self::Low,
        }
    }
}

#[async_trait::async_trait]
/// Persistable unit of background work executed by [`super::JobManager`].
///
/// Implementations are serialized before dispatch so interrupted jobs can be reconstructed through
/// [`super::JobRegistry`]. Long-running work must observe [`JobContext::cancel`] and report durable
/// progress through the supplied reporter.
pub trait BackgroundJob: Send + Sync + 'static {
    /// Stable discriminator persisted with serialized job parameters.
    const JOB_TYPE: &'static str;

    /// Successful value serialized into job history.
    type Output: serde::Serialize + Send;

    /// Stable identity for this submission and all of its retry attempts.
    fn id(&self) -> JobId;

    fn job_type(&self) -> &'static str {
        Self::JOB_TYPE
    }

    /// Human-readable description exposed in the jobs UI.
    fn description(&self) -> String;

    fn priority(&self) -> JobPriority {
        JobPriority::Normal
    }

    fn source_id(&self) -> Option<i64> {
        None
    }

    /// Current zero-based retry count persisted for recovery.
    fn attempt_count(&self) -> u32 {
        0
    }

    /// Serialized parameters for a follow-up retry, or `None` when retry reconstruction is unsupported.
    fn retry_params(&self) -> Option<String> {
        None
    }

    /// Executes the job. Returning does not bypass manager-side history or retry handling.
    async fn run(self: Box<Self>, ctx: JobContext) -> Result<Self::Output, JobError>;

    /// Performs job-specific cancellation cleanup after the cancellation token is triggered.
    async fn on_cancel(&self) {}
}

/// Shared cell used to give jobs access to the `AppService` without a circular Arc.
/// The manager populates this after `AppService` is fully constructed.
pub(crate) type ServiceCell = Arc<std::sync::Mutex<Option<crate::service::AppService>>>;

#[derive(Clone)]
/// Resources and the submission-time concurrency snapshot supplied to a running job.
pub struct JobContext {
    pub pool: sqlx::sqlite::SqlitePool,
    pub cancel: tokio_util::sync::CancellationToken,
    pub progress: JobProgressReporter,
    pub concurrency: JobConcurrencySnapshot,
    pub(crate) svc: ServiceCell,
}

impl JobContext {
    /// # Panics
    ///
    /// Panics if the service was not set before any job was dispatched.
    pub fn service(&self) -> crate::service::AppService {
        self.svc
            .lock()
            .expect("service cell lock")
            .clone()
            .expect("AppService not set in job service cell")
    }
}

#[derive(Clone)]
/// Runtime concurrency settings captured when a job context is created.
pub struct JobConcurrencySnapshot {
    pub page_concurrency: usize,
    pub per_source_download_concurrency: usize,
    pub scan_concurrency: usize,
}

#[derive(Clone, serde::Serialize, Default)]
/// Latest progress snapshot retained for persistence and API reporting.
pub struct JobProgress {
    pub current: u64,
    pub total: u64,
    pub message: String,
}

#[derive(Clone)]
/// Updates a job's in-memory progress snapshot and broadcasts the same state over SSE.
pub struct JobProgressReporter {
    job_id: JobId,
    job_type: &'static str,
    sse_tx: tokio::sync::broadcast::Sender<AppEvent>,
    progress: Arc<tokio::sync::Mutex<JobProgress>>,
}

impl JobProgressReporter {
    pub(crate) fn new(
        job_id: JobId,
        job_type: &'static str,
        sse_tx: tokio::sync::broadcast::Sender<AppEvent>,
    ) -> Self {
        Self {
            job_id,
            job_type,
            sse_tx,
            progress: Arc::new(tokio::sync::Mutex::new(JobProgress::default())),
        }
    }

    pub async fn report(&self, current: u64, total: u64, message: impl Into<String>) {
        let msg = message.into();
        {
            let mut p = self.progress.lock().await;
            p.current = current;
            p.total = total;
            p.message = msg.clone();
        }
        let _ = self.sse_tx.send(AppEvent::JobProgress {
            job_id: self.job_id,
            job_type: self.job_type.to_string(),
            current,
            total,
            message: msg,
        });
    }

    pub async fn current(&self) -> JobProgress {
        self.progress.lock().await.clone()
    }
}
