use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct StorageMonitorJob {
    id: JobId,
}

impl StorageMonitorJob {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
        }
    }
}

impl Default for StorageMonitorJob {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BackgroundJob for StorageMonitorJob {
    const JOB_TYPE: &'static str = "storage_monitor";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        "Storage usage snapshot".to_string()
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        svc.capture_storage_snapshot()
            .await
            .map_err(|e| JobError::Internal(e.to_string()))
    }
}
