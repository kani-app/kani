use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::SqlitePool;
use time::format_description::well_known::Rfc3339;

use crate::error::{Result, ServiceError};
use crate::ids::{ChapterId, MangaId};

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

#[derive(Clone)]
pub struct WebhookService {
    pub db: SqlitePool,
    pub db_read: SqlitePool,
    pub http: rquest::Client,
    /// Test-only escape hatch: when set, the SSRF egress guard permits
    /// private/loopback hosts so a test can deliver to a local mock server.
    /// Always `false` in production (there is no way to set it there).
    allow_private_egress: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl WebhookService {
    pub fn new(db: SqlitePool, db_read: SqlitePool) -> Self {
        let http = kani_core::network::build_validating_client().unwrap_or_else(|e| {
            tracing::warn!(
                "Webhook: failed to build SSRF-validating client ({e}); \
                 falling back to a plain client — literal-IP delivery is still \
                 blocked, but DNS-rebinding protection is unavailable"
            );
            rquest::Client::new()
        });
        Self {
            db,
            db_read,
            http,
            allow_private_egress: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Permit delivery to private/loopback hosts. Test-only — lets a test drive
    /// the delivery pipeline against a loopback mock server while production
    /// keeps blocking SSRF targets.
    #[cfg(any(test, feature = "test-util"))]
    pub fn allow_private_egress_for_test(&self) {
        self.allow_private_egress
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookPayload {
    ChapterNew {
        manga_id: MangaId,
        manga_name: String,
        chapter_count: usize,
        chapter_ids: Vec<i64>,
        chapter_names: Vec<String>,
    },
    MangaAdded {
        manga_id: MangaId,
        manga_name: String,
        source_id: i64,
    },
    MangaDeleted {
        manga_id: MangaId,
        manga_name: String,
    },
    ChapterDownloaded {
        chapter_id: ChapterId,
        manga_id: MangaId,
        manga_name: String,
        chapter_name: String,
    },
    ScanCompleted {
        total_scanned: usize,
        failed_count: usize,
    },
}

impl WebhookPayload {
    fn event_type(&self) -> &'static str {
        match self {
            Self::ChapterNew { .. } => "chapter.new",
            Self::MangaAdded { .. } => "manga.added",
            Self::MangaDeleted { .. } => "manga.deleted",
            Self::ChapterDownloaded { .. } => "chapter.downloaded",
            Self::ScanCompleted { .. } => "scan.completed",
        }
    }

    fn manga_id(&self) -> Option<i64> {
        match self {
            Self::ChapterNew { manga_id, .. }
            | Self::MangaAdded { manga_id, .. }
            | Self::MangaDeleted { manga_id, .. }
            | Self::ChapterDownloaded { manga_id, .. } => Some(manga_id.0),
            Self::ScanCompleted { .. } => None,
        }
    }
}

#[derive(Serialize)]
struct Envelope<'a> {
    event: &'static str,
    timestamp: String,
    data: &'a WebhookPayload,
}

#[derive(Debug, Serialize)]
pub struct WebhookRow {
    pub id: i64,
    pub url: String,
    #[serde(skip)]
    pub secret: Option<String>,
    pub events: String,
    pub enabled: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
}

#[derive(Serialize)]
pub struct DeliveryRow {
    pub id: i64,
    pub webhook_id: i64,
    pub event_type: String,
    pub payload: String,
    pub http_status: Option<i64>,
    pub error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub delivered_at: time::OffsetDateTime,
}

#[derive(Deserialize)]
pub struct CreateWebhookBody {
    pub url: String,
    pub secret: Option<String>,
    /// JSON array of event type strings, or `["*"]` for all. Defaults to `["*"]`.
    pub events: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateWebhookBody {
    pub url: Option<String>,
    pub secret: Option<String>,
    pub events: Option<String>,
    pub enabled: Option<bool>,
}

impl WebhookService {
    /// Build the delivery envelope and resolve the webhooks that should receive it.
    /// Returns `(event_type, serialized_body, applicable_webhooks)`.
    pub(crate) async fn applicable_deliveries(
        &self,
        payload: &WebhookPayload,
    ) -> Result<(String, String, Vec<WebhookRow>)> {
        let event_type = payload.event_type();
        let manga_id = payload.manga_id();

        let envelope = Envelope {
            event: event_type,
            timestamp: now_rfc3339(),
            data: payload,
        };
        let body =
            serde_json::to_string(&envelope).map_err(|e| ServiceError::Internal(e.to_string()))?;

        let webhooks = self.list_applicable(event_type, manga_id).await?;
        Ok((event_type.to_owned(), body, webhooks))
    }

    /// Sign and POST a webhook body. Returns `(http_status, error)`; never fails the caller.
    pub async fn send_signed(
        &self,
        url: &str,
        secret: Option<&str>,
        body: &str,
    ) -> (Option<i64>, Option<String>) {
        // Last line of defence at the egress point: a row may predate URL
        // validation, or an admin may have edited the DB directly. A literal
        // forbidden IP never reaches the resolver, so reject it here too.
        if !self
            .allow_private_egress
            .load(std::sync::atomic::Ordering::Relaxed)
            && kani_core::network::is_forbidden_url_host(url)
        {
            tracing::warn!("Webhook delivery to {url} refused: forbidden host");
            return (
                None,
                Some("Refused: webhook host is not permitted".to_owned()),
            );
        }

        let sig = secret.map(|s| {
            let mut mac =
                Hmac::<Sha256>::new_from_slice(s.as_bytes()).expect("HMAC accepts any key size");
            mac.update(body.as_bytes());
            format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
        });

        let mut builder = self
            .http
            .post(url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "Kani-Webhook/1.0")
            .body(body.to_owned());
        if let Some(ref sig) = sig {
            builder = builder.header("X-Kani-Signature", sig.as_str());
        }

        match builder.send().await {
            Ok(r) => (Some(r.status().as_u16() as i64), None),
            Err(e) => {
                tracing::warn!("Webhook delivery to {url} failed: {e}");
                (None, Some(e.to_string()))
            }
        }
    }

    /// Record a delivery attempt in `webhook_deliveries`.
    pub async fn record_delivery(
        &self,
        webhook_id: i64,
        event_type: &str,
        body: &str,
        http_status: Option<i64>,
        error: Option<String>,
    ) {
        let _ = sqlx::query!(
            "INSERT INTO webhook_deliveries \
             (webhook_id, event_type, payload, http_status, error) \
             VALUES (?, ?, ?, ?, ?)",
            webhook_id,
            event_type,
            body,
            http_status,
            error,
        )
        .execute(&self.db)
        .await;
    }

    /// Send a synchronous test event to a single webhook and return success/error.
    pub async fn send_test(&self, id: i64) -> Result<()> {
        let wh = self.get_by_id(id).await?;

        let body = serde_json::to_string(&serde_json::json!({
            "event": "test",
            "timestamp": now_rfc3339(),
            "data": {},
        }))
        .map_err(|e| ServiceError::Internal(e.to_string()))?;

        let (http_status, error) = self.send_signed(&wh.url, wh.secret.as_deref(), &body).await;

        if let Some(e) = error {
            return Err(ServiceError::Internal(format!(
                "Webhook delivery failed: {e}"
            )));
        }
        match http_status {
            Some(s) if (200..300).contains(&s) => Ok(()),
            Some(s) => Err(ServiceError::Internal(format!("Webhook returned HTTP {s}"))),
            None => Err(ServiceError::Internal("Webhook delivery failed".into())),
        }
    }

    async fn list_applicable(
        &self,
        event_type: &str,
        manga_id: Option<i64>,
    ) -> Result<Vec<WebhookRow>> {
        let webhooks = sqlx::query_as!(
            WebhookRow,
            r#"SELECT id, url, secret, events, enabled AS "enabled: bool", created_at
               FROM webhooks
               WHERE enabled = TRUE
                 AND (
                     events = '["*"]'
                     OR EXISTS (SELECT 1 FROM json_each(events) WHERE value = ?)
                 )"#,
            event_type,
        )
        .fetch_all(&self.db_read)
        .await?;

        let Some(mid) = manga_id else {
            return Ok(webhooks);
        };

        // Fetch opt-out rows for this manga (webhook_id = 0 = global opt-out).
        let opted_out: Vec<i64> = sqlx::query_scalar!(
            "SELECT webhook_id FROM webhook_manga_overrides \
             WHERE manga_id = ? AND enabled = FALSE",
            mid,
        )
        .fetch_all(&self.db_read)
        .await?;

        let opted_out_set: std::collections::HashSet<i64> = opted_out.into_iter().collect();
        if opted_out_set.contains(&0) {
            // Global opt-out — suppress all webhooks for this manga.
            return Ok(vec![]);
        }

        Ok(webhooks
            .into_iter()
            .filter(|w| !opted_out_set.contains(&w.id))
            .collect())
    }

    pub async fn list_webhooks(&self) -> Result<Vec<WebhookRow>> {
        let rows = sqlx::query_as!(
            WebhookRow,
            r#"SELECT id, url, secret, events, enabled AS "enabled: bool", created_at
               FROM webhooks ORDER BY id ASC"#,
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn get_by_id(&self, id: i64) -> Result<WebhookRow> {
        sqlx::query_as!(
            WebhookRow,
            r#"SELECT id, url, secret, events, enabled AS "enabled: bool", created_at
               FROM webhooks WHERE id = ?"#,
            id,
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Webhook {id} not found")))
    }

    pub async fn create_webhook(&self, body: CreateWebhookBody) -> Result<WebhookRow> {
        validate_url(&body.url)?;
        let events = body.events.unwrap_or_else(|| r#"["*"]"#.to_owned());
        validate_events_json(&events)?;

        let id = sqlx::query_scalar!(
            "INSERT INTO webhooks (url, secret, events) VALUES (?, ?, ?) RETURNING id",
            body.url,
            body.secret,
            events,
        )
        .fetch_one(&self.db)
        .await?;

        self.get_by_id(id).await
    }

    pub async fn update_webhook(&self, id: i64, body: UpdateWebhookBody) -> Result<WebhookRow> {
        self.get_by_id(id).await?;

        if let Some(ref url) = body.url {
            validate_url(url)?;
        }
        if let Some(ref events) = body.events {
            validate_events_json(events)?;
        }

        if let Some(url) = &body.url {
            sqlx::query!("UPDATE webhooks SET url = ? WHERE id = ?", url, id)
                .execute(&self.db)
                .await?;
        }
        if body.secret.is_some() {
            sqlx::query!(
                "UPDATE webhooks SET secret = ? WHERE id = ?",
                body.secret,
                id
            )
            .execute(&self.db)
            .await?;
        }
        if let Some(events) = &body.events {
            sqlx::query!("UPDATE webhooks SET events = ? WHERE id = ?", events, id)
                .execute(&self.db)
                .await?;
        }
        if let Some(enabled) = body.enabled {
            sqlx::query!("UPDATE webhooks SET enabled = ? WHERE id = ?", enabled, id)
                .execute(&self.db)
                .await?;
        }

        self.get_by_id(id).await
    }

    pub async fn delete_webhook(&self, id: i64) -> Result<()> {
        let affected = sqlx::query!("DELETE FROM webhooks WHERE id = ?", id)
            .execute(&self.db)
            .await?
            .rows_affected();
        if affected == 0 {
            return Err(ServiceError::NotFound(format!("Webhook {id} not found")));
        }
        Ok(())
    }

    pub async fn list_deliveries(&self, webhook_id: i64) -> Result<Vec<DeliveryRow>> {
        let rows = sqlx::query_as!(
            DeliveryRow,
            "SELECT id, webhook_id, event_type, payload, http_status, error, delivered_at \
             FROM webhook_deliveries \
             WHERE webhook_id = ? \
             ORDER BY delivered_at DESC \
             LIMIT 50",
            webhook_id,
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows)
    }

    /// Absence of a global opt-out means notifications are enabled.
    pub async fn get_manga_notify(&self, manga_id: MangaId) -> Result<bool> {
        let opted_out: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM webhook_manga_overrides \
             WHERE webhook_id = 0 AND manga_id = ? AND enabled = FALSE",
            manga_id,
        )
        .fetch_one(&self.db_read)
        .await?;
        Ok(opted_out == 0)
    }

    pub async fn set_manga_notify(&self, manga_id: MangaId, enabled: bool) -> Result<()> {
        if enabled {
            // Remove the opt-out row so the default (enabled) applies.
            sqlx::query!(
                "DELETE FROM webhook_manga_overrides WHERE webhook_id = 0 AND manga_id = ?",
                manga_id,
            )
            .execute(&self.db)
            .await?;
        } else {
            sqlx::query!(
                "INSERT INTO webhook_manga_overrides (webhook_id, manga_id, enabled) \
                 VALUES (0, ?, FALSE) \
                 ON CONFLICT (webhook_id, manga_id) DO UPDATE SET enabled = FALSE",
                manga_id,
            )
            .execute(&self.db)
            .await?;
        }
        Ok(())
    }
}

impl crate::service::AppService {
    /// Resolve applicable webhooks for an event and submit one delivery job per webhook.
    /// Never blocks or fails the caller — errors are logged.
    pub async fn fire_webhooks(&self, payload: WebhookPayload) {
        let (event_type, body, webhooks) =
            match self.webhook_service.applicable_deliveries(&payload).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!("Webhook: failed to resolve deliveries: {e}");
                    return;
                }
            };

        for wh in webhooks {
            let job = crate::jobs::webhook_delivery::WebhookDeliveryJob::new(
                wh.id,
                event_type.clone(),
                body.clone(),
            );
            if let Err(e) = self.job_manager.submit(job).await {
                tracing::warn!("Webhook: failed to submit delivery job for {}: {e}", wh.id);
            }
        }
    }
}

fn validate_url(url: &str) -> Result<()> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ServiceError::Validation(
            "Webhook URL must start with http:// or https://".to_owned(),
        ));
    }
    if kani_core::network::is_forbidden_url_host(url) {
        return Err(ServiceError::Validation(
            "Webhook URL must not point at a private, loopback, or reserved address".to_owned(),
        ));
    }
    Ok(())
}

fn validate_events_json(events: &str) -> Result<()> {
    let arr: serde_json::Value = serde_json::from_str(events)
        .map_err(|_| ServiceError::Validation("events must be a JSON array".to_owned()))?;
    if !arr.is_array() {
        return Err(ServiceError::Validation(
            "events must be a JSON array".to_owned(),
        ));
    }
    Ok(())
}
