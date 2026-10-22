use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PendingDeleteRetryJob {
    id: JobId,
}

impl PendingDeleteRetryJob {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
        }
    }
}

impl Default for PendingDeleteRetryJob {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BackgroundJob for PendingDeleteRetryJob {
    const JOB_TYPE: &'static str = "pending_delete_retry";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        "Retry pending chapter deletions".to_string()
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        svc.retry_pending_deletes()
            .await
            .map_err(|e| JobError::Internal(e.to_string()))
    }
}
