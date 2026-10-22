use crate::ids::MangaId;
use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct ThumbnailGenerationJob {
    id: JobId,
    pub manga_id: i64,
}

impl ThumbnailGenerationJob {
    pub fn new(manga_id: i64) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            manga_id,
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for ThumbnailGenerationJob {
    const JOB_TYPE: &'static str = "thumbnail_generation";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        format!("Generate cover thumbnails for manga {}", self.manga_id)
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        svc.generate_and_store_thumbnails(MangaId(self.manga_id))
            .await
            .map_err(|e| JobError::Internal(e.to_string()))
    }
}
