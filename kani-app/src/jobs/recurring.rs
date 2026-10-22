use crate::error::Result;
use crate::service::AppService;
use time::OffsetDateTime;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, sqlx::Type,
)]
#[sqlx(type_name = "TEXT")]
pub enum RecurringJobKind {
    DbMaintenance,
    DbVacuum,
    AutoScan,
    AuditPrune,
    TrashPurge,
    StorageMonitor,
    PendingDeleteRetry,
    ScheduledBackup,
    TrackerSync,
    V8ProcessReap,
    UpdateCheck,
    IntegrityScrubQuick,
    IntegrityScrubDeep,
    ImportRelinkSweep,
}

impl RecurringJobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DbMaintenance => "db_maintenance",
            Self::DbVacuum => "db_vacuum",
            Self::AutoScan => "auto_scan",
            Self::AuditPrune => "audit_prune",
            Self::TrashPurge => "trash_purge",
            Self::StorageMonitor => "storage_monitor",
            Self::PendingDeleteRetry => "pending_delete_retry",
            Self::ScheduledBackup => "scheduled_backup",
            Self::TrackerSync => "tracker_sync",
            Self::V8ProcessReap => "v8_process_reap",
            Self::UpdateCheck => "update_check",
            Self::IntegrityScrubQuick => "integrity_scrub_quick",
            Self::IntegrityScrubDeep => "integrity_scrub_deep",
            Self::ImportRelinkSweep => "import_relink_sweep",
        }
    }

    fn default_interval_secs(self) -> i64 {
        match self {
            Self::DbMaintenance => 24 * 60 * 60,
            Self::DbVacuum => 7 * 24 * 60 * 60,
            Self::AutoScan => 60 * 60,
            Self::AuditPrune => 7 * 24 * 60 * 60,
            Self::TrashPurge => 7 * 24 * 60 * 60,
            Self::StorageMonitor => 24 * 60 * 60,
            Self::PendingDeleteRetry => 60 * 60,
            Self::ScheduledBackup => 60 * 60,
            Self::TrackerSync => 60 * 60,
            Self::V8ProcessReap => 5 * 60,
            Self::UpdateCheck => 24 * 60 * 60,
            Self::IntegrityScrubQuick => 24 * 60 * 60,
            Self::IntegrityScrubDeep => 7 * 24 * 60 * 60,
            Self::ImportRelinkSweep => 60 * 60,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::DbMaintenance,
            Self::DbVacuum,
            Self::AutoScan,
            Self::AuditPrune,
            Self::TrashPurge,
            Self::StorageMonitor,
            Self::PendingDeleteRetry,
            Self::ScheduledBackup,
            Self::TrackerSync,
            Self::V8ProcessReap,
            Self::UpdateCheck,
            Self::IntegrityScrubQuick,
            Self::IntegrityScrubDeep,
            Self::ImportRelinkSweep,
        ]
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::all().iter().copied().find(|k| k.as_str() == s)
    }
}

pub async fn ensure_recurring_rows(pool: &sqlx::SqlitePool) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    for kind in RecurringJobKind::all() {
        let kind_str = kind.as_str();
        let interval = kind.default_interval_secs();
        let next_due = now + time::Duration::seconds(interval);
        sqlx::query!(
            "INSERT OR IGNORE INTO recurring_jobs (kind, last_run_at, next_due_at) \
             VALUES (?, NULL, ?)",
            kind_str,
            next_due
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub(crate) async fn record_run(
    pool: &sqlx::SqlitePool,
    kind: RecurringJobKind,
    interval_override: Option<i64>,
) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    let interval = interval_override.unwrap_or_else(|| kind.default_interval_secs());
    let next_due = now + time::Duration::seconds(interval);
    let kind_str = kind.as_str();
    sqlx::query!(
        "UPDATE recurring_jobs SET last_run_at = ?, next_due_at = ? WHERE kind = ?",
        now,
        next_due,
        kind_str
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn is_due(pool: &sqlx::SqlitePool, kind: RecurringJobKind) -> bool {
    let kind_str = kind.as_str();
    let now = OffsetDateTime::now_utc();
    sqlx::query_scalar!(
        "SELECT COUNT(*) FROM recurring_jobs WHERE kind = ? AND next_due_at <= ?",
        kind_str,
        now
    )
    .fetch_one(pool)
    .await
    .map(|c| c > 0)
    .unwrap_or(false)
}

async fn auto_scan_job_active(pool: &sqlx::SqlitePool) -> bool {
    sqlx::query_scalar!(
        "SELECT COUNT(*) FROM jobs WHERE job_type = 'auto_scan' AND status IN ('pending', 'running')"
    )
    .fetch_one(pool)
    .await
    .map(|c| c > 0)
    .unwrap_or(false)
}

async fn integrity_scrub_job_active(pool: &sqlx::SqlitePool) -> bool {
    sqlx::query_scalar!(
        "SELECT COUNT(*) FROM jobs WHERE job_type = 'integrity_scrub' AND status IN ('pending', 'running')"
    )
    .fetch_one(pool)
    .await
    .map(|c| c > 0)
    .unwrap_or(false)
}

async fn scheduled_backup_job_active(pool: &sqlx::SqlitePool) -> bool {
    sqlx::query_scalar!(
        "SELECT COUNT(*) FROM jobs WHERE job_type = 'scheduled_backup' AND status IN ('pending', 'running')"
    )
    .fetch_one(pool)
    .await
    .map(|c| c > 0)
    .unwrap_or(false)
}

async fn tracker_sync_job_active(pool: &sqlx::SqlitePool) -> bool {
    sqlx::query_scalar!(
        "SELECT COUNT(*) FROM jobs WHERE job_type = 'tracker_sync' AND status IN ('pending', 'running')"
    )
    .fetch_one(pool)
    .await
    .map(|c| c > 0)
    .unwrap_or(false)
}

async fn scheduled_backup_time_matches(svc: &AppService) -> bool {
    let Ok(config) = svc.get_backup_schedule().await else {
        return false;
    };
    if !config.enabled {
        return false;
    }
    let now = OffsetDateTime::now_utc();
    let current_hour = now.hour();
    let current_weekday = now.weekday().number_days_from_monday();
    match config.frequency {
        crate::service::backup_scheduler::BackupFrequency::Daily { hour } => current_hour == hour,
        crate::service::backup_scheduler::BackupFrequency::Weekly { weekday, hour } => {
            current_weekday == weekday && current_hour == hour
        }
    }
}

async fn run_kind(svc: &AppService, kind: RecurringJobKind) {
    if kind == RecurringJobKind::AutoScan && auto_scan_job_active(&svc.db).await {
        return;
    }

    // Quick and deep share one job type, so this also stops the two scrub kinds
    // from overlapping each other.
    if matches!(
        kind,
        RecurringJobKind::IntegrityScrubQuick | RecurringJobKind::IntegrityScrubDeep
    ) && integrity_scrub_job_active(&svc.db).await
    {
        return;
    }

    // Nothing is waiting on an id, so the sweep would submit an empty pass.
    if kind == RecurringJobKind::ImportRelinkSweep && svc.pending_import_link_count().await == 0 {
        if let Err(e) = record_run(&svc.db, kind, None).await {
            tracing::warn!(
                "Failed to record recurring job reschedule for {:?}: {e}",
                kind
            );
        }
        return;
    }

    if kind == RecurringJobKind::ScheduledBackup {
        if !scheduled_backup_time_matches(svc).await {
            if let Err(e) = record_run(&svc.db, kind, None).await {
                tracing::warn!(
                    "Failed to record recurring job reschedule for {:?}: {e}",
                    kind
                );
            }
            return;
        }
        if scheduled_backup_job_active(&svc.db).await {
            return;
        }
    }

    if kind == RecurringJobKind::TrackerSync {
        let (enabled, interval_hours) = {
            let s = svc.settings.read().await;
            (s.tracker_auto_sync_enabled, s.tracker_sync_interval_hours)
        };
        if !enabled {
            if let Err(e) = record_run(&svc.db, kind, Some(interval_hours * 60 * 60)).await {
                tracing::warn!(
                    "Failed to record recurring job reschedule for {:?}: {e}",
                    kind
                );
            }
            return;
        }
        if tracker_sync_job_active(&svc.db).await {
            return;
        }
    }

    let result = submit_kind_job(svc, kind).await.map(|_| ());

    match result {
        Ok(()) => {
            let interval_override = {
                let s = svc.settings.read().await;
                match kind {
                    RecurringJobKind::AutoScan => Some(s.scan_interval_minutes * 60),
                    RecurringJobKind::TrackerSync => Some(s.tracker_sync_interval_hours * 60 * 60),
                    RecurringJobKind::DbMaintenance => {
                        Some(s.db_maintenance_interval_hours * 60 * 60)
                    }
                    RecurringJobKind::DbVacuum => Some(s.db_vacuum_interval_hours * 60 * 60),
                    RecurringJobKind::AuditPrune => Some(s.audit_prune_interval_hours * 60 * 60),
                    RecurringJobKind::TrashPurge => Some(s.trash_purge_interval_hours * 60 * 60),
                    RecurringJobKind::V8ProcessReap => Some(s.v8_idle_timeout_s.max(60)),
                    RecurringJobKind::IntegrityScrubQuick => {
                        Some(s.integrity_quick_scrub_interval_hours * 60 * 60)
                    }
                    RecurringJobKind::IntegrityScrubDeep => {
                        Some(s.integrity_deep_scrub_interval_hours * 60 * 60)
                    }
                    _ => None,
                }
            };
            if let Err(e) = record_run(&svc.db, kind, interval_override).await {
                tracing::warn!("Failed to record recurring job run for {:?}: {e}", kind);
            }
        }
        Err(e) => tracing::warn!("Recurring job {:?} failed: {e}", kind),
    }
}

/// Submits the background job that backs a recurring kind, returning its id.
/// Shared by the scheduler and the manual trigger path.
async fn submit_kind_job(svc: &AppService, kind: RecurringJobKind) -> Result<crate::jobs::JobId> {
    match kind {
        RecurringJobKind::IntegrityScrubQuick => {
            svc.job_manager
                .submit(crate::jobs::scrub::ScrubJob::new(
                    crate::service::integrity::ScrubDepth::Quick,
                    false,
                ))
                .await
        }
        RecurringJobKind::IntegrityScrubDeep => {
            svc.job_manager
                .submit(crate::jobs::scrub::ScrubJob::new(
                    crate::service::integrity::ScrubDepth::Deep,
                    false,
                ))
                .await
        }
        RecurringJobKind::DbMaintenance => {
            svc.job_manager
                .submit(crate::jobs::maintenance::AnalyzeJob::new())
                .await
        }
        RecurringJobKind::DbVacuum => {
            svc.job_manager
                .submit(crate::jobs::maintenance::VacuumJob::new())
                .await
        }
        RecurringJobKind::AutoScan => {
            svc.job_manager
                .submit(crate::jobs::scan::AutoScanJob::new())
                .await
        }
        RecurringJobKind::AuditPrune => {
            svc.job_manager
                .submit(crate::jobs::audit_prune::AuditPruneJob::new())
                .await
        }
        RecurringJobKind::TrashPurge => {
            let days = svc.settings.read().await.trash_retention_days.max(0) as u32;
            svc.job_manager
                .submit(crate::jobs::trash_purge::TrashPurgeJob::new(days))
                .await
        }
        RecurringJobKind::StorageMonitor => {
            svc.job_manager
                .submit(crate::jobs::storage::StorageMonitorJob::new())
                .await
        }
        RecurringJobKind::PendingDeleteRetry => {
            svc.job_manager
                .submit(crate::jobs::pending_delete_retry::PendingDeleteRetryJob::new())
                .await
        }
        RecurringJobKind::ScheduledBackup => {
            svc.job_manager
                .submit(crate::jobs::backup::ScheduledBackupJob::new())
                .await
        }
        RecurringJobKind::TrackerSync => {
            svc.job_manager
                .submit(crate::jobs::tracker_sync::TrackerSyncJob::new())
                .await
        }
        RecurringJobKind::V8ProcessReap => {
            svc.job_manager
                .submit(crate::jobs::v8_reap::V8ReapJob::new())
                .await
        }
        RecurringJobKind::UpdateCheck => {
            svc.job_manager
                .submit(crate::jobs::update_check::UpdateCheckJob::new())
                .await
        }
        RecurringJobKind::ImportRelinkSweep => svc.queue_import_resolve(None).await,
    }
}

/// Manually triggers a recurring kind's job immediately, bypassing the schedule
/// (does not touch `next_due_at`). Skips with `Ok(None)` if an instance of a
/// singleton kind is already pending/running. Used by the admin trigger endpoint.
pub async fn trigger_now(
    svc: &AppService,
    kind: RecurringJobKind,
) -> Result<Option<crate::jobs::JobId>> {
    let already_active = match kind {
        RecurringJobKind::AutoScan => auto_scan_job_active(&svc.db).await,
        RecurringJobKind::ScheduledBackup => scheduled_backup_job_active(&svc.db).await,
        RecurringJobKind::TrackerSync => tracker_sync_job_active(&svc.db).await,
        _ => false,
    };
    if already_active {
        return Ok(None);
    }
    let id = submit_kind_job(svc, kind).await?;
    Ok(Some(id))
}

pub fn spawn_recurring_scheduler(svc: &AppService) {
    let svc = svc.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            tokio::select! {
                _ = svc.shutdown_token.cancelled() => break,
                _ = interval.tick() => {}
            }
            for &kind in RecurringJobKind::all() {
                if is_due(&svc.db, kind).await {
                    run_kind(&svc, kind).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn v8_process_reap_is_registered() {
        assert!(RecurringJobKind::all().contains(&RecurringJobKind::V8ProcessReap));
        assert_eq!(
            RecurringJobKind::parse("v8_process_reap"),
            Some(RecurringJobKind::V8ProcessReap)
        );
        assert_eq!(RecurringJobKind::V8ProcessReap.as_str(), "v8_process_reap");
    }

    #[test]
    fn update_check_is_registered_and_runs_daily() {
        assert!(RecurringJobKind::all().contains(&RecurringJobKind::UpdateCheck));
        assert_eq!(
            RecurringJobKind::parse("update_check"),
            Some(RecurringJobKind::UpdateCheck)
        );
        assert_eq!(
            RecurringJobKind::UpdateCheck.default_interval_secs(),
            24 * 60 * 60
        );
    }
}
