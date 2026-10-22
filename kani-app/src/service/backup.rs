use std::io::{Cursor, Read as _, Write as _};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, aead::Aead};
use serde::{Deserialize, Serialize};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use super::AppService;
use crate::error::{Result, ServiceError};
use crate::events::AppEvent;
use crate::ids::{MangaId, SourceId, UserId};
use kani_shared::types::{
    AdvancedSettings, DownloadSettings, EmailSettings, MaintenanceSettings, PerformanceSettings,
    ScanSettings, SecuritySettings, SettingsUpdate, TrackingSettings,
};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackupData {
    pub version: u32,
    pub exported_at: String,
    pub manga: Vec<BackupManga>,
    pub categories: Vec<BackupCategory>,
    pub settings: Option<BackupSettings>,
    #[serde(default)]
    pub repos: Vec<BackupRepo>,
    #[serde(default)]
    pub blocked_repos: Vec<BackupBlockedRepo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackupRepo {
    pub url: String,
    pub name: String,
    pub maintainer_key: String,
    pub trusted_level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_cache: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackupBlockedRepo {
    pub url: String,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackupManga {
    pub source_name: SourceId,
    pub source_manga_id: String,
    pub name: String,
    pub status: Option<i64>,
    pub auto_download: bool,
    pub auto_scan: bool,
    pub scanlator_mode: String,
    pub categories: Vec<String>,
    pub tracking: Option<BackupMangaTracking>,
    pub download_rules: Vec<BackupDownloadRule>,
    pub chapter_progress: Vec<BackupChapterProgress>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackupMangaTracking {
    pub status: i64,
    pub score: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackupChapterProgress {
    pub source_chapter_id: String,
    pub is_read: bool,
    pub last_page_read: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackupDownloadRule {
    pub rule_type: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BackupCategory {
    pub name: String,
    pub sort_order: i64,
}

/// The settings carried in a backup.
///
/// Instead it carries the eight `SettingsUpdate` group structs, which between
/// them cover sixty-one of the sixty-two fields — everything a user can edit.
/// The sixty-second is `first_run_complete`, which must never be restored: it
/// would re-arm or bypass the setup wizard on the target install. A setting
/// added to any group from now on is backed up without touching this file.
///
/// Every group is optional, and absent means "leave alone" rather than "reset
/// to default". That is what lets a backup written by an older version — which
/// carried only the three flat fields — restore exactly as it always did
/// instead of resetting the other fifty-eight to defaults.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct BackupSettings {
    /// Written by versions that predate the group payloads, and still written
    /// today so a backup taken here can be restored by one of those versions
    /// rather than failing to deserialise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_interval_minutes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_scan: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrent_page_downloads: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<DownloadSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan: Option<ScanSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advanced: Option<AdvancedSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking: Option<TrackingSettings>,
    /// Carried with credential values masked, exactly as the settings API
    /// masks them. A backup file is not a place for the SMTP password, and
    /// restoring through `update_settings` substitutes the stored value back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<EmailSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance: Option<MaintenanceSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<SecuritySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance: Option<PerformanceSettings>,
}

#[derive(Debug, Serialize)]
pub struct BackupPreview {
    pub version: u32,
    pub exported_at: String,
    pub manga_count: u32,
    pub category_count: u32,
    pub download_rule_count: u32,
    pub has_tracking: bool,
    pub has_chapter_progress: bool,
    pub has_settings: bool,
    pub repo_count: u32,
    pub blocked_repo_count: u32,
    pub sources: Vec<BackupSourceSummary>,
}

#[derive(Debug, Serialize)]
pub struct BackupSourceSummary {
    pub source_name: SourceId,
    pub manga_count: u32,
    pub found: bool,
}

#[derive(Debug, Deserialize)]
pub struct RestoreOptions {
    pub merge: bool,
    pub import_manga: bool,
    pub import_categories: bool,
    pub import_download_rules: bool,
    pub import_tracking: bool,
    pub import_chapter_progress: bool,
    pub import_settings: bool,
    #[serde(default = "default_true")]
    pub import_repos: bool,
}

fn default_true() -> bool {
    true
}

impl Default for RestoreOptions {
    fn default() -> Self {
        Self {
            merge: false,
            import_manga: true,
            import_categories: true,
            import_download_rules: true,
            import_tracking: true,
            import_chapter_progress: false,
            import_settings: false,
            import_repos: true,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RestoreResult {
    pub imported_manga: u32,
    pub skipped_manga: u32,
    pub trashed_manga: u32,
    pub possible_duplicates: u32,
    pub imported_categories: u32,
    pub imported_repos: u32,
    pub pending_imports_added: u32,
    pub warnings: Vec<String>,
}

const BACKUP_V2_MAGIC: &[u8] = b"KANI-BACKUP-V2\n";

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let params = Params::new(65536, 3, 1, Some(32))
        .map_err(|e| ServiceError::Internal(format!("Argon2 params: {e}")))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| ServiceError::Internal(format!("Key derivation failed: {e}")))?;
    Ok(key)
}

pub(crate) fn encrypt_backup(zip: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let salt: [u8; 16] = rand::random();
    let nonce_bytes: [u8; 12] = rand::random();
    let key = derive_key(passphrase, &salt)?;
    let cipher = ChaCha20Poly1305::new((&key).into());
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, zip)
        .map_err(|e| ServiceError::Internal(format!("Encryption failed: {e}")))?;
    let mut out = Vec::with_capacity(BACKUP_V2_MAGIC.len() + 16 + 12 + ciphertext.len());
    out.extend_from_slice(BACKUP_V2_MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub(crate) fn decrypt_backup(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    if !data.starts_with(BACKUP_V2_MAGIC) {
        return Err(ServiceError::Validation(
            "Not an encrypted backup (missing magic header)".into(),
        ));
    }
    let rest = &data[BACKUP_V2_MAGIC.len()..];
    if rest.len() < 28 {
        return Err(ServiceError::Validation(
            "Encrypted backup is truncated".into(),
        ));
    }
    let (salt, rest) = rest.split_at(16);
    let (nonce_bytes, ciphertext) = rest.split_at(12);
    let key = derive_key(passphrase, salt)?;
    let cipher = ChaCha20Poly1305::new((&key).into());
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext).map_err(|_| {
        ServiceError::Validation("Decryption failed — wrong passphrase or corrupted backup".into())
    })
}

fn maybe_decrypt_backup<'a>(
    data: &'a [u8],
    passphrase: Option<&str>,
) -> Result<std::borrow::Cow<'a, [u8]>> {
    if data.starts_with(BACKUP_V2_MAGIC) {
        let pp = passphrase.ok_or_else(|| {
            ServiceError::Validation("Backup is encrypted but no passphrase was provided".into())
        })?;
        Ok(std::borrow::Cow::Owned(decrypt_backup(data, pp)?))
    } else {
        Ok(std::borrow::Cow::Borrowed(data))
    }
}

fn parse_zip_backup(data: &[u8]) -> Result<BackupData> {
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| ServiceError::Validation(format!("Not a valid ZIP file: {e}")))?;

    let mut json_bytes = Vec::new();
    {
        let mut file = archive
            .by_name("backup.json")
            .map_err(|_| ServiceError::Validation("ZIP does not contain backup.json".into()))?;
        file.read_to_end(&mut json_bytes)
            .map_err(ServiceError::Io)?;
    }

    let data: BackupData = serde_json::from_slice(&json_bytes)
        .map_err(|e| ServiceError::Validation(format!("Invalid backup JSON: {e}")))?;

    if data.version != 1 {
        return Err(ServiceError::Validation(format!(
            "Unsupported backup version {}",
            data.version
        )));
    }

    Ok(data)
}

fn build_zip(backup: &BackupData) -> Result<Vec<u8>> {
    let buf = Vec::new();
    let cursor = Cursor::new(buf);
    let mut zip = ZipWriter::new(cursor);

    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("backup.json", opts)
        .map_err(|e| ServiceError::Internal(e.to_string()))?;
    let json =
        serde_json::to_vec_pretty(backup).map_err(|e| ServiceError::Internal(e.to_string()))?;
    zip.write_all(&json).map_err(ServiceError::Io)?;

    zip.start_file("VERSION", opts)
        .map_err(|e| ServiceError::Internal(e.to_string()))?;
    zip.write_all(b"1").map_err(ServiceError::Io)?;

    let cursor = zip
        .finish()
        .map_err(|e| ServiceError::Internal(e.to_string()))?;
    Ok(cursor.into_inner())
}

impl AppService {
    pub async fn preview_backup(
        &self,
        data: &[u8],
        passphrase: Option<String>,
    ) -> Result<BackupPreview> {
        let data = maybe_decrypt_backup(data, passphrase.as_deref())?;
        let backup = parse_zip_backup(&data)?;

        let mut source_counts: indexmap::IndexMap<SourceId, u32> = Default::default();
        let mut download_rule_count: u32 = 0;
        let mut has_tracking = false;
        let mut has_chapter_progress = false;

        for m in &backup.manga {
            *source_counts.entry(m.source_name.clone()).or_insert(0) += 1;
            download_rule_count += m.download_rules.len() as u32;
            if m.tracking.is_some() {
                has_tracking = true;
            }
            if !m.chapter_progress.is_empty() {
                has_chapter_progress = true;
            }
        }

        let mut sources = Vec::new();
        for (name, count) in source_counts {
            let found = sqlx::query_scalar!(
                "SELECT id FROM sources WHERE name = ? AND deleted_at IS NULL",
                name
            )
            .fetch_optional(&self.db_read)
            .await?
            .is_some();
            sources.push(BackupSourceSummary {
                source_name: name,
                manga_count: count,
                found,
            });
        }

        Ok(BackupPreview {
            version: backup.version,
            exported_at: backup.exported_at,
            manga_count: backup.manga.len() as u32,
            category_count: backup.categories.len() as u32,
            download_rule_count,
            has_tracking,
            has_chapter_progress,
            has_settings: backup.settings.is_some(),
            repo_count: backup.repos.len() as u32,
            blocked_repo_count: backup.blocked_repos.len() as u32,
            sources,
        })
    }

    pub async fn export_backup(
        &self,
        user_id: UserId,
        include_chapter_progress: bool,
        passphrase: Option<String>,
    ) -> Result<Vec<u8>> {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "unknown".into());

        let rows = sqlx::query!(
            "SELECT m.id, m.source_manga_id, m.name, m.status, m.auto_download, m.auto_scan,
                    m.scanlator_mode, s.name AS source_name
             FROM manga m
             JOIN sources s ON s.id = m.source_id",
        )
        .fetch_all(&self.db_read)
        .await?;

        let mut manga_out = Vec::with_capacity(rows.len());

        for row in rows {
            let categories: Vec<String> = sqlx::query_scalar!(
                "SELECT c.name FROM manga_categories mc \
                 JOIN categories c ON c.id = mc.category_id \
                 WHERE mc.manga_id = ?",
                row.id
            )
            .fetch_all(&self.db_read)
            .await?;

            let tracking = sqlx::query!(
                "SELECT status, score FROM user_manga_tracking \
                 WHERE manga_id = ? AND user_id = ?",
                row.id,
                user_id
            )
            .fetch_optional(&self.db_read)
            .await?
            .map(|r| BackupMangaTracking {
                status: r.status,
                score: r.score,
            });

            let download_rules: Vec<BackupDownloadRule> = sqlx::query!(
                "SELECT rule_type, value FROM download_rules WHERE manga_id = ?",
                row.id
            )
            .fetch_all(&self.db_read)
            .await?
            .into_iter()
            .map(|r| BackupDownloadRule {
                rule_type: r.rule_type,
                value: r.value,
            })
            .collect();

            let chapter_progress = if include_chapter_progress {
                sqlx::query!(
                    "SELECT c.source_chapter_id, uct.is_read, uct.last_page_read \
                     FROM user_chapter_tracking uct \
                     JOIN chapters c ON c.id = uct.chapter_id \
                     WHERE c.manga_id = ? AND uct.user_id = ?",
                    row.id,
                    user_id
                )
                .fetch_all(&self.db_read)
                .await?
                .into_iter()
                .map(|r| BackupChapterProgress {
                    source_chapter_id: r.source_chapter_id,
                    is_read: r.is_read,
                    last_page_read: r.last_page_read,
                })
                .collect()
            } else {
                vec![]
            };

            manga_out.push(BackupManga {
                source_name: SourceId(row.source_name),
                source_manga_id: row.source_manga_id,
                name: row.name,
                status: Some(row.status),
                auto_download: row.auto_download,
                auto_scan: row.auto_scan,
                scanlator_mode: row.scanlator_mode,
                categories,
                tracking,
                download_rules,
                chapter_progress,
            });
        }

        let categories: Vec<BackupCategory> =
            sqlx::query!("SELECT name, sort_order FROM categories ORDER BY sort_order")
                .fetch_all(&self.db_read)
                .await?
                .into_iter()
                .map(|r| BackupCategory {
                    name: r.name,
                    sort_order: r.sort_order,
                })
                .collect();

        // Built from `get_settings` rather than the raw cache: it owns the
        // conversions the cache does not (PathBuf to String, the category-id
        // JSON to Vec<i64>) and it already masks the email credentials.
        let s = self.get_settings().await;
        let settings = Some(BackupSettings {
            // Kept so an older build can still read a backup written here.
            scan_interval_minutes: Some(s.scan_interval_minutes),
            auto_scan: Some(s.auto_scan),
            concurrent_page_downloads: Some(s.concurrent_page_downloads),

            download: Some(DownloadSettings {
                concurrent_page_downloads: s.concurrent_page_downloads,
                max_retries: s.max_retries,
                initial_retry_delay_ms: s.initial_retry_delay_ms,
                auto_download_category_ids: s.auto_download_category_ids.clone(),
                scan_concurrency: s.scan_concurrency,
                per_source_download_concurrency: s.per_source_download_concurrency,
            }),
            scan: Some(ScanSettings {
                auto_scan: s.auto_scan,
                scan_interval_minutes: s.scan_interval_minutes,
                scan_exclude_completed: s.scan_exclude_completed,
                upgrade_detection_enabled: s.upgrade_detection_enabled,
                upgrade_min_res_gain: s.upgrade_min_res_gain,
                upgrade_confirm_fetches: s.upgrade_confirm_fetches,
                upgrade_axis_resolution: s.upgrade_axis_resolution.clone(),
                upgrade_axis_colour: s.upgrade_axis_colour.clone(),
                upgrade_axis_encoder: s.upgrade_axis_encoder.clone(),
                upgrade_axis_bitrate: s.upgrade_axis_bitrate.clone(),
                upgrade_show_downgrades: s.upgrade_show_downgrades,
                upgrade_auto_replace_reasons: s.upgrade_auto_replace_reasons.clone(),
                scan_barren_page_tolerance: s.scan_barren_page_tolerance,
            }),
            advanced: Some(AdvancedSettings {
                flaresolverr_url: s.flaresolverr_url.clone(),
                library_path: s.library_path.clone(),
                wasm_storage_path: s.wasm_storage_path.clone(),
                max_wasm_instances: s.max_wasm_instances,
                http_request_logging: s.http_request_logging,
                v8_debug_logging: s.v8_debug_logging,
                registration_enabled: s.registration_enabled,
                cover_max_dimension: s.cover_max_dimension,
                v8_max_memory_mb: s.v8_max_memory_mb,
                v8_idle_timeout_s: s.v8_idle_timeout_s,
                update_check_enabled: s.update_check_enabled,
                opds_page_index_zero_based: s.opds_page_index_zero_based,
                global_search_timeout_secs: s.global_search_timeout_secs,
            }),
            tracking: Some(TrackingSettings {
                default_tracking_enabled: s.default_tracking_enabled,
                tracker_auto_sync_enabled: s.tracker_auto_sync_enabled,
                tracker_sync_interval_hours: s.tracker_sync_interval_hours,
            }),
            email: Some(EmailSettings {
                email_enabled: s.email_enabled,
                email_provider: s.email_provider.clone(),
                // Already masked by get_settings, so the SMTP password never
                // reaches the file.
                email_provider_config: s.email_provider_config.clone(),
                email_from_address: s.email_from_address.clone(),
                app_url: s.app_url.clone(),
                password_reset_enabled: s.password_reset_enabled,
                email_verification_required: s.email_verification_required,
            }),
            maintenance: Some(MaintenanceSettings {
                trash_retention_days: s.trash_retention_days,
                audit_retention_days: s.audit_retention_days,
                audit_security_retention_days: s.audit_security_retention_days,
                disk_warn_threshold: s.disk_warn_threshold,
                thumbnail_formats: s.thumbnail_formats.clone(),
                integrity_quick_scrub_interval_hours: s.integrity_quick_scrub_interval_hours,
                integrity_deep_scrub_interval_hours: s.integrity_deep_scrub_interval_hours,
                scrub_on_startup: s.scrub_on_startup,
                integrity_revalidate_after_days: s.integrity_revalidate_after_days,
            }),
            security: Some(SecuritySettings {
                max_login_attempts: s.max_login_attempts,
                max_ip_attempts: s.max_ip_attempts,
                login_lockout_seconds: s.login_lockout_seconds,
                session_timeout_secs: s.session_timeout_secs,
            }),
            performance: Some(PerformanceSettings {
                max_concurrent_jobs: s.max_concurrent_jobs,
                db_maintenance_interval_hours: s.db_maintenance_interval_hours,
                db_vacuum_interval_hours: s.db_vacuum_interval_hours,
                audit_prune_interval_hours: s.audit_prune_interval_hours,
                trash_purge_interval_hours: s.trash_purge_interval_hours,
            }),
        });

        let repos: Vec<BackupRepo> = sqlx::query!(
            "SELECT url, name, maintainer_key, trusted_level, index_cache FROM repo_trust ORDER BY url"
        )
        .fetch_all(&self.db_read)
        .await?
        .into_iter()
        .map(|r| BackupRepo {
            url: r.url,
            name: r.name,
            maintainer_key: r.maintainer_key,
            trusted_level: r.trusted_level,
            index_cache: r.index_cache,
        })
        .collect();

        let blocked_repos: Vec<BackupBlockedRepo> =
            sqlx::query!("SELECT url, reason FROM blocked_repos ORDER BY url")
                .fetch_all(&self.db_read)
                .await?
                .into_iter()
                .map(|r| BackupBlockedRepo {
                    url: r.url,
                    reason: r.reason,
                })
                .collect();

        let backup = BackupData {
            version: 1,
            exported_at: now,
            manga: manga_out,
            categories,
            settings,
            repos,
            blocked_repos,
        };

        let zip = build_zip(&backup)?;
        let bytes = match passphrase {
            Some(pp) => encrypt_backup(&zip, &pp)?,
            None => zip,
        };
        self.audit(Some(user_id), "backup.export", None, None).await;
        Ok(bytes)
    }

    pub async fn restore_backup(
        &self,
        user_id: UserId,
        data: &[u8],
        opts: RestoreOptions,
        passphrase: Option<String>,
    ) -> Result<RestoreResult> {
        let data = maybe_decrypt_backup(data, passphrase.as_deref())?;
        let backup = parse_zip_backup(&data)?;

        let mut result = RestoreResult {
            imported_manga: 0,
            skipped_manga: 0,
            trashed_manga: 0,
            possible_duplicates: 0,
            imported_categories: 0,
            imported_repos: 0,
            pending_imports_added: 0,
            warnings: vec![],
        };

        let mut tx = self.db.begin().await?;

        if !opts.merge {
            sqlx::query!(
                "DELETE FROM user_chapter_tracking WHERE user_id = ?",
                user_id
            )
            .execute(&mut *tx)
            .await?;
            sqlx::query!("DELETE FROM user_manga_tracking WHERE user_id = ?", user_id)
                .execute(&mut *tx)
                .await?;
            if opts.import_manga {
                sqlx::query!("DELETE FROM manga_categories")
                    .execute(&mut *tx)
                    .await?;
                sqlx::query!("DELETE FROM download_rules")
                    .execute(&mut *tx)
                    .await?;
                sqlx::query!("DELETE FROM manga").execute(&mut *tx).await?;
            }
            if opts.import_categories {
                sqlx::query!("DELETE FROM categories")
                    .execute(&mut *tx)
                    .await?;
            }
        }

        let mut cat_map: std::collections::HashMap<String, i64> = Default::default();
        if opts.import_categories {
            for cat in &backup.categories {
                sqlx::query!(
                    "INSERT OR IGNORE INTO categories (name, sort_order) VALUES (?, ?)",
                    cat.name,
                    cat.sort_order
                )
                .execute(&mut *tx)
                .await?;
                result.imported_categories += 1;
            }
        }
        let cats = sqlx::query!("SELECT id, name FROM categories")
            .fetch_all(&mut *tx)
            .await?;
        for c in cats {
            cat_map.insert(c.name, c.id);
        }

        tx.commit().await?;

        let mut new_manga_ids: Vec<i64> = Vec::new();

        let total_manga = if opts.import_manga {
            backup.manga.len() as u32
        } else {
            0
        };
        if total_manga > 0 {
            let _ = self.refresh_tx.send(AppEvent::ImportStarted {
                origin: "kani_backup".into(),
                total: total_manga,
            });
        }
        let mut title_index: std::collections::HashMap<
            i64,
            std::collections::HashMap<String, Vec<i64>>,
        > = Default::default();

        for (processed, m) in (1_u32..).zip(backup.manga.iter()) {
            let source_id: Option<i64> = sqlx::query_scalar!(
                "SELECT id FROM sources WHERE name = ? AND deleted_at IS NULL",
                m.source_name
            )
            .fetch_optional(&self.db_read)
            .await?;

            let source_id = match source_id {
                Some(id) => id,
                None => {
                    if !opts.import_manga {
                        result.skipped_manga += 1;
                        continue;
                    }
                    result.warnings.push(format!(
                        "Source '{}' not installed — '{}' saved to pending imports",
                        m.source_name, m.name
                    ));
                    self.save_pending_import_for_user(user_id, "kani_backup", m, None, None)
                        .await?;
                    result.pending_imports_added += 1;
                    result.skipped_manga += 1;
                    let _ = self.refresh_tx.send(AppEvent::ImportProgress {
                        origin: "kani_backup".into(),
                        completed: processed,
                        total: total_manga,
                        title: m.name.clone(),
                    });
                    continue;
                }
            };

            let mut existing_id: Option<i64> = sqlx::query_scalar!(
                "SELECT id FROM manga WHERE source_id = ? AND source_manga_id = ?",
                source_id,
                m.source_manga_id
            )
            .fetch_optional(&self.db_read)
            .await?;

            if existing_id.is_none() {
                use std::collections::hash_map::Entry;
                let index = match title_index.entry(source_id) {
                    Entry::Occupied(e) => e.into_mut(),
                    Entry::Vacant(e) => e.insert(
                        crate::service::dedup::source_title_index(&self.db_read, source_id).await?,
                    ),
                };
                // An ambiguous title matches nothing: attaching a backup's
                // progress to the wrong series is worse than a duplicate.
                existing_id = index
                    .get(&crate::service::dedup::normalise_title(&m.name))
                    .filter(|ids| ids.len() == 1)
                    .map(|ids| ids[0]);
            }

            if let Some(id) = existing_id {
                let trashed: Option<i64> = sqlx::query_scalar!(
                    "SELECT id FROM manga WHERE id = ? AND deleted_at IS NOT NULL",
                    id
                )
                .fetch_optional(&self.db_read)
                .await?;
                if trashed.is_some() {
                    result.trashed_manga += 1;
                    let _ = self.refresh_tx.send(AppEvent::ImportProgress {
                        origin: "kani_backup".into(),
                        completed: processed,
                        total: total_manga,
                        title: m.name.clone(),
                    });
                    continue;
                }
            }

            // Without `import_manga` the run may only touch what is already in
            // the library, so an entry with nothing to attach to is skipped
            // rather than added under another option's name.
            if existing_id.is_none() && !opts.import_manga {
                result.skipped_manga += 1;
                let _ = self.refresh_tx.send(AppEvent::ImportProgress {
                    origin: "kani_backup".into(),
                    completed: processed,
                    total: total_manga,
                    title: m.name.clone(),
                });
                continue;
            }

            let (manga_id, is_new) = if let Some(id) = existing_id {
                (id, false)
            } else {
                let authors: Vec<String> = vec![];
                let hits =
                    crate::service::dedup::find_similar_manga(&self.db, &m.name, &authors, None)
                        .await?;
                if !hits.is_empty() {
                    self.save_pending_import_for_user(
                        user_id,
                        "kani_backup",
                        m,
                        Some(hits[0].id),
                        Some(hits[0].similarity),
                    )
                    .await?;
                    result.possible_duplicates += 1;
                    result.pending_imports_added += 1;
                    let _ = self.refresh_tx.send(AppEvent::ImportProgress {
                        origin: "kani_backup".into(),
                        completed: processed,
                        total: total_manga,
                        title: m.name.clone(),
                    });
                    continue;
                }

                let mut tx = self.db.begin().await?;
                let status = m.status.unwrap_or(0);
                let id = sqlx::query_scalar!(
                    "INSERT INTO manga (source_id, source_manga_id, name, status, auto_download, auto_scan, scanlator_mode) \
                     VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
                    source_id,
                    m.source_manga_id,
                    m.name,
                    status,
                    m.auto_download,
                    m.auto_scan,
                    m.scanlator_mode
                )
                .fetch_one(&mut *tx)
                .await?;

                for cat_name in &m.categories {
                    if let Some(&cat_id) = cat_map.get(cat_name) {
                        sqlx::query!(
                            "INSERT OR IGNORE INTO manga_categories (manga_id, category_id) VALUES (?, ?)",
                            id,
                            cat_id
                        )
                        .execute(&mut *tx)
                        .await?;
                    }
                }

                if opts.import_download_rules {
                    for rule in &m.download_rules {
                        sqlx::query!(
                            "INSERT INTO download_rules (manga_id, rule_type, value) VALUES (?, ?, ?)",
                            id,
                            rule.rule_type,
                            rule.value
                        )
                        .execute(&mut *tx)
                        .await?;
                    }
                }

                tx.commit().await?;
                (id, true)
            };

            // Restored manga need the same follow-up as ones added from a
            // source, chapters above all: without them there is nothing to
            // read, and nothing for chapter progress below to attach to.
            if is_new {
                self.after_manga_added(
                    MangaId(manga_id),
                    source_id,
                    &m.source_manga_id,
                    &m.name,
                    None,
                    true,
                )
                .await;
            }

            if opts.import_tracking
                && let Some(ref tr) = m.tracking
            {
                // Updated rather than replaced: REPLACE would reset every column the
                // backup does not carry, including this user's reader settings.
                sqlx::query!(
                    "INSERT INTO user_manga_tracking (user_id, manga_id, status, score) \
                         VALUES (?, ?, ?, ?) \
                         ON CONFLICT(user_id, manga_id) DO UPDATE SET \
                         status = excluded.status, score = excluded.score",
                    user_id,
                    manga_id,
                    tr.status,
                    tr.score
                )
                .execute(&self.db)
                .await?;
            }

            if opts.import_chapter_progress && !m.chapter_progress.is_empty() {
                let mut unmatched = 0u32;
                for cp in &m.chapter_progress {
                    // A Kani backup carries no chapter number, so zero here
                    // leaves the id the only thing this can match on.
                    let applied = self
                        .apply_one_progress(
                            MangaId(manga_id),
                            user_id,
                            &cp.source_chapter_id,
                            0.0,
                            cp.is_read,
                            cp.last_page_read,
                        )
                        .await?;
                    if !applied {
                        unmatched += 1;
                    }
                }
                if unmatched > 0 {
                    result.warnings.push(format!(
                        "Read progress for '{}': {unmatched} entries matched none of its chapters in the library.",
                        m.name
                    ));
                }
            }

            new_manga_ids.push(manga_id);
            result.imported_manga += 1;
            let _ = self.refresh_tx.send(AppEvent::ImportProgress {
                origin: "kani_backup".into(),
                completed: processed,
                total: total_manga,
                title: m.name.clone(),
            });
        }

        if total_manga > 0 {
            let _ = self.refresh_tx.send(AppEvent::ImportCompleted {
                origin: "kani_backup".into(),
                imported: result.imported_manga,
                skipped: result.skipped_manga,
                pending: result.pending_imports_added,
            });
        }

        if opts.import_settings
            && let Some(ref s) = backup.settings
        {
            // Restore through validation and cache refresh, substituting stored email credentials
            // for the masked values carried by the backup.
            let mut applied = false;
            if let Some(g) = s.download.clone() {
                self.update_settings(SettingsUpdate::Download(g), user_id)
                    .await?;
                applied = true;
            }
            if let Some(g) = s.scan.clone() {
                self.update_settings(SettingsUpdate::Scan(g), user_id)
                    .await?;
                applied = true;
            }
            if let Some(g) = s.advanced.clone() {
                self.update_settings(SettingsUpdate::Advanced(g), user_id)
                    .await?;
                applied = true;
            }
            if let Some(g) = s.tracking.clone() {
                self.update_settings(SettingsUpdate::Tracking(g), user_id)
                    .await?;
                applied = true;
            }
            if let Some(g) = s.email.clone() {
                self.update_settings(SettingsUpdate::Email(g), user_id)
                    .await?;
                applied = true;
            }
            if let Some(g) = s.maintenance.clone() {
                self.update_settings(SettingsUpdate::Maintenance(g), user_id)
                    .await?;
                applied = true;
            }
            if let Some(g) = s.security.clone() {
                self.update_settings(SettingsUpdate::Security(g), user_id)
                    .await?;
                applied = true;
            }
            if let Some(g) = s.performance.clone() {
                self.update_settings(SettingsUpdate::Performance(g), user_id)
                    .await?;
                applied = true;
            }

            // Legacy backups change only their three flat settings through the current update path.
            if !applied {
                let cur = self.get_settings().await;
                if s.scan_interval_minutes.is_some() || s.auto_scan.is_some() {
                    self.update_settings(
                        SettingsUpdate::Scan(ScanSettings {
                            auto_scan: s.auto_scan.unwrap_or(cur.auto_scan),
                            scan_interval_minutes: s
                                .scan_interval_minutes
                                .unwrap_or(cur.scan_interval_minutes),
                            scan_exclude_completed: cur.scan_exclude_completed,
                            upgrade_detection_enabled: cur.upgrade_detection_enabled,
                            upgrade_min_res_gain: cur.upgrade_min_res_gain,
                            upgrade_confirm_fetches: cur.upgrade_confirm_fetches,
                            upgrade_axis_resolution: cur.upgrade_axis_resolution.clone(),
                            upgrade_axis_colour: cur.upgrade_axis_colour.clone(),
                            upgrade_axis_encoder: cur.upgrade_axis_encoder.clone(),
                            upgrade_axis_bitrate: cur.upgrade_axis_bitrate.clone(),
                            upgrade_show_downgrades: cur.upgrade_show_downgrades,
                            upgrade_auto_replace_reasons: cur.upgrade_auto_replace_reasons.clone(),
                            scan_barren_page_tolerance: cur.scan_barren_page_tolerance,
                        }),
                        user_id,
                    )
                    .await?;
                }
                if let Some(cpd) = s.concurrent_page_downloads {
                    self.update_settings(
                        SettingsUpdate::Download(DownloadSettings {
                            concurrent_page_downloads: cpd,
                            max_retries: cur.max_retries,
                            initial_retry_delay_ms: cur.initial_retry_delay_ms,
                            auto_download_category_ids: cur.auto_download_category_ids.clone(),
                            scan_concurrency: cur.scan_concurrency,
                            per_source_download_concurrency: cur.per_source_download_concurrency,
                        }),
                        user_id,
                    )
                    .await?;
                }
            }
        }

        if opts.import_repos {
            for repo in &backup.repos {
                sqlx::query!(
                    "INSERT INTO repo_trust (url, name, maintainer_key, trusted_level, index_cache) \
                     VALUES (?, ?, ?, ?, ?) \
                     ON CONFLICT(url) DO UPDATE SET \
                         name = excluded.name, \
                         maintainer_key = excluded.maintainer_key, \
                         trusted_level = excluded.trusted_level, \
                         index_cache = excluded.index_cache",
                    repo.url,
                    repo.name,
                    repo.maintainer_key,
                    repo.trusted_level,
                    repo.index_cache
                )
                .execute(&self.db)
                .await?;
                result.imported_repos += 1;
            }
            for blocked in &backup.blocked_repos {
                sqlx::query!(
                    "INSERT INTO blocked_repos (url, reason) VALUES (?, ?) \
                     ON CONFLICT(url) DO UPDATE SET reason = excluded.reason",
                    blocked.url,
                    blocked.reason
                )
                .execute(&self.db)
                .await?;
            }
        }

        if !new_manga_ids.is_empty() {
            let pool = self.db.clone();
            tokio::spawn(async move {
                for id in new_manga_ids {
                    if let Err(e) =
                        crate::service::dedup::record_duplicates_for_manga(&pool, MangaId(id)).await
                    {
                        tracing::warn!("Duplicate recording failed for manga {id}: {e}");
                    }
                }
            });
        }

        self.cache.invalidate_stats(user_id);
        self.audit(
            Some(user_id),
            "backup.restore",
            None,
            Some(serde_json::json!({
                "imported": result.imported_manga,
                "skipped": result.skipped_manga,
                "pending": result.pending_imports_added,
            })),
        )
        .await;

        Ok(result)
    }

    /// Save a manga that couldn't be matched to the pending imports queue.
    pub(crate) async fn save_pending_import_for_user(
        &self,
        user_id: UserId,
        origin: &str,
        m: &BackupManga,
        duplicate_of: Option<MangaId>,
        similarity: Option<f64>,
    ) -> Result<()> {
        let tracking = m
            .tracking
            .as_ref()
            .and_then(|t| serde_json::to_string(t).ok());
        let chapter_progress = if m.chapter_progress.is_empty() {
            None
        } else {
            serde_json::to_string(&m.chapter_progress).ok()
        };

        sqlx::query!(
            "INSERT INTO pending_imports \
             (user_id, origin, title, source_hint, source_manga_id, tracking, chapter_progress, \
              possible_duplicate_of, duplicate_similarity) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            user_id,
            origin,
            m.name,
            m.source_name,
            m.source_manga_id,
            tracking,
            chapter_progress,
            duplicate_of,
            similarity
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn dummy_zip() -> Vec<u8> {
        let backup = BackupData {
            version: 1,
            exported_at: "2026-01-01T00:00:00Z".into(),
            manga: vec![],
            categories: vec![],
            settings: None,
            repos: vec![],
            blocked_repos: vec![],
        };
        build_zip(&backup).unwrap()
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let zip = dummy_zip();
        let encrypted = encrypt_backup(&zip, "hunter2").unwrap();
        assert!(encrypted.starts_with(BACKUP_V2_MAGIC));
        assert!(encrypted.len() > zip.len());
        let decrypted = decrypt_backup(&encrypted, "hunter2").unwrap();
        assert_eq!(decrypted, zip);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let zip = dummy_zip();
        let encrypted = encrypt_backup(&zip, "correct").unwrap();
        assert!(decrypt_backup(&encrypted, "wrong").is_err());
    }

    #[test]
    fn magic_header_detected() {
        let zip = dummy_zip();
        assert!(!zip.starts_with(BACKUP_V2_MAGIC));
        let encrypted = encrypt_backup(&zip, "pass").unwrap();
        assert!(encrypted.starts_with(BACKUP_V2_MAGIC));
    }

    #[test]
    fn maybe_decrypt_plaintext_passthrough() {
        let zip = dummy_zip();
        let cow = maybe_decrypt_backup(&zip, None).unwrap();
        assert_eq!(&*cow, &zip[..]);
    }

    #[test]
    fn maybe_decrypt_encrypted_without_passphrase_errors() {
        let zip = dummy_zip();
        let encrypted = encrypt_backup(&zip, "pass").unwrap();
        assert!(maybe_decrypt_backup(&encrypted, None).is_err());
    }

    #[test]
    fn encrypted_backup_parse_roundtrip() {
        let zip = dummy_zip();
        let encrypted = encrypt_backup(&zip, "secret!").unwrap();
        let decrypted = decrypt_backup(&encrypted, "secret!").unwrap();
        let parsed = parse_zip_backup(&decrypted).unwrap();
        assert_eq!(parsed.version, 1);
    }
}
