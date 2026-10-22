use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct V8ReapJob {
    id: JobId,
}

impl V8ReapJob {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
        }
    }
}

impl Default for V8ReapJob {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BackgroundJob for V8ReapJob {
    const JOB_TYPE: &'static str = "v8_process_reap";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        "Reap idle V8 worker processes".to_string()
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        let idle_secs = svc.settings.read().await.v8_idle_timeout_s.max(0) as u64;
        let idle_for = std::time::Duration::from_secs(idle_secs);
        for id in svc.sources.active_ids() {
            if let Some(backend) = svc.sources.get_backend(id) {
                backend.reap_idle_v8(idle_for).await;
            }
        }
        Ok(())
    }
}
