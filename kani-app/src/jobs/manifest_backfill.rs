use crate::ids::ChapterId;
use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct ManifestBackfillJob {
    id: JobId,
}

impl ManifestBackfillJob {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
        }
    }
}

impl Default for ManifestBackfillJob {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BackgroundJob for ManifestBackfillJob {
    const JOB_TYPE: &'static str = "manifest_backfill";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        "Backfill chapter content hashes".to_string()
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();

        let ids: Vec<i64> = sqlx::query_scalar!(
            "SELECT id FROM chapters WHERE download_status = 2 AND file_path IS NULL"
        )
        .fetch_all(&svc.db_read)
        .await?;

        let total = ids.len() as u64;
        if total == 0 {
            return Ok(());
        }
        tracing::info!("Backfilling content hashes for {total} chapter(s)");

        for (done, id) in ids.into_iter().enumerate() {
            let chapter_id = ChapterId(id);

            // Rows that were already rename-orphaned cannot be resolved. Leave
            // them NULL so the first scrub reports them as missing rather than
            // silently dropping them here.
            if let Ok(info) = svc.chapter_cbz_path(chapter_id).await
                && tokio::fs::try_exists(&info.path).await.unwrap_or(false)
            {
                svc.record_chapter_manifest(chapter_id, info.path).await;
            }

            ctx.progress
                .report(done as u64 + 1, total, "Hashing chapters")
                .await;
        }

        Ok(())
    }
}
