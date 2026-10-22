use crate::jobs::error::{DownloadErrorKind, JobError};
use crate::jobs::framework::{BackgroundJob, JobContext, JobId, JobPriority};

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct WebhookDeliveryJob {
    id: JobId,
    pub webhook_id: i64,
    pub event_type: String,
    pub body: String,
    pub attempt: u32,
}

impl WebhookDeliveryJob {
    pub fn new(webhook_id: i64, event_type: String, body: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            webhook_id,
            event_type,
            body,
            attempt: 0,
        }
    }
}

#[async_trait::async_trait]
impl BackgroundJob for WebhookDeliveryJob {
    const JOB_TYPE: &'static str = "webhook_delivery";
    type Output = ();

    fn id(&self) -> JobId {
        self.id
    }

    fn description(&self) -> String {
        format!("Deliver {} webhook", self.event_type)
    }

    fn priority(&self) -> JobPriority {
        JobPriority::Low
    }

    fn attempt_count(&self) -> u32 {
        self.attempt
    }

    fn retry_params(&self) -> Option<String> {
        let next = Self {
            id: uuid::Uuid::new_v4(),
            attempt: self.attempt + 1,
            ..self.clone()
        };
        serde_json::to_string(&next).ok()
    }

    async fn run(self: Box<Self>, ctx: JobContext) -> Result<(), JobError> {
        let svc = ctx.service();

        let wh = match svc.webhook_service.get_by_id(self.webhook_id).await {
            Ok(w) => w,
            Err(_) => return Ok(()),
        };
        if !wh.enabled {
            return Ok(());
        }

        let (http_status, error) = svc
            .webhook_service
            .send_signed(&wh.url, wh.secret.as_deref(), &self.body)
            .await;

        svc.webhook_service
            .record_delivery(
                self.webhook_id,
                &self.event_type,
                &self.body,
                http_status,
                error.clone(),
            )
            .await;

        if error.is_some() {
            return Err(JobError::Download(DownloadErrorKind::Network {
                retryable: true,
            }));
        }
        Ok(())
    }
}
