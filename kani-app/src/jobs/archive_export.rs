use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId};
use crate::service::archive::ArchiveSpec;
use kani_core::archive::ArchiveReport;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ArchiveExportJob {
    id: JobId,
    spec: ArchiveSpec,
}

impl ArchiveExportJob {
    pub fn new(spec: ArchiveSpec) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            spec,
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for ArchiveExportJob {
    const JOB_TYPE: &'static str = "archive_export";
    type Output = ArchiveReport;

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        match &self.spec.manga_ids {
            Some(ids) => format!("Archive export ({} series)", ids.len()),
            None => "Archive export (whole library)".to_string(),
        }
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<ArchiveReport, JobError> {
        let svc = ctx.service();
        svc.export_archive(&self.spec, Some(ctx.progress.clone()))
            .await
            .map_err(|e| JobError::Internal(e.to_string()))
    }
}
