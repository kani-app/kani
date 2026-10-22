//! Persistent priority queue, recovery, retry, and concurrency enforcement for background jobs.

use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::{Mutex, Notify, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::error::{Result, ServiceError};
use crate::events::AppEvent;
use crate::jobs::error::JobError;
use crate::jobs::framework::{
    BackgroundJob, JobConcurrencySnapshot, JobContext, JobId, JobPriority, JobProgressReporter,
};

#[async_trait::async_trait]
pub(crate) trait ErasedJob: Send + Sync + 'static {
    fn id(&self) -> JobId;
    fn job_type(&self) -> &'static str;
    fn description(&self) -> String;
    fn priority(&self) -> JobPriority;
    fn source_id(&self) -> Option<i64>;
    fn attempt_count(&self) -> u32;
    fn retry_params(&self) -> Option<String>;
    async fn run_erased(self: Box<Self>, ctx: JobContext) -> Result<serde_json::Value, JobError>;
    async fn on_cancel(&self);
}

#[async_trait::async_trait]
impl<T: BackgroundJob> ErasedJob for T
where
    T::Output: serde::Serialize + Send + 'static,
{
    fn id(&self) -> JobId {
        BackgroundJob::id(self)
    }
    fn job_type(&self) -> &'static str {
        BackgroundJob::job_type(self)
    }
    fn description(&self) -> String {
        BackgroundJob::description(self)
    }
    fn priority(&self) -> JobPriority {
        BackgroundJob::priority(self)
    }
    fn source_id(&self) -> Option<i64> {
        BackgroundJob::source_id(self)
    }
    fn attempt_count(&self) -> u32 {
        BackgroundJob::attempt_count(self)
    }
    fn retry_params(&self) -> Option<String> {
        BackgroundJob::retry_params(self)
    }
    async fn run_erased(self: Box<Self>, ctx: JobContext) -> Result<serde_json::Value, JobError> {
        let out = BackgroundJob::run(self, ctx).await?;
        serde_json::to_value(out).map_err(|e| JobError::Internal(e.to_string()))
    }
    async fn on_cancel(&self) {
        BackgroundJob::on_cancel(self).await
    }
}

type JobFactory =
    Box<dyn Fn(serde_json::Value) -> Result<Box<dyn ErasedJob>, serde_json::Error> + Send + Sync>;

#[derive(Default)]
/// Deserialization registry used to reconstruct persisted jobs during startup recovery.
pub struct JobRegistry {
    factories: HashMap<&'static str, JobFactory>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<J>(&mut self)
    where
        J: BackgroundJob + serde::de::DeserializeOwned + 'static,
        J::Output: serde::Serialize + Send + 'static,
    {
        self.factories.insert(
            J::JOB_TYPE,
            Box::new(|v| {
                let job: J = serde_json::from_value(v)?;
                Ok(Box::new(job) as Box<dyn ErasedJob>)
            }),
        );
    }

    fn deserialise(
        &self,
        job_type: &str,
        params: &str,
    ) -> std::result::Result<Box<dyn ErasedJob>, String> {
        let factory = self
            .factories
            .get(job_type)
            .ok_or_else(|| format!("Unknown job_type: {job_type}"))?;
        let v: serde_json::Value = serde_json::from_str(params).map_err(|e| e.to_string())?;
        factory(v).map_err(|e| e.to_string())
    }
}

/// Queue entry ordered by descending priority and then oldest submission first.
pub(crate) struct QueuedJob {
    pub priority: JobPriority,
    pub created_at: Instant,
    pub source_id: Option<i64>,
    pub(crate) job: Box<dyn ErasedJob>,
}

impl PartialEq for QueuedJob {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}
impl Eq for QueuedJob {}

impl Ord for QueuedJob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then(other.created_at.cmp(&self.created_at))
    }
}

impl PartialOrd for QueuedJob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Cancellation and progress controls retained while one job is running.
pub struct ActiveJobHandle {
    pub job_id: JobId,
    pub job_type: &'static str,
    pub description: String,
    pub cancel: CancellationToken,
    pub progress: JobProgressReporter,
}

/// Concurrency gate for one registered job type.
pub struct JobTypeConfig {
    pub max_concurrent: usize,
    pub semaphore: Arc<Semaphore>,
}

/// Startup configuration and recovery registry for a [`JobManager`].
pub struct JobManagerConfig {
    pub global_max_concurrent: usize,
    pub job_shutdown_timeout: Duration,
    pub type_configs: HashMap<&'static str, JobTypeConfig>,
    pub registry: JobRegistry,
    pub max_history: usize,
    pub concurrency: ConcurrencyConfig,
    pub svc_cell: crate::jobs::framework::ServiceCell,
}

/// Live concurrency limits propagated into each [`JobContext`].
pub struct ConcurrencyConfig {
    pub page_concurrency: usize,
    pub per_source_download_concurrency: usize,
    pub scan_concurrency: usize,
}

#[derive(Debug, serde::Serialize)]
/// Detailed persisted state for one job lookup.
pub struct JobStatus {
    pub id: String,
    pub status: String,
    pub completed_at: Option<i64>,
    pub progress: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Default)]
/// Database filters and pagination applied when listing job history.
pub struct JobListFilter {
    pub job_type: Option<String>,
    /// Match any of these statuses. Empty means "no status filter" — the jobs UI
    /// groups statuses per tab (active = pending+running, failed = failed+cancelled),
    /// so a single-value filter can't express what it needs.
    pub statuses: Vec<String>,
    pub limit: i64,
    pub offset: i64,
    pub user_id: Option<i64>,
}

/// One page of jobs plus the total matching count, so the UI can paginate.
#[derive(Debug, serde::Serialize)]
pub struct JobListPage {
    pub jobs: Vec<JobSummary>,
    pub total: i64,
    /// Every job type present in the table (ignoring the current filter), so the
    /// type filter can offer a complete option list rather than only what is on
    /// the current page.
    pub job_types: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
/// API-facing summary of one current or historical job.
/// Timestamp fields are Unix seconds.
pub struct JobSummary {
    pub id: String,
    pub job_type: String,
    pub status: String,
    pub priority: i64,
    pub description: String,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub progress: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
    pub user_id: Option<i64>,
}

#[derive(Clone)]
/// Persistent background-work scheduler shared by service and HTTP layers.
///
/// It enforces global, per-type, and per-source concurrency; records lifecycle state; reconstructs
/// interrupted work at startup; and owns retry and circuit-breaker decisions.
pub struct JobManager {
    pool: sqlx::sqlite::SqlitePool,
    sse_tx: tokio::sync::broadcast::Sender<AppEvent>,
    active: Arc<DashMap<JobId, ActiveJobHandle>>,
    queue: Arc<Mutex<BinaryHeap<QueuedJob>>>,
    global_semaphore: Arc<Semaphore>,
    type_configs: Arc<HashMap<&'static str, JobTypeConfig>>,
    new_job_notify: Arc<Notify>,
    completion_tx: tokio::sync::broadcast::Sender<JobId>,
    max_history: usize,
    concurrency: Arc<ConcurrencyConfig>,
    svc_cell: crate::jobs::framework::ServiceCell,
    registry: Arc<JobRegistry>,
    per_source_semaphores: Arc<DashMap<i64, Arc<Semaphore>>>,
    circuit_breakers: Arc<DashMap<i64, crate::jobs::circuit_breaker::CircuitBreaker>>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

impl JobManager {
    pub async fn new(
        pool: sqlx::sqlite::SqlitePool,
        sse_tx: tokio::sync::broadcast::Sender<AppEvent>,
        shutdown_token: CancellationToken,
        config: JobManagerConfig,
    ) -> Result<Self> {
        // Startup recovery: any jobs stuck in 'running' state crashed mid-execution.
        let crashed_ids: Vec<String> = sqlx::query_scalar::<_, String>(
            "UPDATE jobs SET status = 'pending', started_at = NULL WHERE status = 'running' RETURNING id",
        )
        .fetch_all(&pool)
        .await?;
        if !crashed_ids.is_empty() {
            tracing::warn!(
                "Recovered {} crashed jobs → re-queued as pending",
                crashed_ids.len()
            );
        }

        let registry = Arc::new(config.registry);
        let queue = Arc::new(Mutex::new(BinaryHeap::<QueuedJob>::new()));

        let pending = sqlx::query!(
            "SELECT id, job_type, priority, description, params_json FROM jobs \
             WHERE status = 'pending' ORDER BY priority DESC, created_at ASC"
        )
        .fetch_all(&pool)
        .await?;

        {
            let mut q = queue.lock().await;
            for row in pending {
                let Some(params) = row.params_json else {
                    continue;
                };
                match registry.deserialise(&row.job_type, &params) {
                    Ok(job) => {
                        let source_id = job.source_id();
                        q.push(QueuedJob {
                            priority: JobPriority::from_i64(row.priority),
                            created_at: Instant::now(),
                            source_id,
                            job,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Cannot restore job {} (type={}): {} — marking failed",
                            row.id,
                            row.job_type,
                            e
                        );
                        let err_json = serde_json::json!({
                            "kind": "internal",
                            "message": "params_json deserialisation failed after restart"
                        })
                        .to_string();
                        let _ = sqlx::query!(
                            "UPDATE jobs SET status = 'failed', error_json = ? WHERE id = ?",
                            err_json,
                            row.id
                        )
                        .execute(&pool)
                        .await;
                    }
                }
            }
        }

        let circuit_breakers: Arc<DashMap<i64, crate::jobs::circuit_breaker::CircuitBreaker>> =
            Arc::new(DashMap::new());
        let cb_rows = sqlx::query!(
            "SELECT source_id, state, failure_count, last_failure_at, next_retry_at \
             FROM source_circuit_breakers"
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        for row in cb_rows {
            use crate::jobs::circuit_breaker::{CircuitBreaker, CircuitState};
            let state = match row.state.as_str() {
                "open" => CircuitState::Open,
                "half_open" => CircuitState::HalfOpen,
                _ => CircuitState::Closed,
            };
            circuit_breakers.insert(
                row.source_id,
                CircuitBreaker {
                    state,
                    failure_count: row.failure_count as u32,
                    last_failure_at: row.last_failure_at,
                    next_retry_at: row.next_retry_at,
                },
            );
        }

        let (completion_tx, _) = tokio::sync::broadcast::channel::<JobId>(64);
        let new_job_notify = Arc::new(Notify::new());
        let global_semaphore = Arc::new(Semaphore::new(config.global_max_concurrent));
        let type_configs = Arc::new(config.type_configs);
        let max_history = config.max_history;
        let concurrency = Arc::new(config.concurrency);

        let manager = Self {
            pool,
            sse_tx,
            active: Arc::new(DashMap::new()),
            queue,
            global_semaphore,
            type_configs,
            new_job_notify,
            completion_tx,
            max_history,
            concurrency,
            svc_cell: config.svc_cell,
            registry,
            per_source_semaphores: Arc::new(DashMap::new()),
            circuit_breakers,
        };

        manager.spawn_executor(shutdown_token, config.job_shutdown_timeout);

        if !crashed_ids.is_empty() {
            manager.new_job_notify.notify_one();
        }

        Ok(manager)
    }

    fn spawn_executor(&self, shutdown_token: CancellationToken, drain_timeout: Duration) {
        let pool = self.pool.clone();
        let sse_tx = self.sse_tx.clone();
        let active = Arc::clone(&self.active);
        let queue = Arc::clone(&self.queue);
        let global_semaphore = Arc::clone(&self.global_semaphore);
        let type_configs = Arc::clone(&self.type_configs);
        let notify = Arc::clone(&self.new_job_notify);
        let completion_tx = self.completion_tx.clone();
        let max_history = self.max_history;
        let concurrency = Arc::clone(&self.concurrency);
        let svc_cell = Arc::clone(&self.svc_cell);
        let registry = Arc::clone(&self.registry);
        let per_source_semaphores = Arc::clone(&self.per_source_semaphores);
        let circuit_breakers = Arc::clone(&self.circuit_breakers);

        tokio::spawn(async move {
            const POLL_INTERVAL: Duration = Duration::from_secs(10);

            loop {
                tokio::select! {
                    _ = shutdown_token.cancelled() => {
                        Self::drain_active(&active, &completion_tx, drain_timeout).await;
                        break;
                    }
                    _ = notify.notified() => {}
                    _ = tokio::time::sleep(POLL_INTERVAL) => {}
                }

                Self::dispatch_ready_jobs(
                    &pool,
                    &sse_tx,
                    &active,
                    &queue,
                    &global_semaphore,
                    &type_configs,
                    &notify,
                    &completion_tx,
                    max_history,
                    &concurrency,
                    &svc_cell,
                    &registry,
                    &per_source_semaphores,
                    &circuit_breakers,
                )
                .await;
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    async fn dispatch_ready_jobs(
        pool: &sqlx::sqlite::SqlitePool,
        sse_tx: &tokio::sync::broadcast::Sender<AppEvent>,
        active: &Arc<DashMap<JobId, ActiveJobHandle>>,
        queue: &Arc<Mutex<BinaryHeap<QueuedJob>>>,
        global_semaphore: &Arc<Semaphore>,
        type_configs: &Arc<HashMap<&'static str, JobTypeConfig>>,
        notify: &Arc<Notify>,
        completion_tx: &tokio::sync::broadcast::Sender<JobId>,
        max_history: usize,
        concurrency: &Arc<ConcurrencyConfig>,
        svc_cell: &crate::jobs::framework::ServiceCell,
        registry: &Arc<JobRegistry>,
        per_source_semaphores: &Arc<DashMap<i64, Arc<Semaphore>>>,
        circuit_breakers: &Arc<DashMap<i64, crate::jobs::circuit_breaker::CircuitBreaker>>,
    ) {
        loop {
            let permit = match global_semaphore.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => break,
            };

            let queued = {
                let mut q = queue.lock().await;
                match q.pop() {
                    Some(j) => j,
                    None => {
                        drop(permit);
                        break;
                    }
                }
            };

            let job_id = queued.job.id();
            let job_type = queued.job.job_type();
            let description = queued.job.description();
            let priority = queued.job.priority();
            let source_id = queued.source_id;
            let retry_params_opt = queued.job.retry_params();
            let attempt = queued.job.attempt_count();

            let type_permit = if let Some(cfg) = type_configs.get(job_type) {
                match cfg.semaphore.clone().try_acquire_owned() {
                    Ok(p) => Some(p),
                    Err(_) => {
                        queue.lock().await.push(QueuedJob {
                            priority,
                            created_at: Instant::now(),
                            source_id,
                            job: queued.job,
                        });
                        drop(permit);
                        break;
                    }
                }
            } else {
                None
            };

            if let Some(sid) = source_id {
                let now = unix_now();
                let is_open = {
                    let mut entry = circuit_breakers.entry(sid).or_default();
                    entry.maybe_transition_to_half_open(now);
                    entry.is_open_at(now)
                };
                if is_open {
                    let job_id_str = job_id.to_string();
                    let msg = format!("Circuit breaker open for source {sid}");
                    let error_json = serde_json::json!({
                        "kind": "circuit_open",
                        "message": msg
                    })
                    .to_string();
                    let _ = sqlx::query!(
                        "UPDATE jobs SET status = 'failed', completed_at = ?, \
                         error_json = ?, params_json = NULL WHERE id = ?",
                        now,
                        error_json,
                        job_id_str
                    )
                    .execute(pool)
                    .await;
                    let _ = sse_tx.send(AppEvent::JobFailed {
                        job_id,
                        job_type: job_type.to_string(),
                        message: msg,
                        retryable: false,
                    });
                    drop(type_permit);
                    drop(permit);
                    notify.notify_one();
                    continue;
                }
            }

            let src_permit = if let Some(sid) = source_id {
                let sem = {
                    if let Some(s) = per_source_semaphores.get(&sid) {
                        Arc::clone(&*s)
                    } else {
                        let n = sqlx::query_scalar!(
                            "SELECT download_concurrency FROM sources WHERE id = ?",
                            sid
                        )
                        .fetch_optional(pool)
                        .await
                        .ok()
                        .flatten()
                        .flatten()
                        .map(|v| v as usize)
                        .unwrap_or(concurrency.per_source_download_concurrency);
                        let new_sem = Arc::new(Semaphore::new(n));
                        per_source_semaphores.insert(sid, Arc::clone(&new_sem));
                        new_sem
                    }
                };
                match sem.clone().try_acquire_owned() {
                    Ok(p) => Some(p),
                    Err(_) => {
                        queue.lock().await.push(QueuedJob {
                            priority,
                            created_at: Instant::now(),
                            source_id,
                            job: queued.job,
                        });
                        drop(type_permit);
                        drop(permit);
                        break;
                    }
                }
            } else {
                None
            };

            let job_cancel = CancellationToken::new();
            let progress = JobProgressReporter::new(job_id, job_type, sse_tx.clone());
            let ctx = JobContext {
                pool: pool.clone(),
                cancel: job_cancel.clone(),
                progress: progress.clone(),
                concurrency: JobConcurrencySnapshot {
                    page_concurrency: concurrency.page_concurrency,
                    per_source_download_concurrency: concurrency.per_source_download_concurrency,
                    scan_concurrency: concurrency.scan_concurrency,
                },
                svc: Arc::clone(svc_cell),
            };

            let now = unix_now();
            let job_id_str = job_id.to_string();
            let _ = sqlx::query!(
                "UPDATE jobs SET status = 'running', started_at = ? WHERE id = ?",
                now,
                job_id_str
            )
            .execute(pool)
            .await;
            metrics::gauge!("kani_jobs_running").set(active.len() as f64);

            let _ = sse_tx.send(AppEvent::JobStarted {
                job_id,
                job_type: job_type.to_string(),
                description: description.clone(),
            });

            active.insert(
                job_id,
                ActiveJobHandle {
                    job_id,
                    job_type,
                    description: description.clone(),
                    cancel: job_cancel,
                    progress,
                },
            );

            let pool_t = pool.clone();
            let sse_t = sse_tx.clone();
            let active_t = Arc::clone(active);
            let completion_t = completion_tx.clone();
            let notify_t = Arc::clone(notify);
            let registry_t = Arc::clone(registry);
            let queue_t = Arc::clone(queue);
            let cb_t = Arc::clone(circuit_breakers);

            tokio::spawn(async move {
                let _permit = permit;
                let _type_permit = type_permit;
                let _src_permit = src_permit;

                let result = queued.job.run_erased(ctx).await;
                let now = unix_now();
                let job_id_str = job_id.to_string();

                match result {
                    Ok(value) => {
                        if let Some(sid) = source_id
                            && let Some(mut cb) = cb_t.get_mut(&sid)
                        {
                            cb.record_success();
                            let state = cb.state.to_string();
                            let fc = cb.failure_count as i64;
                            let lf = cb.last_failure_at;
                            let nr = cb.next_retry_at;
                            drop(cb);
                            let _ = sqlx::query!(
                                "INSERT INTO source_circuit_breakers \
                                 (source_id, state, failure_count, last_failure_at, next_retry_at) \
                                 VALUES (?, ?, ?, ?, ?) \
                                 ON CONFLICT(source_id) DO UPDATE SET \
                                 state=excluded.state, failure_count=excluded.failure_count, \
                                 last_failure_at=excluded.last_failure_at, \
                                 next_retry_at=excluded.next_retry_at",
                                sid,
                                state,
                                fc,
                                lf,
                                nr,
                            )
                            .execute(&pool_t)
                            .await;
                        }
                        let result_json = serde_json::to_string(&value).ok();
                        let _ = sqlx::query!(
                            "UPDATE jobs SET status = 'completed', completed_at = ?, \
                             result_json = ?, params_json = NULL WHERE id = ?",
                            now,
                            result_json,
                            job_id_str
                        )
                        .execute(&pool_t)
                        .await;
                        let _ = sse_t.send(AppEvent::JobCompleted {
                            job_id,
                            job_type: job_type.to_string(),
                            description,
                        });
                    }
                    Err(JobError::Cancelled) => {
                        let _ = sqlx::query!(
                            "UPDATE jobs SET status = 'cancelled', completed_at = ?, \
                             params_json = NULL WHERE id = ?",
                            now,
                            job_id_str
                        )
                        .execute(&pool_t)
                        .await;
                        let _ = sse_t.send(AppEvent::JobCancelled {
                            job_id,
                            job_type: job_type.to_string(),
                        });
                    }
                    Err(e) => {
                        if let (Some(sid), JobError::Download(kind)) = (source_id, &e) {
                            let mut entry = cb_t.entry(sid).or_insert_with(|| {
                                crate::jobs::circuit_breaker::CircuitBreaker::new()
                            });
                            entry.record_failure(kind, now);
                            let state = entry.state.to_string();
                            let fc = entry.failure_count as i64;
                            let lf = entry.last_failure_at;
                            let nr = entry.next_retry_at;
                            drop(entry);
                            let _ = sqlx::query!(
                                "INSERT INTO source_circuit_breakers \
                                 (source_id, state, failure_count, last_failure_at, next_retry_at) \
                                 VALUES (?, ?, ?, ?, ?) \
                                 ON CONFLICT(source_id) DO UPDATE SET \
                                 state=excluded.state, failure_count=excluded.failure_count, \
                                 last_failure_at=excluded.last_failure_at, \
                                 next_retry_at=excluded.next_retry_at",
                                sid,
                                state,
                                fc,
                                lf,
                                nr,
                            )
                            .execute(&pool_t)
                            .await;
                        }

                        let retry_scheduled = if let JobError::Download(kind) = &e {
                            if let (Some(params), Some(policy)) =
                                (retry_params_opt, kind.retry_policy())
                            {
                                if attempt < policy.max_attempts {
                                    let delay = policy.delay_for_attempt(attempt);
                                    let pool_r = pool_t.clone();
                                    let queue_r = Arc::clone(&queue_t);
                                    let notify_r = Arc::clone(&notify_t);
                                    let registry_r = Arc::clone(&registry_t);
                                    let cb_r = Arc::clone(&cb_t);
                                    tokio::spawn(async move {
                                        tokio::time::sleep(delay).await;
                                        // Skip retry if circuit opened while we were waiting.
                                        if let Some(sid) = source_id {
                                            let is_open = cb_r
                                                .get(&sid)
                                                .map(|cb| cb.is_open_at(unix_now()))
                                                .unwrap_or(false);
                                            if is_open {
                                                return;
                                            }
                                        }
                                        match registry_r.deserialise(job_type, &params) {
                                            Ok(new_job) => {
                                                let new_id = new_job.id();
                                                let new_id_str = new_id.to_string();
                                                let new_priority = new_job.priority() as i64;
                                                let new_desc = new_job.description();
                                                let new_src_id = new_job.source_id();
                                                let t = unix_now();
                                                let priority_enum = new_job.priority();
                                                let _ = sqlx::query!(
                                                    "INSERT INTO jobs \
                                                     (id, job_type, status, priority, description, params_json, created_at) \
                                                     VALUES (?, ?, 'pending', ?, ?, ?, ?)",
                                                    new_id_str,
                                                    job_type,
                                                    new_priority,
                                                    new_desc,
                                                    params,
                                                    t,
                                                )
                                                .execute(&pool_r)
                                                .await;
                                                queue_r.lock().await.push(QueuedJob {
                                                    priority: priority_enum,
                                                    created_at: Instant::now(),
                                                    source_id: new_src_id,
                                                    job: new_job,
                                                });
                                                notify_r.notify_one();
                                            }
                                            Err(de) => {
                                                tracing::warn!(
                                                    "Retry deserialisation failed for {job_type}: {de}"
                                                );
                                            }
                                        }
                                    });
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        metrics::counter!("kani_jobs_failed_total", "kind" => job_type)
                            .increment(1);
                        let error_json = serde_json::to_string(&e).ok();
                        let _ = sqlx::query!(
                            "UPDATE jobs SET status = 'failed', completed_at = ?, \
                             error_json = ?, params_json = NULL WHERE id = ?",
                            now,
                            error_json,
                            job_id_str
                        )
                        .execute(&pool_t)
                        .await;
                        let _ = sse_t.send(AppEvent::JobFailed {
                            job_id,
                            job_type: job_type.to_string(),
                            message: e.to_string(),
                            retryable: retry_scheduled,
                        });
                    }
                }

                active_t.remove(&job_id);
                metrics::gauge!("kani_jobs_running").set(active_t.len() as f64);
                let _ = completion_t.send(job_id);
                notify_t.notify_one();

                let _ = Self::prune_history(&pool_t, max_history).await;
            });
        }
    }

    async fn drain_active(
        active: &Arc<DashMap<JobId, ActiveJobHandle>>,
        completion_tx: &tokio::sync::broadcast::Sender<JobId>,
        timeout: Duration,
    ) {
        let count = active.len();
        if count == 0 {
            return;
        }

        for entry in active.iter() {
            entry.value().cancel.cancel();
        }

        let mut rx = completion_tx.subscribe();
        let mut remaining = count;
        let deadline = tokio::time::Instant::now() + timeout;

        while remaining > 0 {
            let wait = deadline.saturating_duration_since(tokio::time::Instant::now());
            if wait.is_zero() {
                tracing::warn!(
                    "Drain timeout: {} job(s) still running; forcing shutdown",
                    remaining
                );
                break;
            }
            match tokio::time::timeout(wait, rx.recv()).await {
                Ok(Ok(_)) => {
                    remaining = remaining.saturating_sub(1);
                }
                _ => break,
            }
        }
    }

    pub async fn submit<J>(&self, job: J) -> Result<JobId>
    where
        J: BackgroundJob + serde::Serialize,
        J::Output: serde::Serialize + Send + 'static,
    {
        let job_id = job.id();
        let job_id_str = job_id.to_string();
        let params =
            serde_json::to_string(&job).map_err(|e| ServiceError::Internal(e.to_string()))?;
        let priority = job.priority() as i64;
        let description = job.description();
        let job_type = job.job_type();
        let now = unix_now();

        sqlx::query!(
            "INSERT INTO jobs (id, job_type, status, priority, description, params_json, created_at) \
             VALUES (?, ?, 'pending', ?, ?, ?, ?)",
            job_id_str,
            job_type,
            priority,
            description,
            params,
            now
        )
        .execute(&self.pool)
        .await?;

        let source_id = job.source_id();
        self.queue.lock().await.push(QueuedJob {
            priority: job.priority(),
            created_at: Instant::now(),
            source_id,
            job: Box::new(job),
        });

        self.new_job_notify.notify_one();
        Ok(job_id)
    }

    /// Holds a queued job so the scheduler stops considering it.
    ///
    /// Pending only. A running job cannot be paused: stopping it means
    /// cancelling it, and `cancel` deliberately nulls `params_json`, so there
    /// would be nothing left to resume from. Refusing is honest — the caller
    /// can cancel and re-submit if that is what they meant.
    pub async fn pause(&self, job_id: JobId) -> Result<()> {
        if self.active.contains_key(&job_id) {
            return Err(ServiceError::Conflict(format!(
                "Job {job_id} is already running and cannot be paused; cancel it instead"
            )));
        }

        let mut q = self.queue.lock().await;
        let heap = std::mem::take(&mut *q);
        let mut found = false;
        for item in heap {
            if item.job.id() == job_id {
                found = true;
            } else {
                q.push(item);
            }
        }
        drop(q);

        let job_id_str = job_id.to_string();
        let row = sqlx::query!("SELECT status FROM jobs WHERE id = ?", job_id_str)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Job {job_id} not found")))?;
        if row.status != "pending" {
            return Err(ServiceError::Conflict(format!(
                "Job {job_id} is '{}', not pending",
                row.status
            )));
        }

        sqlx::query!("UPDATE jobs SET status = 'paused' WHERE id = ?", job_id_str)
            .execute(&self.pool)
            .await?;
        tracing::info!("Paused job {job_id} (was queued: {found})");
        Ok(())
    }

    /// Returns a paused job to the queue, rebuilt from its stored params by the
    /// same registry path that restores jobs after a restart.
    pub async fn resume(&self, job_id: JobId) -> Result<()> {
        let job_id_str = job_id.to_string();
        let row = sqlx::query!(
            "SELECT job_type, priority, params_json FROM jobs WHERE id = ? AND status = 'paused'",
            job_id_str
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Job {job_id} is not paused")))?;

        let params = row.params_json.ok_or_else(|| {
            ServiceError::Conflict(format!(
                "Job {job_id} has no stored parameters to resume from"
            ))
        })?;
        let job = self
            .registry
            .deserialise(&row.job_type, &params)
            .map_err(|e| ServiceError::Internal(format!("Cannot rebuild job {job_id}: {e}")))?;

        sqlx::query!(
            "UPDATE jobs SET status = 'pending' WHERE id = ?",
            job_id_str
        )
        .execute(&self.pool)
        .await?;

        let source_id = job.source_id();
        self.queue.lock().await.push(QueuedJob {
            priority: JobPriority::from_i64(row.priority),
            created_at: Instant::now(),
            source_id,
            job,
        });
        self.new_job_notify.notify_one();
        Ok(())
    }

    pub async fn cancel(&self, job_id: JobId) -> Result<()> {
        if let Some(handle) = self.active.get(&job_id) {
            handle.cancel.cancel();
            return Ok(());
        }

        let mut q = self.queue.lock().await;
        let heap = std::mem::take(&mut *q);
        let mut cancelled_job: Option<Box<dyn ErasedJob>> = None;
        for item in heap {
            if item.job.id() == job_id {
                cancelled_job = Some(item.job);
            } else {
                q.push(item);
            }
        }
        drop(q);

        let job_id_str = job_id.to_string();

        if let Some(job) = cancelled_job {
            job.on_cancel().await;
            let now = unix_now();
            sqlx::query!(
                "UPDATE jobs SET status = 'cancelled', completed_at = ?, params_json = NULL \
                 WHERE id = ?",
                now,
                job_id_str
            )
            .execute(&self.pool)
            .await?;
            let _ = self.sse_tx.send(AppEvent::JobCancelled {
                job_id,
                job_type: String::new(),
            });
            return Ok(());
        }

        let row = sqlx::query!("SELECT status FROM jobs WHERE id = ?", job_id_str)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            None => Err(ServiceError::NotFound(format!("Job {job_id} not found"))),
            Some(r) if matches!(r.status.as_str(), "cancelled" | "completed" | "failed") => {
                Err(ServiceError::Conflict(format!(
                    "Job {job_id} is already in terminal state '{}'",
                    r.status
                )))
            }
            _ => {
                let now = unix_now();
                sqlx::query!(
                    "UPDATE jobs SET status = 'cancelled', completed_at = ?, params_json = NULL \
                     WHERE id = ?",
                    now,
                    job_id_str
                )
                .execute(&self.pool)
                .await?;
                Ok(())
            }
        }
    }

    pub async fn status(&self, job_id: JobId) -> Result<JobStatus> {
        let live_progress = if let Some(handle) = self.active.get(&job_id) {
            let progress = handle.progress.clone();
            drop(handle);
            let p = progress.current().await;
            Some(serde_json::to_value(&p).unwrap_or_default())
        } else {
            None
        };

        let job_id_str = job_id.to_string();
        let row = sqlx::query!(
            "SELECT id, status, completed_at, progress_json, result_json, error_json \
             FROM jobs WHERE id = ?",
            job_id_str
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Job {job_id} not found")))?;

        let progress = live_progress.or_else(|| {
            row.progress_json
                .and_then(|s| serde_json::from_str(&s).ok())
        });

        Ok(JobStatus {
            id: row.id,
            status: row.status,
            completed_at: row.completed_at,
            progress,
            result: row.result_json.and_then(|s| serde_json::from_str(&s).ok()),
            error: row.error_json.and_then(|s| serde_json::from_str(&s).ok()),
        })
    }

    pub async fn list_jobs(&self, filter: JobListFilter) -> Result<JobListPage> {
        let limit = filter.limit.clamp(1, 200);
        let offset = filter.offset.max(0);

        let mut qb = sqlx::QueryBuilder::new(
            "SELECT id, job_type, status, priority, description, created_at, \
             started_at, completed_at, progress_json, error_json, user_id, \
             COUNT(*) OVER() AS total_count \
             FROM jobs WHERE 1=1",
        );
        if let Some(ref jt) = filter.job_type {
            qb.push(" AND job_type = ");
            qb.push_bind(jt);
        }
        if !filter.statuses.is_empty() {
            qb.push(" AND status IN (");
            let mut sep = qb.separated(", ");
            for s in &filter.statuses {
                sep.push_bind(s);
            }
            qb.push(")");
        }
        if let Some(uid) = filter.user_id {
            qb.push(" AND user_id = ");
            qb.push_bind(uid);
        }
        qb.push(" ORDER BY created_at DESC LIMIT ");
        qb.push_bind(limit);
        qb.push(" OFFSET ");
        qb.push_bind(offset);

        let rows = qb.build().fetch_all(&self.pool).await?;

        use sqlx::Row;
        let total = rows
            .first()
            .map(|r| r.get::<i64, _>("total_count"))
            .unwrap_or(0);

        let jobs = rows
            .iter()
            .map(|r| JobSummary {
                id: r.get("id"),
                job_type: r.get("job_type"),
                status: r.get("status"),
                priority: r.get("priority"),
                description: r.get("description"),
                created_at: r.get("created_at"),
                started_at: r.get("started_at"),
                completed_at: r.get("completed_at"),
                progress: r
                    .get::<Option<String>, _>("progress_json")
                    .and_then(|s| serde_json::from_str(&s).ok()),
                error: r
                    .get::<Option<String>, _>("error_json")
                    .and_then(|s| serde_json::from_str(&s).ok()),
                user_id: r.get("user_id"),
            })
            .collect();

        let job_types = sqlx::query_scalar!("SELECT DISTINCT job_type FROM jobs ORDER BY job_type")
            .fetch_all(&self.pool)
            .await?;

        Ok(JobListPage {
            jobs,
            total,
            job_types,
        })
    }

    /// Trims completed, failed, and cancelled jobs to the newest `max_history`.
    /// Runs after every job completes, so the `jobs` table cannot grow without bound.
    pub(crate) async fn prune_history(
        pool: &sqlx::sqlite::SqlitePool,
        max_history: usize,
    ) -> Result<u64> {
        let max = max_history as i64;
        let result = sqlx::query!(
            "DELETE FROM jobs \
             WHERE status IN ('completed','failed','cancelled') \
             AND id NOT IN (\
                 SELECT id FROM jobs \
                 WHERE status IN ('completed','failed','cancelled') \
                 ORDER BY completed_at DESC \
                 LIMIT ?\
             )",
            max
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn drain(&self, timeout: Duration) {
        Self::drain_active(&self.active, &self.completion_tx, timeout).await;
    }

    pub(crate) fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn active_job_summaries(&self) -> Vec<serde_json::Value> {
        self.active
            .iter()
            .map(|e| {
                let h = e.value();
                serde_json::json!({
                    "id": h.job_id.to_string(),
                    "job_type": h.job_type,
                    "description": h.description,
                })
            })
            .collect()
    }

    pub fn circuit_state(&self, source_id: i64) -> Option<String> {
        self.circuit_breakers
            .get(&source_id)
            .map(|cb| cb.state.to_string())
    }

    pub(crate) fn invalidate_source_semaphore(&self, source_id: i64) {
        self.per_source_semaphores.remove(&source_id);
    }

    pub(crate) fn invalidate_all_source_semaphores(&self) {
        self.per_source_semaphores.clear();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn priority_queue_orders_high_before_normal() {
        let mut heap = BinaryHeap::new();
        heap.push(QueuedJob {
            priority: JobPriority::Normal,
            created_at: Instant::now(),
            source_id: None,
            job: Box::new(NoopJob::new("normal")),
        });
        heap.push(QueuedJob {
            priority: JobPriority::High,
            created_at: Instant::now(),
            source_id: None,
            job: Box::new(NoopJob::new("high")),
        });
        assert_eq!(heap.pop().unwrap().priority, JobPriority::High);
    }

    #[test]
    fn priority_queue_breaks_ties_by_earlier_created_at() {
        let t1 = Instant::now();
        let t2 = t1 + Duration::from_secs(1);
        let mut heap = BinaryHeap::new();
        heap.push(QueuedJob {
            priority: JobPriority::Normal,
            created_at: t2,
            source_id: None,
            job: Box::new(NoopJob::new("later")),
        });
        heap.push(QueuedJob {
            priority: JobPriority::Normal,
            created_at: t1,
            source_id: None,
            job: Box::new(NoopJob::new("earlier")),
        });
        let first = heap.pop().unwrap();
        assert_eq!(first.created_at, t1);
    }

    #[test]
    fn priority_queue_low_last() {
        let mut heap = BinaryHeap::new();
        heap.push(QueuedJob {
            priority: JobPriority::Low,
            created_at: Instant::now(),
            source_id: None,
            job: Box::new(NoopJob::new("low")),
        });
        heap.push(QueuedJob {
            priority: JobPriority::High,
            created_at: Instant::now(),
            source_id: None,
            job: Box::new(NoopJob::new("high")),
        });
        heap.push(QueuedJob {
            priority: JobPriority::Normal,
            created_at: Instant::now(),
            source_id: None,
            job: Box::new(NoopJob::new("normal")),
        });
        assert_eq!(heap.pop().unwrap().priority, JobPriority::High);
        assert_eq!(heap.pop().unwrap().priority, JobPriority::Normal);
        assert_eq!(heap.pop().unwrap().priority, JobPriority::Low);
    }

    struct NoopJob {
        id: JobId,
        name: &'static str,
    }

    impl NoopJob {
        fn new(name: &'static str) -> Self {
            Self {
                id: JobId::new_v4(),
                name,
            }
        }
    }

    #[async_trait::async_trait]
    impl BackgroundJob for NoopJob {
        const JOB_TYPE: &'static str = "test_noop";
        type Output = ();
        fn id(&self) -> JobId {
            self.id
        }
        fn description(&self) -> String {
            self.name.into()
        }
        async fn run(self: Box<Self>, _ctx: JobContext) -> Result<(), JobError> {
            Ok(())
        }
    }
}
