use crate::ids::MangaId;
use crate::jobs::error::{DownloadErrorKind, JobError};
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};
use crate::service::import::resolve::Resolution;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ImportResolveJob {
    id: JobId,
    /// The source whose queued manga this run covers, or every source when absent.
    #[serde(default)]
    pub source_id: Option<i64>,
    #[serde(default)]
    attempt: u32,
}

impl ImportResolveJob {
    pub fn new(source_id: Option<i64>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            source_id,
            attempt: 0,
        }
    }
}

#[derive(serde::Serialize)]
pub struct ImportResolveOutcome {
    pub already_valid: u32,
    pub relinked: u32,
    pub unresolved: u32,
    pub deferred: u32,
}

#[async_trait::async_trait]
impl BackgroundJob for ImportResolveJob {
    const JOB_TYPE: &'static str = "import_resolve";
    type Output = ImportResolveOutcome;

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        "Link imported manga to their source".to_string()
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    fn source_id(&self) -> Option<i64> {
        self.source_id
    }

    fn attempt_count(&self) -> u32 {
        self.attempt
    }

    fn retry_params(&self) -> Option<String> {
        serde_json::to_string(&Self {
            id: uuid::Uuid::new_v4(),
            source_id: self.source_id,
            attempt: self.attempt + 1,
        })
        .ok()
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<ImportResolveOutcome, JobError> {
        let svc = ctx.service();

        // Read the queue from the database rather than a list captured at submit
        // time, so a retry picks up exactly what is still unlinked and a restart
        // loses nothing.
        let queued = sqlx::query_as::<_, (i64, i64, String, String)>(
            "SELECT m.id, m.source_id, m.source_manga_id, m.name \
             FROM manga_import_links l JOIN manga m ON m.id = l.manga_id \
             WHERE l.status = 'pending' AND m.deleted_at IS NULL \
               AND (?1 IS NULL OR m.source_id = ?1) \
             ORDER BY m.id",
        )
        .bind(self.source_id)
        .fetch_all(&ctx.pool)
        .await?;

        let total = queued.len() as u64;
        let mut outcome = ImportResolveOutcome {
            already_valid: 0,
            relinked: 0,
            unresolved: 0,
            deferred: 0,
        };
        let mut changed = false;

        for (idx, (manga_id, source_id, stored_id, title)) in queued.into_iter().enumerate() {
            if ctx.cancel.is_cancelled() {
                return Err(JobError::Cancelled);
            }

            ctx.progress
                .report(idx as u64, total, &format!("Linking {title}"))
                .await;

            match svc
                .resolve_imported_manga(source_id, MangaId(manga_id), &stored_id, &title)
                .await
            {
                Ok(Resolution::AlreadyValid) => {
                    outcome.already_valid += 1;
                    changed = true;
                }
                Ok(Resolution::Relinked(_)) => {
                    outcome.relinked += 1;
                    changed = true;
                }
                Ok(Resolution::Deferred(reason)) => {
                    outcome.deferred += 1;
                    tracing::info!(
                        manga_id,
                        title,
                        reason,
                        "linking deferred; source unavailable"
                    );
                    continue;
                }
                Ok(Resolution::Unresolved(reason)) => {
                    outcome.unresolved += 1;
                    changed = true;
                    tracing::info!(
                        manga_id,
                        title,
                        reason,
                        "imported manga could not be linked"
                    );
                    continue;
                }
                Err(e) => {
                    outcome.deferred += 1;
                    tracing::warn!(manga_id, %e, "linking an imported manga failed");
                    continue;
                }
            }

            // Only now can chapters be fetched: before this the source was being
            // asked about an id it does not recognise.
            if let Err(e) = svc.fetch_and_store_chapters_silent(MangaId(manga_id)).await {
                tracing::warn!(manga_id, %e, "chapter fetch after linking failed");
                continue;
            }

            match svc.apply_stored_import_progress(MangaId(manga_id)).await {
                Ok(0) => {}
                Ok(unmatched) => {
                    tracing::info!(
                        manga_id,
                        title,
                        unmatched,
                        "imported read progress matched no chapter"
                    );
                }
                Err(e) => tracing::warn!(manga_id, %e, "applying imported read progress failed"),
            }
        }

        ctx.progress.report(total, total, "Linked").await;
        if changed {
            svc.invalidate_library();
        }

        if outcome.deferred > 0 {
            return Err(JobError::Download(DownloadErrorKind::Network {
                retryable: true,
            }));
        }
        Ok(outcome)
    }
}
