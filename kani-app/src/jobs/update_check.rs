use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct UpdateCheckJob {
    id: JobId,
}

impl UpdateCheckJob {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
        }
    }
}

impl Default for UpdateCheckJob {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BackgroundJob for UpdateCheckJob {
    const JOB_TYPE: &'static str = "update_check";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        "Check for a newer Kani release".to_string()
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        ctx.service()
            .run_update_check()
            .await
            .map_err(|e| JobError::Internal(e.to_string()))
    }
}
