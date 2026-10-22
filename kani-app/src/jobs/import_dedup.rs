use crate::ids::MangaId;
use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ImportDedupJob {
    id: JobId,
    pub manga_ids: Vec<i64>,
}

impl ImportDedupJob {
    pub fn new(manga_ids: Vec<i64>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            manga_ids,
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for ImportDedupJob {
    const JOB_TYPE: &'static str = "import_dedup";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        format!(
            "Record duplicates for {} imported manga",
            self.manga_ids.len()
        )
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let total = self.manga_ids.len() as u64;
        for (idx, manga_id) in self.manga_ids.iter().enumerate() {
            if ctx.cancel.is_cancelled() {
                return Err(JobError::Cancelled);
            }
            if let Err(e) =
                crate::service::dedup::record_duplicates_for_manga(&ctx.pool, MangaId(*manga_id))
                    .await
            {
                tracing::warn!("Duplicate recording failed for manga {manga_id}: {e}");
            }
            ctx.progress
                .report(idx as u64 + 1, total, "Recording duplicates")
                .await;
        }
        Ok(())
    }
}
