use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct TrashPurgeJob {
    id: JobId,
    pub days: u32,
}

impl TrashPurgeJob {
    pub fn new(days: u32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            days,
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for TrashPurgeJob {
    const JOB_TYPE: &'static str = "trash_purge";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        format!("Purge trashed manga older than {} days", self.days)
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        svc.purge_expired_trash(self.days)
            .await
            .map_err(|e| JobError::Internal(e.to_string()))?;
        // Files displaced by an upgrade share the trash's retention window;
        // until they are swept, applying an upgrade stays reversible.
        if let Err(e) = svc.purge_replaced(self.days as i64).await {
            tracing::warn!("Failed to sweep .replaced: {e}");
        }
        Ok(())
    }
}
