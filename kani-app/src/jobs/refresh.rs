use crate::ids::MangaId;
use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId};
use crate::models::RefreshOptions;

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct RefreshMangaJob {
    id: JobId,
    pub manga_id: i64,
    pub manga_title: String,
    pub opts: RefreshOptions,
}

impl RefreshMangaJob {
    pub fn new(manga_id: i64, manga_title: String, opts: RefreshOptions) -> Self {
        Self {
            id: JobId::new_v4(),
            manga_id,
            manga_title,
            opts,
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for RefreshMangaJob {
    const JOB_TYPE: &'static str = "refresh_manga";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        format!("Refresh metadata for {}", self.manga_title)
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        svc.refresh_manga_with_options(MangaId::from(self.manga_id), self.opts)
            .await
            .map_err(|e| JobError::Internal(e.to_string()))
    }
}
