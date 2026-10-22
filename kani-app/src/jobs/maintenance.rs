use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AnalyzeJob {
    id: JobId,
}

impl AnalyzeJob {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
        }
    }
}

impl Default for AnalyzeJob {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BackgroundJob for AnalyzeJob {
    const JOB_TYPE: &'static str = "db_analyze";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        "Database analyze: WAL checkpoint and ANALYZE".to_string()
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&ctx.pool)
            .await
            .map_err(|e| JobError::Internal(e.to_string()))?;
        sqlx::query("ANALYZE")
            .execute(&ctx.pool)
            .await
            .map_err(|e| JobError::Internal(e.to_string()))?;
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct VacuumJob {
    id: JobId,
}

impl VacuumJob {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
        }
    }
}

impl Default for VacuumJob {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BackgroundJob for VacuumJob {
    const JOB_TYPE: &'static str = "db_vacuum";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        "Database vacuum: full file compaction".to_string()
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        sqlx::query("VACUUM")
            .execute(&ctx.pool)
            .await
            .map_err(|e| JobError::Internal(e.to_string()))?;
        Ok(())
    }
}
