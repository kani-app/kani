use super::{get_access_token, get_mapping};
use crate::error::{Result, ServiceError};
use crate::ids::{MangaId, UserId};
use crate::service::AppService;

impl AppService {
    /// Sync a single manga's tracking state with all linked external trackers.
    pub async fn sync_manga_trackers(&self, user_id: UserId, manga_id: MangaId) -> Result<()> {
        let local = self.get_manga_tracking(user_id, manga_id).await?;

        if !local.tracking_enabled {
            return Ok(());
        }

        // Collect tracker IDs and names to process; avoids holding the read lock across awaits
        // on DB calls that could be slow, and prevents any theoretical deadlock.
        let tracker_ids: Vec<i64> = {
            let registry = self.tracker_registry.read().await;
            registry.trackers.keys().copied().collect()
        };

        for tracker_id in tracker_ids {
            let Some(tracker_manga_id) =
                get_mapping(&self.db, user_id, tracker_id, manga_id).await?
            else {
                continue;
            };

            let (access_token, remote) = {
                let registry = self.tracker_registry.read().await;
                let Some(tracker) = registry.get(tracker_id) else {
                    continue;
                };

                let token = match get_access_token(
                    &self.db,
                    tracker_id,
                    user_id,
                    tracker,
                    self.encryption.as_deref(),
                )
                .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping sync for tracker {} ({}): {e}",
                            tracker_id,
                            tracker.name()
                        );
                        continue;
                    }
                };

                let remote = match tracker.get_status(&token, &tracker_manga_id).await {
                    Ok(s) => s,
                    Err(e @ ServiceError::RateLimited { .. }) => return Err(e),
                    // The token was revoked while still notionally valid, so the
                    // proactive `expires_at` refresh never fired. Flag the link
                    // rather than retrying a dead credential forever.
                    Err(e @ ServiceError::TrackerAuthExpired(_)) => {
                        tracing::warn!(
                            "{} rejected the stored credentials; marking link for reauth: {e}",
                            tracker.name()
                        );
                        super::mark_needs_reauth(&self.db, user_id, tracker_id).await;
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to get status from {} for manga {}: {e}",
                            tracker.name(),
                            tracker_manga_id
                        );
                        continue;
                    }
                };

                (token, remote)
            };

            // Conflict resolution: progress only moves forward.
            let chapters_read = local.chapters_read.max(remote.chapters_read);
            let status = local.status.or(remote.status);
            let score = local.score.or(remote.score);

            // Push merged state to the remote tracker (lock-free — we dropped the read lock).
            if let Some(s) = status {
                let registry = self.tracker_registry.read().await;
                if let Some(tracker) = registry.get(tracker_id) {
                    match tracker
                        .update_status(&access_token, &tracker_manga_id, s, score, chapters_read)
                        .await
                    {
                        Ok(()) => {}
                        Err(e @ ServiceError::RateLimited { .. }) => return Err(e),
                        Err(e @ ServiceError::TrackerAuthExpired(_)) => {
                            tracing::warn!(
                                "{} rejected the stored credentials on push; marking link for reauth: {e}",
                                tracker.name()
                            );
                            super::mark_needs_reauth(&self.db, user_id, tracker_id).await;
                        }
                        Err(e) => tracing::warn!("Failed to push to {}: {e}", tracker.name()),
                    }
                }
            }

            if remote.status.is_some()
                && local.status.is_none()
                && let Some(s) = remote.status
            {
                self.set_manga_status(user_id, manga_id, s).await?;
            }

            if remote.score.is_some()
                && local.score.is_none()
                && let Some(s) = remote.score
            {
                self.set_manga_score(user_id, manga_id, s).await?;
            }

            if remote.chapters_read > local.chapters_read {
                tracing::info!(
                    "Remote tracker {} has more progress ({} vs {})",
                    tracker_id,
                    remote.chapters_read,
                    local.chapters_read
                );
            }
        }

        Ok(())
    }

    /// Sync all mapped manga for a user across all linked trackers.
    pub async fn sync_all_trackers(&self, user_id: UserId) -> Result<()> {
        let manga_ids: Vec<i64> = sqlx::query_scalar!(
            "SELECT DISTINCT manga_id FROM tracker_manga_mappings WHERE user_id = ?",
            user_id,
        )
        .fetch_all(&self.db_read)
        .await?;

        for manga_id in manga_ids {
            if let Err(e) = self.sync_manga_trackers(user_id, MangaId(manga_id)).await {
                tracing::warn!("Sync failed for manga {manga_id}: {e}");
            }
        }

        Ok(())
    }

    /// Sync only stale tracker mappings (never-synced, or last synced longer than
    /// `interval_hours` ago), throttled per access token and honouring HTTP 429
    /// `Retry-After`. Respects `cancel` for graceful shutdown and caps work at `cap`.
    pub async fn sync_stale_trackers(
        &self,
        interval_hours: i64,
        cap: usize,
        min_spacing: std::time::Duration,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<TrackerSyncOutcome> {
        use std::time::Instant;

        let now = time::OffsetDateTime::now_utc();
        let cutoff = now - time::Duration::hours(interval_hours.max(0));
        let cap_i64 = cap as i64;

        let rows = sqlx::query!(
            "SELECT DISTINCT user_id, manga_id FROM tracker_manga_mappings \
             WHERE last_synced_at IS NULL OR last_synced_at < ? \
             ORDER BY manga_id LIMIT ?",
            cutoff,
            cap_i64,
        )
        .fetch_all(&self.db_read)
        .await?;

        let mut throttle = crate::jobs::tracker_sync::TokenThrottle::default();
        let mut outcome = TrackerSyncOutcome::default();

        for row in rows {
            if cancel.is_cancelled() {
                break;
            }

            let key = row.user_id;
            let wait = throttle.delay_before(key, Instant::now(), min_spacing);
            if !wait.is_zero() {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(wait) => {}
                }
            }
            throttle.record_call(key, Instant::now());

            match self
                .sync_manga_trackers(UserId(row.user_id), MangaId(row.manga_id))
                .await
            {
                Ok(()) => {
                    let synced_at = time::OffsetDateTime::now_utc();
                    sqlx::query!(
                        "UPDATE tracker_manga_mappings SET last_synced_at = ? \
                         WHERE user_id = ? AND manga_id = ?",
                        synced_at,
                        row.user_id,
                        row.manga_id,
                    )
                    .execute(&self.db)
                    .await?;
                    outcome.synced += 1;
                }
                Err(ServiceError::RateLimited { retry_after_secs }) => {
                    let backoff = std::time::Duration::from_secs(retry_after_secs.unwrap_or(60));
                    throttle.record_rate_limited(key, Instant::now(), backoff);
                    outcome.rate_limited += 1;
                }
                Err(e) => {
                    tracing::warn!("Tracker sync failed for manga {}: {e}", row.manga_id);
                    outcome.failed += 1;
                }
            }
        }

        self.reconcile_tracker_credential_degradation().await;
        Ok(outcome)
    }

    /// Reports links whose credentials the provider has rejected.
    ///
    /// Reconciled from the table rather than raised where the rejection happens:
    /// re-authentication clears `needs_reauth` through an OAuth path that has no
    /// registry, so an event-driven entry would never be cleared.
    async fn reconcile_tracker_credential_degradation(&self) {
        let Ok(stale) = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM user_tracker_credentials WHERE needs_reauth = TRUE"
        )
        .fetch_one(&self.db)
        .await
        else {
            return;
        };

        if stale > 0 {
            self.degradations.register(
                crate::service::degradations::ids::TRACKER_CREDENTIALS,
                crate::service::degradations::Severity::Warn,
                "Tracker credentials",
                format!(
                    "{stale} tracker link(s) were rejected by their provider, so progress is no \
                     longer syncing for them."
                ),
                "Re-link the affected tracker in Settings -> Trackers.",
            );
        } else {
            self.degradations
                .clear(crate::service::degradations::ids::TRACKER_CREDENTIALS);
        }
    }
}

/// Summary of a stale-tracker sync run.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct TrackerSyncOutcome {
    pub synced: usize,
    pub rate_limited: usize,
    pub failed: usize,
}
