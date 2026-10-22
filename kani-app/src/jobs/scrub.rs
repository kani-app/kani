use crate::jobs::error::JobError;
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};
use crate::service::integrity::ScrubDepth;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ScrubJob {
    id: JobId,
    depth: ScrubDepth,
    fix: bool,
    /// Ignore the revalidation window and check every chapter.
    #[serde(default)]
    full: bool,
}

impl ScrubJob {
    /// A scheduled scrub: skips chapters verified inside the revalidation
    /// window.
    pub fn new(depth: ScrubDepth, fix: bool) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            depth,
            fix,
            full: false,
        }
    }

    /// A scrub the user asked for: checks everything.
    pub fn full(depth: ScrubDepth, fix: bool) -> Self {
        Self {
            full: true,
            ..Self::new(depth, fix)
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for ScrubJob {
    const JOB_TYPE: &'static str = "integrity_scrub";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        let depth = match self.depth {
            ScrubDepth::Quick => "Quick",
            ScrubDepth::Deep => "Deep",
        };
        let scope = if self.full { " (full)" } else { "" };
        if self.fix {
            format!("{depth} integrity scrub{scope} (repairing)")
        } else {
            format!("{depth} integrity scrub{scope}")
        }
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();
        svc.scrub_library_inner(self.depth, self.fix, self.full, Some(ctx.progress.clone()))
            .await
            .map(|_| ())
            .map_err(|e| JobError::Internal(e.to_string()))
    }
}
