use std::sync::Arc;

use kani_core::downloader::DownloadError;
use kani_shared::extension::ExtensionErrorKind;
use kani_shared::types::DownloadStatus;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::events::{AppEvent, RefreshProgressEvent};
use crate::ids::{ChapterId, MangaId};
use crate::jobs::error::{DownloadErrorKind, JobError};
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

pub(crate) fn classify_download_error(err: DownloadError) -> DownloadErrorKind {
    match err {
        DownloadError::Cancelled => DownloadErrorKind::Cancelled,
        DownloadError::Extension {
            kind,
            message,
            retry_after_secs,
        } => match kind {
            ExtensionErrorKind::Network
            | ExtensionErrorKind::Timeout
            | ExtensionErrorKind::Updating => DownloadErrorKind::Network { retryable: true },
            ExtensionErrorKind::RateLimited => DownloadErrorKind::RateLimited {
                retry_after_secs: retry_after_secs.map(u64::from),
            },
            ExtensionErrorKind::NotFound | ExtensionErrorKind::ContentUnavailable => {
                DownloadErrorKind::NotFound
            }
            ExtensionErrorKind::Auth => DownloadErrorKind::AuthRequired,
            ExtensionErrorKind::Parse | ExtensionErrorKind::InvalidInput => {
                DownloadErrorKind::ParseError { message }
            }
            ExtensionErrorKind::Internal => DownloadErrorKind::ExtensionError { message },
            ExtensionErrorKind::Unknown => DownloadErrorKind::Unknown { message },
        },
        DownloadError::Io(e) => DownloadErrorKind::StorageError {
            path: String::new(),
            message: e.to_string(),
        },
        // The status is known exactly, so classify on it — and pass on the
        // server's own `Retry-After` instead of discarding it and falling back
        // to our backoff guess.
        DownloadError::PageHttp {
            status,
            retry_after_secs,
            ..
        } => match status {
            429 => DownloadErrorKind::RateLimited { retry_after_secs },
            401 | 403 => DownloadErrorKind::AuthRequired,
            404 | 410 => DownloadErrorKind::NotFound,
            s if (500..600).contains(&s) => DownloadErrorKind::Network { retryable: true },
            _ => DownloadErrorKind::Network { retryable: false },
        },
        DownloadError::PageFetch(msg) => {
            let lower = msg.to_lowercase();
            if lower.contains("429") || lower.contains("rate limit") {
                DownloadErrorKind::RateLimited {
                    retry_after_secs: None,
                }
            } else if lower.contains("401") || lower.contains("403") || lower.contains("auth") {
                DownloadErrorKind::AuthRequired
            } else if lower.contains("404") || lower.contains("not found") {
                DownloadErrorKind::NotFound
            } else {
                DownloadErrorKind::Network { retryable: true }
            }
        }
    }
}

/// Core chapter download logic: checks status, reads resume_offset, builds task,
/// calls the downloader, and writes the terminal DB state. Used by both
/// `ChapterDownloadJob` and `MangaDownloadAllJob`.
pub(crate) async fn run_chapter_download(
    svc: &crate::service::AppService,
    chapter_id: ChapterId,
    job_id: Option<String>,
    cancel: tokio_util::sync::CancellationToken,
    on_page: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<(), JobError> {
    let status: Option<i64> = sqlx::query_scalar!(
        "SELECT download_status FROM chapters WHERE id = ?",
        chapter_id
    )
    .fetch_optional(&svc.db)
    .await?;

    match status {
        None => {
            return Err(JobError::ChapterNotFound(format!(
                "Chapter {} was deleted",
                chapter_id
            )));
        }
        Some(s) if s == DownloadStatus::Complete as i64 => return Ok(()),
        _ => {}
    }

    let resume_offset: i64 = sqlx::query_scalar!(
        "SELECT resume_offset FROM chapters WHERE id = ?",
        chapter_id
    )
    .fetch_one(&svc.db)
    .await
    .unwrap_or(0);

    let task = svc
        .build_download_task(chapter_id)
        .await
        .map_err(|e| JobError::Internal(e.to_string()))?;

    let completed = (
        task.chapter_id,
        task.name.clone(),
        task.manga_id,
        task.manga_title.clone(),
    );

    let result = svc
        .downloader
        .download_chapter_direct(task, cancel, job_id, on_page)
        .await;

    match result {
        Ok(outcome) => {
            let now = time::OffsetDateTime::now_utc();
            sqlx::query!(
                "UPDATE chapters SET download_status = ?, resume_offset = 0, download_error = NULL, downloaded_at = ? WHERE id = ?",
                DownloadStatus::Complete,
                now,
                chapter_id,
            )
            .execute(&svc.db)
            .await?;

            let (id, name, manga_id, manga_title) = completed;
            svc.downloader.emit_chapter_completed(
                id,
                name,
                manga_id,
                manga_title,
                outcome.successful_pages,
            );

            // Clear a renamed chapter's stored path before resolving the downloader's derived path.
            let _ = svc.clear_chapter_manifest(chapter_id).await;
            if let Ok(info) = svc.chapter_cbz_path(chapter_id).await {
                svc.record_chapter_manifest(chapter_id, info.path).await;
            }
            Ok(())
        }
        Err(DownloadError::Cancelled) => {
            sqlx::query!(
                "UPDATE chapters SET resume_offset = ?, download_status = ? WHERE id = ?",
                resume_offset,
                DownloadStatus::Pending,
                chapter_id,
            )
            .execute(&svc.db)
            .await?;
            Err(JobError::Cancelled)
        }
        Err(e) => {
            let kind = classify_download_error(e);
            let kind_json = serde_json::to_string(&kind).unwrap_or_default();
            let is_storage = matches!(kind, DownloadErrorKind::StorageError { .. });
            let new_offset = if is_storage { 0i64 } else { resume_offset };
            sqlx::query!(
                "UPDATE chapters SET download_status = ?, download_error = ?, resume_offset = ? WHERE id = ?",
                DownloadStatus::Pending,
                kind_json,
                new_offset,
                chapter_id,
            )
            .execute(&svc.db)
            .await?;
            Err(JobError::Download(kind))
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ChapterDownloadJob {
    id: JobId,
    pub chapter_id: i64,
    pub manga_id: i64,
    pub manga_title: String,
    pub source_id: i64,
    pub attempt: u32,
}

impl ChapterDownloadJob {
    pub fn new(chapter_id: i64, manga_id: i64, manga_title: String, source_id: i64) -> Self {
        Self {
            id: JobId::new_v4(),
            chapter_id,
            manga_id,
            manga_title,
            source_id,
            attempt: 0,
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for ChapterDownloadJob {
    const JOB_TYPE: &'static str = "chapter_download";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        format!(
            "Download chapter {} ({})",
            self.chapter_id, self.manga_title
        )
    }

    fn priority(&self) -> JobPriority {
        if self.attempt == 0 {
            JobPriority::High
        } else {
            JobPriority::Normal
        }
    }

    fn source_id(&self) -> Option<i64> {
        Some(self.source_id)
    }

    fn attempt_count(&self) -> u32 {
        self.attempt
    }

    fn retry_params(&self) -> Option<String> {
        let next = ChapterDownloadJob {
            id: crate::jobs::framework::JobId::new_v4(),
            attempt: self.attempt + 1,
            ..self.clone()
        };
        serde_json::to_string(&next).ok()
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        let chapter_id = ChapterId(self.chapter_id);
        let job_id = Some(self.id.to_string());

        let progress = ctx.progress.clone();
        let on_page = move |current: u64, total: u64| {
            let progress = progress.clone();
            tokio::spawn(async move {
                progress.report(current, total, "").await;
            });
        };

        run_chapter_download(&svc, chapter_id, job_id, ctx.cancel.clone(), on_page).await
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MangaDownloadAllJob {
    id: JobId,
    pub manga_id: i64,
    pub manga_title: String,
    pub source_id: i64,
    pub preferred_only: bool,
}

impl MangaDownloadAllJob {
    pub fn new(manga_id: i64, manga_title: String, source_id: i64, preferred_only: bool) -> Self {
        Self {
            id: JobId::new_v4(),
            manga_id,
            manga_title,
            source_id,
            preferred_only,
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for MangaDownloadAllJob {
    const JOB_TYPE: &'static str = "manga_download_all";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        format!("Download all chapters ({})", self.manga_title)
    }

    fn source_id(&self) -> Option<i64> {
        Some(self.source_id)
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        let manga_id = MangaId(self.manga_id);

        if ctx.cancel.is_cancelled() {
            return Err(JobError::Cancelled);
        }

        let candidate_ids: Vec<ChapterId> = sqlx::query_scalar!(
            "SELECT id FROM chapters \
             WHERE manga_id = ? AND download_status = ? AND is_orphaned = 0",
            manga_id,
            DownloadStatus::Pending,
        )
        .fetch_all(&ctx.pool)
        .await?
        .into_iter()
        .map(ChapterId)
        .collect();

        if candidate_ids.is_empty() {
            return Ok(());
        }

        let preferred_ids = if self.preferred_only {
            let filtered = svc.filter_chapters_by_rules(manga_id, candidate_ids).await;
            if filtered.is_empty() {
                return Ok(());
            }
            filtered
        } else {
            candidate_ids
        };

        let mut claimed: Vec<ChapterId> = Vec::with_capacity(preferred_ids.len());
        for id in preferred_ids {
            let res: Option<i64> = sqlx::query_scalar!(
                "UPDATE chapters SET download_status = ? \
                 WHERE id = ? AND download_status = ? \
                 RETURNING id",
                DownloadStatus::InProgress,
                id,
                DownloadStatus::Pending,
            )
            .fetch_optional(&ctx.pool)
            .await?;
            if let Some(claimed_id) = res {
                claimed.push(ChapterId(claimed_id));
            }
        }

        if claimed.is_empty() {
            return Ok(());
        }

        let total = claimed.len();
        ctx.progress.report(0, total as u64, "").await;

        let concurrency = ctx.concurrency.per_source_download_concurrency.max(1);
        let semaphore = Arc::new(Semaphore::new(concurrency));
        let mut join_set: JoinSet<(ChapterId, Result<(), JobError>)> = JoinSet::new();
        let job_id_str = self.id.to_string();

        for chapter_id in claimed {
            let svc2 = svc.clone();
            let sem = Arc::clone(&semaphore);
            let cancel = ctx.cancel.child_token();
            let jid = job_id_str.clone();
            let pool = ctx.pool.clone();
            join_set.spawn(async move {
                let _permit = sem.acquire_owned().await.expect("semaphore closed");
                if cancel.is_cancelled() {
                    let _ = sqlx::query!(
                        "UPDATE chapters SET download_status = ? WHERE id = ?",
                        DownloadStatus::Pending,
                        chapter_id,
                    )
                    .execute(&pool)
                    .await;
                    return (chapter_id, Err(JobError::Cancelled));
                }
                let result =
                    run_chapter_download(&svc2, chapter_id, Some(jid), cancel, |_, _| {}).await;
                (chapter_id, result)
            });
        }

        let mut done = 0usize;

        while let Some(res) = join_set.join_next().await {
            let (chapter_id, result) = res.map_err(|e| JobError::Internal(e.to_string()))?;

            done += 1;

            if let Err(JobError::Download(ref kind)) = result
                && kind.is_retryable()
                && let Ok(Some(row)) = sqlx::query!(
                    "SELECT c.manga_id, m.source_id, m.name as manga_title \
                     FROM chapters c JOIN manga m ON c.manga_id = m.id WHERE c.id = ?",
                    chapter_id,
                )
                .fetch_optional(&ctx.pool)
                .await
            {
                let retry_job = ChapterDownloadJob::new(
                    chapter_id.0,
                    row.manga_id,
                    row.manga_title,
                    row.source_id,
                );
                let _ = svc.job_manager.submit(retry_job).await;
            }

            ctx.progress.report(done as u64, total as u64, "").await;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SourceScanJob {
    id: JobId,
    pub manga_id: i64,
    pub manga_title: String,
    pub source_id: i64,
    pub trigger: String,
}

impl SourceScanJob {
    pub fn new(manga_id: i64, manga_title: String, source_id: i64, trigger: String) -> Self {
        Self {
            id: JobId::new_v4(),
            manga_id,
            manga_title,
            source_id,
            trigger,
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for SourceScanJob {
    const JOB_TYPE: &'static str = "source_scan";
    type Output = usize;

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        format!("Scan {} for new chapters", self.manga_title)
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<usize, JobError> {
        let svc = ctx.service();
        let manga_id = MangaId::from(self.manga_id);

        if ctx.cancel.is_cancelled() {
            return Err(JobError::Cancelled);
        }

        let chapter_ids = svc
            .scan_for_new_chapters(manga_id)
            .await
            .map_err(|e| JobError::Internal(e.to_string()))?;

        ctx.progress.report(1, 1, "").await;

        Ok(chapter_ids.len())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct LibraryScanJob {
    id: JobId,
    pub manga_ids: Vec<i64>,
    pub trigger: String,
}

impl LibraryScanJob {
    pub fn new(manga_ids: Vec<i64>, trigger: String) -> Self {
        Self {
            id: JobId::new_v4(),
            manga_ids,
            trigger,
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for LibraryScanJob {
    const JOB_TYPE: &'static str = "library_scan";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        format!("Scan library ({} manga)", self.manga_ids.len())
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        let manga_ids: Vec<MangaId> = self.manga_ids.iter().map(|&id| MangaId::from(id)).collect();
        let total = manga_ids.len();
        let scan_concurrency = ctx.concurrency.scan_concurrency;

        let manga_names: std::collections::HashMap<MangaId, String> = {
            let ids_json = serde_json::to_string(&self.manga_ids).unwrap_or_default();
            sqlx::query!(
                "SELECT id, name FROM manga WHERE id IN (SELECT value FROM json_each(?))",
                ids_json
            )
            .fetch_all(&ctx.pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| (MangaId::from(r.id), r.name))
            .collect()
        };

        let _ = svc
            .refresh_tx
            .send(AppEvent::Refresh(RefreshProgressEvent::Started {
                total,
                manga_ids: manga_ids.clone(),
            }));

        let mut join_set: JoinSet<(MangaId, String, Result<usize, String>)> = JoinSet::new();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(scan_concurrency));
        let mut completed = 0usize;
        let mut failed = 0usize;
        let mut iter = manga_ids.into_iter().peekable();

        loop {
            while join_set.len() < scan_concurrency {
                let Some(manga_id) = iter.next() else { break };
                let svc2 = svc.clone();
                let sem2 = Arc::clone(&semaphore);
                let name = manga_names
                    .get(&manga_id)
                    .cloned()
                    .unwrap_or_else(|| manga_id.to_string());
                let cancel = ctx.cancel.clone();
                join_set.spawn(async move {
                    let _permit = sem2.acquire_owned().await.expect("semaphore closed");
                    if cancel.is_cancelled() {
                        return (manga_id, name, Err("cancelled".to_string()));
                    }
                    let result = svc2
                        .scan_for_new_chapters(manga_id)
                        .await
                        .map(|ids| ids.len())
                        .map_err(|e| e.to_string());
                    (manga_id, name, result)
                });
            }

            let Some(res) = join_set.join_next().await else {
                break;
            };
            let (manga_id, manga_name, scan_result) =
                res.map_err(|e| JobError::Internal(format!("scan task panicked: {e}")))?;

            completed += 1;
            let (success, new_chapters) = match scan_result {
                Ok(count) => (true, count as u32),
                Err(_) => {
                    failed += 1;
                    (false, 0)
                }
            };

            let _ = svc
                .refresh_tx
                .send(AppEvent::Refresh(RefreshProgressEvent::MangaRefreshed {
                    manga_id,
                    manga_name,
                    completed,
                    total,
                    success,
                    new_chapters,
                }));

            ctx.progress
                .report(completed as u64, total as u64, "")
                .await;

            if ctx.cancel.is_cancelled() {
                break;
            }
        }

        let _ = svc
            .refresh_tx
            .send(AppEvent::Refresh(RefreshProgressEvent::Completed {
                total: completed,
                failed,
            }));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::jobs::circuit_breaker::{CircuitBreaker, CircuitState};

    fn ext(kind: ExtensionErrorKind) -> DownloadError {
        DownloadError::Extension {
            kind,
            message: "boom".to_string(),
            retry_after_secs: None,
        }
    }

    #[test]
    fn transient_extension_kinds_map_to_retryable_network() {
        for kind in [
            ExtensionErrorKind::Network,
            ExtensionErrorKind::Timeout,
            ExtensionErrorKind::Updating,
        ] {
            let mapped = classify_download_error(ext(kind));
            assert!(
                matches!(mapped, DownloadErrorKind::Network { retryable: true }),
                "{kind:?} should map to retryable Network, got {mapped:?}"
            );
            assert!(mapped.is_retryable());
        }
    }

    #[test]
    fn rate_limited_extension_maps_to_rate_limited() {
        let mapped = classify_download_error(ext(ExtensionErrorKind::RateLimited));
        assert!(matches!(
            mapped,
            DownloadErrorKind::RateLimited {
                retry_after_secs: None
            }
        ));
        assert!(mapped.is_retryable());
    }

    #[test]
    fn rate_limited_extension_carries_retry_after_to_the_policy() {
        let err = DownloadError::Extension {
            kind: ExtensionErrorKind::RateLimited,
            message: "slow down".to_string(),
            retry_after_secs: Some(45),
        };
        assert!(matches!(
            classify_download_error(err),
            DownloadErrorKind::RateLimited {
                retry_after_secs: Some(45)
            }
        ));
    }

    #[test]
    fn permanent_extension_kinds_are_not_retryable() {
        for kind in [
            ExtensionErrorKind::NotFound,
            ExtensionErrorKind::ContentUnavailable,
            ExtensionErrorKind::Auth,
        ] {
            let mapped = classify_download_error(ext(kind));
            assert!(
                !mapped.is_retryable(),
                "{kind:?} should not be retryable, got {mapped:?}"
            );
        }
    }

    #[test]
    fn parse_and_invalid_input_map_to_parse_error() {
        for kind in [ExtensionErrorKind::Parse, ExtensionErrorKind::InvalidInput] {
            assert!(matches!(
                classify_download_error(ext(kind)),
                DownloadErrorKind::ParseError { .. }
            ));
        }
    }

    #[test]
    fn internal_and_unknown_map_to_soft_errors() {
        assert!(matches!(
            classify_download_error(ext(ExtensionErrorKind::Internal)),
            DownloadErrorKind::ExtensionError { .. }
        ));
        assert!(matches!(
            classify_download_error(ext(ExtensionErrorKind::Unknown)),
            DownloadErrorKind::Unknown { .. }
        ));
    }

    #[test]
    fn transient_extension_error_trips_circuit_but_not_found_does_not() {
        let mut cb = CircuitBreaker::new();
        let transient = classify_download_error(ext(ExtensionErrorKind::Updating));
        for _ in 0..5 {
            cb.record_failure(&transient, 0);
        }
        assert!(
            cb.is_open_at(0),
            "repeated transient extension failures should open the circuit"
        );

        let mut cb2 = CircuitBreaker::new();
        let permanent = classify_download_error(ext(ExtensionErrorKind::NotFound));
        for _ in 0..10 {
            cb2.record_failure(&permanent, 0);
        }
        assert!(
            !cb2.is_open_at(0),
            "not-found should never count toward the circuit"
        );
        assert_eq!(cb2.state, CircuitState::Closed);
    }
}
