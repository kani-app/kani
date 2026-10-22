use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AuditPruneJob {
    id: JobId,
}

impl AuditPruneJob {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
        }
    }
}

impl Default for AuditPruneJob {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BackgroundJob for AuditPruneJob {
    const JOB_TYPE: &'static str = "audit_prune";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        "Prune old audit-log entries".to_string()
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        svc.prune_audit_log()
            .await
            .map(|_| ())
            .map_err(|e| JobError::Internal(e.to_string()))
    }
}
