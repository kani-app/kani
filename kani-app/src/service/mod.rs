//! Database-backed application service and its domain-specific operation modules.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use dashmap::DashMap;
use indexmap::IndexMap;
use kani_shared::wit_types;
use ordered_float::OrderedFloat;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

use kani_core::downloader::{DownloadTask, DownloaderConfig, DownloaderManager};
use kani_core::wasm::WasmRuntime;

use crate::source::{SourceRegistry, loader};

use crate::cache::RequestCache;
use crate::error::{Result, ServiceError};
use crate::events::{AppEvent, RefreshProgressEvent};
use crate::ids::MangaId;
use crate::models::{DownloadRuleRow, Settings};
use kani_shared::types::{
    ChapterFilterRow, DownloadRule, DownloadRuleKind, GlobalSearchResult, MangaList,
    MigrationPreview, MigrationResult, SearchScope, Source,
};
use trackers::TrackerRegistry;

pub mod api_tokens;
pub mod archive;
mod audit;
pub mod backup;
pub mod backup_scheduler;
mod categories;
mod chapters;
mod cover;
pub mod credential_migration;
pub mod dedup;
pub mod degradations;
pub mod diagnostics;
mod downloads;
pub mod email;
pub mod email_templates;
pub mod email_verification;
pub mod encryption;
pub mod export;
mod filters;
pub mod fs_browse;
mod id_repair;
pub mod import;
pub mod integrity;
pub mod library;
pub mod local_network;
pub mod manifest_capture;
pub mod metadata_provider;
pub mod migration;
mod migration_checksums;
pub mod opds;
pub mod password_policy;
pub mod password_reset;
pub mod path_migration;
pub mod pending_imports;
mod preferences;
pub(crate) mod progress;
pub mod quality;
pub mod repos;
pub mod saved_searches;
mod scanlators;
pub mod sessions;
mod settings;
pub mod smart_collections;
mod sources;
mod stats;
pub mod support_bundle;
pub mod thumbnails;
pub mod totp;
pub mod trackers;
pub mod traits;
pub mod ui_ext;
pub mod update_check;
pub mod volumes;
pub mod webhooks;

#[derive(Clone)]
/// Cloneable application boundary shared by HTTP handlers and background jobs.
///
/// Clones share database pools, runtime registries, caches, cancellation, and side-effect services;
/// cloning does not create an isolated transaction or source-runtime instance.
pub struct AppService {
    pub db: SqlitePool,
    /// Where the database actually lives. Several callers need the sidecar
    /// files (`-wal`, `-shm`) and must not assume the working directory.
    pub db_path: std::path::PathBuf,
    pub db_read: SqlitePool,
    pub wasm_runtime: Arc<WasmRuntime>,
    pub sources: Arc<SourceRegistry>,
    pub settings: Arc<tokio::sync::RwLock<Settings>>,
    pub downloader: DownloaderManager,
    pub smart_client: kani_core::http::SmartClient,
    pub proxy_client: kani_core::http::SmartClient,
    /// Per-source (extraction, image) clients that honour a source's local-network grant.
    pub(crate) source_clients:
        Arc<dashmap::DashMap<i64, (kani_core::http::SmartClient, kani_core::http::SmartClient)>>,
    pub refresh_tx: tokio::sync::broadcast::Sender<AppEvent>,
    pub refresh_task: Arc<tokio::sync::Mutex<Option<tokio::task::AbortHandle>>>,
    pub cache: RequestCache,
    pub ext_cache: std::sync::Arc<dyn kani_core::cache::CacheBackend>,
    pub shutdown_token: tokio_util::sync::CancellationToken,
    pub tracker_registry: Arc<tokio::sync::RwLock<TrackerRegistry>>,
    /// Manga IDs whose cover download failed and should be retried.
    pub cover_retry_queue: Arc<tokio::sync::Mutex<HashSet<MangaId>>>,
    pub email_service: Arc<tokio::sync::RwLock<Option<email::EmailService>>>,
    /// Optional authenticated-encryption cipher for credential fields.
    /// Present when `KANI_SECRET_KEY` or `KANI_SECRET_KEY_FILE` is set at startup.
    pub encryption: Option<Arc<encryption::CredentialCipher>>,
    /// Subsystems running in a reduced state. Surfaced in Settings → Diagnostics.
    pub degradations: Arc<degradations::DegradationRegistry>,
    pub webhook_service: webhooks::WebhookService,
    pub metadata_provider_registry:
        Arc<tokio::sync::RwLock<metadata_provider::MetadataProviderRegistry>>,
    pub job_manager: crate::jobs::JobManager,
    pub latest_version: Arc<tokio::sync::RwLock<Option<update_check::UpdateInfo>>>,
    /// Per-extension install/update locks, keyed by extension id. Serializes
    /// concurrent install/update of the same extension so the upsert (which keys on
    /// `sources.name`) cannot race two writers into duplicate rows + backends.
    pub install_locks: Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub(crate) progress_buffer: progress::ReadProgressBuffer,
    pub(crate) undo_tokens: Arc<DashMap<uuid::Uuid, (crate::ids::MangaId, std::time::Instant)>>,
    #[cfg(any(test, feature = "test-util"))]
    pub mock_sources: Arc<DashMap<i64, Arc<dyn kani_core::downloader::PageListFetcher>>>,
}

/// Load a credential cipher from env vars, or auto-provision one at `data_dir/secret.key`.
///
/// Priority order:
/// 1. `KANI_SECRET_KEY_FILE` — path to a file containing a 64-char hex key.
/// 2. `KANI_SECRET_KEY` — inline hex key.
/// 3. Auto-provision: read `data_dir/secret.key`, or generate and persist it on first boot.
///
/// The auto-provisioned file is written with `0o600` permissions (owner-read only on Unix).
fn load_or_provision_credential_cipher(
    data_dir: &std::path::Path,
    reg: &degradations::DegradationRegistry,
) -> Option<encryption::CredentialCipher> {
    if let Ok(path) = std::env::var("KANI_SECRET_KEY_FILE") {
        let hex = match std::fs::read_to_string(path.trim()) {
            Ok(s) => s.trim().to_string(),
            Err(e) => {
                reg.register(
                    degradations::ids::CREDENTIAL_KEY,
                    degradations::Severity::Error,
                    "Credential encryption",
                    format!("KANI_SECRET_KEY_FILE is set but the file could not be read ({e}), so credential encryption is disabled."),
                    "Check the path and that the server user can read it.",
                );
                return None;
            }
        };
        return parse_cipher_hex(&hex, reg);
    }
    if let Ok(val) = std::env::var("KANI_SECRET_KEY") {
        return parse_cipher_hex(val.trim(), reg);
    }

    let key_path = data_dir.join("secret.key");
    let hex = if key_path.exists() {
        match std::fs::read_to_string(&key_path) {
            Ok(s) => s.trim().to_string(),
            Err(e) => {
                tracing::error!("Failed to read {}: {e}", key_path.display());
                return None;
            }
        }
    } else {
        let key: [u8; 32] = rand::random();
        let hex = hex::encode(key);
        if let Err(e) = std::fs::write(&key_path, &hex) {
            tracing::error!(
                "Failed to write credential key to {}: {e}",
                key_path.display()
            );
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
        }
        tracing::info!(
            "Auto-provisioned credential encryption key at {}. \
             Back this file up alongside your database.",
            key_path.display()
        );
        hex
    };
    parse_cipher_hex(&hex, reg)
}

fn parse_cipher_hex(
    hex: &str,
    reg: &degradations::DegradationRegistry,
) -> Option<encryption::CredentialCipher> {
    match encryption::CredentialCipher::from_hex(hex) {
        Ok(c) => {
            tracing::info!("Credential encryption enabled");
            Some(c)
        }
        Err(e) => {
            reg.register(
                degradations::ids::CREDENTIAL_KEY,
                degradations::Severity::Error,
                "Credential encryption",
                format!(
                    "The encryption key is not usable, so credential encryption is disabled: {e}"
                ),
                "Check KANI_SECRET_KEY / KANI_SECRET_KEY_FILE, or the secret.key file in the data \
                 directory — it must be 64 hex characters.",
            );
            None
        }
    }
}

/// Takes a filesystem path rather than a URL: the database lives wherever
/// `KANI_DATA_DIR` says, and a path with a space, a `?` or a `#` in it cannot be
/// pasted into a `sqlite://…` string without corrupting the URL.
fn connect_options(
    path: &std::path::Path,
    slow_query_threshold_ms: u64,
) -> sqlx::sqlite::SqliteConnectOptions {
    use sqlx::ConnectOptions;

    sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .log_statements(log::LevelFilter::Debug)
        .log_slow_statements(
            log::LevelFilter::Warn,
            std::time::Duration::from_millis(slow_query_threshold_ms),
        )
}

async fn make_write_pool(
    path: &std::path::Path,
    slow_query_threshold_ms: u64,
) -> sqlx::Result<SqlitePool> {
    SqlitePoolOptions::new()
        .max_connections(1)
        .after_connect(|conn, _| {
            Box::pin(async move {
                apply_pragmas(conn, false).await?;
                Ok(())
            })
        })
        .connect_with(connect_options(path, slow_query_threshold_ms))
        .await
}

async fn make_read_pool(
    path: &std::path::Path,
    size: u32,
    slow_query_threshold_ms: u64,
) -> sqlx::Result<SqlitePool> {
    SqlitePoolOptions::new()
        .max_connections(size)
        .after_connect(|conn, _| {
            Box::pin(async move {
                apply_pragmas(conn, true).await?;
                Ok(())
            })
        })
        .connect_with(connect_options(path, slow_query_threshold_ms))
        .await
}

async fn apply_pragmas(
    conn: &mut sqlx::sqlite::SqliteConnection,
    query_only: bool,
) -> sqlx::Result<()> {
    let pragmas = [
        "PRAGMA journal_mode=WAL;",
        "PRAGMA synchronous=NORMAL;",
        "PRAGMA busy_timeout=5000;",
        "PRAGMA foreign_keys=ON;",
        "PRAGMA cache_size=-65536;",
        "PRAGMA temp_store=MEMORY;",
        "PRAGMA mmap_size=1073741824;",
        "PRAGMA wal_autocheckpoint=1000;",
    ];
    for pragma in pragmas {
        sqlx::query(pragma).execute(&mut *conn).await?;
    }
    if query_only {
        sqlx::query("PRAGMA query_only=ON;")
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

impl AppService {
    pub async fn new(data_dir: &std::path::Path) -> Result<Self> {
        let db_path = data_dir.join("kani.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let read_pool_size: u32 = std::env::var("KANI_DB_READ_POOL_SIZE")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(8);
        let slow_query_threshold_ms: u64 = std::env::var("KANI_SLOW_QUERY_THRESHOLD_MS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(100);
        let pool = make_write_pool(&db_path, slow_query_threshold_ms).await?;
        let read_pool = make_read_pool(&db_path, read_pool_size, slow_query_threshold_ms).await?;

        tracing::info!("SQL Pool Created");

        migration_checksums::run(&pool).await?;

        let sources_registry = SourceRegistry::new();

        let degradation_registry = Arc::new(degradations::DegradationRegistry::new());
        let enc = load_or_provision_credential_cipher(data_dir, &degradation_registry);

        let mut settings = sqlx::query_as!(Settings, "SELECT flaresolverr_url, library_path, wasm_storage_path, concurrent_page_downloads, max_retries, initial_retry_delay_ms, max_wasm_instances, auto_scan, scan_interval_minutes, scan_exclude_completed, auto_download_category_ids, default_tracking_enabled, http_request_logging, v8_debug_logging, registration_enabled, cover_max_dimension, email_enabled, email_provider, email_provider_config, email_from_address, app_url, password_reset_enabled, email_verification_required, first_run_complete, scan_concurrency, per_source_download_concurrency, job_max_history, job_shutdown_timeout_secs, trash_retention_days, audit_retention_days, audit_security_retention_days, disk_warn_threshold, thumbnail_formats, max_login_attempts, max_ip_attempts, login_lockout_seconds, session_timeout_secs, tracker_auto_sync_enabled, tracker_sync_interval_hours, max_concurrent_jobs, db_maintenance_interval_hours, db_vacuum_interval_hours, audit_prune_interval_hours, trash_purge_interval_hours, v8_max_memory_mb, v8_idle_timeout_s, update_check_enabled, integrity_quick_scrub_interval_hours, integrity_deep_scrub_interval_hours, scrub_on_startup, integrity_revalidate_after_days, upgrade_detection_enabled, upgrade_min_res_gain, upgrade_confirm_fetches, upgrade_axis_resolution, upgrade_axis_colour, upgrade_axis_encoder, upgrade_axis_bitrate, upgrade_show_downgrades, upgrade_auto_replace_reasons, opds_page_index_zero_based, scan_barren_page_tolerance, global_search_timeout_secs FROM settings")
            .fetch_one(&pool)
            .await?;
        tracing::info!("Settings retrieved");
        kani_core::v8_process::set_v8_debug_logging(settings.v8_debug_logging);
        kani_core::v8_process::set_v8_config(kani_core::v8_process::V8Config {
            max_memory_mb: settings.v8_max_memory_mb as u32,
            idle_timeout_s: settings.v8_idle_timeout_s as u32,
        });

        // Decrypt email_provider_config so in-memory value is always plaintext.
        if let Some(ref cipher) = enc {
            match cipher.decrypt(&settings.email_provider_config) {
                Ok(plain) => settings.email_provider_config = plain,
                Err(e) => degradation_registry.register(
                    degradations::ids::ENCRYPTED_SETTINGS,
                    degradations::Severity::Error,
                    "Encrypted settings",
                    format!("The stored email provider configuration cannot be decrypted: {e}. Email sending will not work."),
                    "This normally means secret.key was lost or replaced. Restore the \
                     original key file, or re-enter the email settings to store them \
                     under the new key.",
                ),
            }
        }

        // KANI_ALLOW_REGISTRATION=false disables new user registration at startup,
        // overriding the database setting. Useful for instances exposed to the internet.
        if let Ok(val) = std::env::var("KANI_ALLOW_REGISTRATION") {
            let allow = val.trim() != "false" && val.trim() != "0";
            if !allow {
                settings.registration_enabled = false;
                tracing::info!("Registration disabled by KANI_ALLOW_REGISTRATION=false");
            }
        }

        if let Ok(dir) = std::env::var("KANI_LIBRARY_DIR") {
            tracing::info!("Library path overridden by KANI_LIBRARY_DIR: {dir}");
            settings.library_path = std::path::PathBuf::from(dir);
        }

        // On first boot only, apply env var overrides for concurrency settings and
        // persist them so subsequent boots use the DB value (precedence: DB > env > tuning).
        if !settings.first_run_complete {
            let mut concurrency_changed = false;
            if let Ok(val) = std::env::var("KANI_SCAN_CONCURRENCY")
                && let Ok(n) = val.trim().parse::<i64>()
                && (1..=32).contains(&n)
            {
                tracing::info!("scan_concurrency set to {n} by KANI_SCAN_CONCURRENCY");
                settings.scan_concurrency = n;
                concurrency_changed = true;
            }
            if let Ok(val) = std::env::var("KANI_PER_SOURCE_DOWNLOAD_CONCURRENCY")
                && let Ok(n) = val.trim().parse::<i64>()
                && (1..=16).contains(&n)
            {
                tracing::info!(
                    "per_source_download_concurrency set to {n} by KANI_PER_SOURCE_DOWNLOAD_CONCURRENCY"
                );
                settings.per_source_download_concurrency = n;
                concurrency_changed = true;
            }
            if concurrency_changed {
                sqlx::query!(
                    "UPDATE settings SET scan_concurrency = ?, per_source_download_concurrency = ? WHERE id = 'singleton'",
                    settings.scan_concurrency,
                    settings.per_source_download_concurrency,
                )
                .execute(&pool)
                .await?;
            }

            let mut env_settings_changed = false;
            if let Ok(val) = std::env::var("KANI_TRASH_RETENTION_DAYS")
                && let Ok(n) = val.trim().parse::<i64>()
                && n >= 0
            {
                settings.trash_retention_days = n;
                env_settings_changed = true;
            }
            if let Ok(val) = std::env::var("KANI_AUDIT_RETENTION_DAYS")
                && let Ok(n) = val.trim().parse::<i64>()
                && n >= 0
            {
                settings.audit_retention_days = n;
                env_settings_changed = true;
            }
            if let Ok(val) = std::env::var("KANI_AUDIT_SECURITY_RETENTION_DAYS")
                && let Ok(n) = val.trim().parse::<i64>()
                && n >= 0
            {
                settings.audit_security_retention_days = n;
                env_settings_changed = true;
            }
            if let Ok(val) = std::env::var("KANI_DISK_WARN_THRESHOLD")
                && let Ok(n) = val.trim().parse::<f64>()
                && (0.0..=1.0).contains(&n)
            {
                settings.disk_warn_threshold = n;
                env_settings_changed = true;
            }
            if let Ok(val) = std::env::var("KANI_THUMBNAIL_FORMATS")
                && !val.trim().is_empty()
            {
                settings.thumbnail_formats = val.trim().to_string();
                env_settings_changed = true;
            }
            if let Ok(val) = std::env::var("KANI_MAX_LOGIN_ATTEMPTS")
                && let Ok(n) = val.trim().parse::<i64>()
                && n >= 1
            {
                settings.max_login_attempts = n;
                env_settings_changed = true;
            }
            if let Ok(val) = std::env::var("KANI_MAX_IP_ATTEMPTS")
                && let Ok(n) = val.trim().parse::<i64>()
                && n >= 1
            {
                settings.max_ip_attempts = n;
                env_settings_changed = true;
            }
            if let Ok(val) = std::env::var("KANI_LOGIN_LOCKOUT_SECONDS")
                && let Ok(n) = val.trim().parse::<i64>()
                && n >= 1
            {
                settings.login_lockout_seconds = n;
                env_settings_changed = true;
            }
            if let Ok(val) = std::env::var("KANI_SESSION_TIMEOUT_SECONDS")
                && let Ok(n) = val.trim().parse::<i64>()
                && n >= 60
            {
                settings.session_timeout_secs = n;
                env_settings_changed = true;
            }
            if env_settings_changed {
                sqlx::query!(
                    "UPDATE settings SET trash_retention_days = ?, audit_retention_days = ?, \
                     audit_security_retention_days = ?, disk_warn_threshold = ?, thumbnail_formats = ?, \
                     max_login_attempts = ?, max_ip_attempts = ?, login_lockout_seconds = ?, \
                     session_timeout_secs = ? WHERE id = 'singleton'",
                    settings.trash_retention_days,
                    settings.audit_retention_days,
                    settings.audit_security_retention_days,
                    settings.disk_warn_threshold,
                    settings.thumbnail_formats,
                    settings.max_login_attempts,
                    settings.max_ip_attempts,
                    settings.login_lockout_seconds,
                    settings.session_timeout_secs,
                )
                .execute(&pool)
                .await?;
            }
        }

        let max_wasm_instances = settings.max_wasm_instances as u32;
        let wasm_runtime = {
            let mut runtime = WasmRuntime::new(max_wasm_instances).map_err(ServiceError::Core)?;
            if let Err((dir, e)) = runtime.attach_module_cache(data_dir) {
                degradation_registry.register(
                    degradations::ids::WASM_MODULE_CACHE,
                    degradations::Severity::Warn,
                    "WASM module cache",
                    format!(
                        "The compiled-module cache at {} is unavailable ({e}), so every \
                             extension is recompiled on each load.",
                        dir.display()
                    ),
                    "Make the directory writable by the server user, or point \
                         KANI_WASM_MODULE_CACHE_DIR somewhere that is.",
                );
            }
            Arc::new(runtime)
        };

        let shutdown_token = tokio_util::sync::CancellationToken::new();

        let engine_for_ticker = wasm_runtime.engine().clone();
        let ticker_token = shutdown_token.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(
                crate::tuning::WASM_EPOCH_TICK_MS,
            ));
            loop {
                tokio::select! {
                    _ = ticker_token.cancelled() => {
                        tracing::info!("Epoch ticker shutting down");
                        break;
                    }
                    _ = interval.tick() => {
                        engine_for_ticker.increment_epoch();
                    }
                }
            }
        });

        // Resolved and created with the other storage directories so a missing
        // or unwritable path surfaces at boot, not when the scheduled job fires
        // in the middle of the night.
        let backup_dir: std::path::PathBuf = sqlx::query_scalar::<_, Option<String>>(
            "SELECT backup_schedule_json FROM settings WHERE id = 'singleton'",
        )
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .flatten()
        .and_then(|json| serde_json::from_str::<backup_scheduler::BackupScheduleConfig>(&json).ok())
        .map(|cfg| match cfg.destination {
            backup_scheduler::BackupDestination::Local { path } => path,
        })
        .unwrap_or_else(|| std::path::PathBuf::from(backup_scheduler::DEFAULT_BACKUP_DIR));

        for (label, dir) in [
            ("WASM storage", settings.wasm_storage_path.as_path()),
            ("library", settings.library_path.as_path()),
            ("backup", backup_dir.as_path()),
        ] {
            if let Err(e) = std::fs::create_dir_all(dir) {
                degradation_registry.register(
                    degradations::ids::STORAGE_DIRECTORY,
                    degradations::Severity::Error,
                    "Storage directory",
                    format!(
                        "The {label} directory {} could not be created: {e}",
                        dir.display()
                    ),
                    "Check the path in Settings and that the server user can write to it.",
                );
            }
        }

        let library_path = std::path::Path::new(&settings.library_path);
        if library_path.exists() {
            if let Err(e) = cleanup_staging_dirs(library_path, &pool).await {
                degradation_registry.register(
                    degradations::ids::LIBRARY_PATH,
                    degradations::Severity::Warn,
                    "Library path",
                    format!("The library directory could not be read at startup: {e}"),
                    "Check that the library path exists and is readable by the server user.",
                );
            }
            tracing::info!("Library cleanup complete");
        }

        let flaresolverr_url = if settings.flaresolverr_url.is_empty() {
            None
        } else {
            Some(settings.flaresolverr_url.clone())
        };

        let global_smart_client = kani_core::http::SmartClient::new(flaresolverr_url.clone())?;
        tracing::info!("Smart client created");

        let cache = RequestCache::new();
        let ext_cache: std::sync::Arc<dyn kani_core::cache::CacheBackend> =
            std::sync::Arc::new(crate::cache::SqliteCache::new(pool.clone()));

        if let Err(e) = Self::scan_and_register_sources(
            &pool,
            &settings.wasm_storage_path,
            global_smart_client.clone(),
            &wasm_runtime,
            &cache.preference_schema,
            ext_cache.as_ref(),
        )
        .await
        {
            degradation_registry.register(
                degradations::ids::SOURCE_REGISTRY,
                degradations::Severity::Error,
                "Source registry",
                format!("Installed extensions could not be scanned or registered: {e}. Sources may be missing."),
                "Check that the WASM storage directory exists and is readable, then restart.",
            );
        }
        tracing::info!("Sources scanned and registered");

        let sources = sqlx::query_as!(
            Source,
            "SELECT id, name, version, base_url, enabled, favourited, unrestricted_http, \
             browser_enabled, download_concurrency, icon, description, languages, schema_version, \
             CAST(NULL AS TEXT) as circuit_state \
             FROM sources WHERE enabled = 1 AND deleted_at IS NULL"
        )
        .fetch_all(&pool)
        .await?;

        let mut live_wasm_hashes: HashSet<String> = HashSet::new();

        for source in sources {
            let prefs = Self::load_pref_map_static(&pool, source.id).await?;
            let ns = format!("{}:", source.name);

            let yaml_path = settings
                .wasm_storage_path
                .join(format!("{}.yaml", source.name));
            let wasm_path = settings
                .wasm_storage_path
                .join(format!("{}.wasm", source.name));

            // An unreadable extension degrades that source without aborting startup.
            if !yaml_path.exists() && !wasm_path.exists() {
                degradation_registry.register(
                    &degradations::ids::source_load(&source.name),
                    degradations::Severity::Error,
                    format!("Source '{}'", source.name),
                    format!(
                        "No extension file at {} or {}, so this source cannot be used.",
                        yaml_path.display(),
                        wasm_path.display()
                    ),
                    "Reinstall the extension, or remove the source if it is no longer wanted.",
                );
                continue;
            }

            let backend = if yaml_path.exists() {
                let text = match tokio::fs::read_to_string(&yaml_path).await {
                    Ok(t) => t,
                    Err(e) => {
                        degradation_registry.register(
                            &degradations::ids::source_load(&source.name),
                            degradations::Severity::Error,
                            format!("Source '{}'", source.name),
                            format!("Cannot read {}: {e}", yaml_path.display()),
                            "Check the file's permissions, then restart.",
                        );
                        continue;
                    }
                };
                match kani_yaml::parse_and_validate(&text, &yaml_path) {
                    Ok(ext) => {
                        if wasm_path.exists() {
                            tracing::info!(
                                "YAML supersedes WASM for source '{}'; WASM retained for rollback",
                                source.name
                            );
                        }
                        loader::build_yaml_source(
                            std::sync::Arc::new(ext),
                            local_network::client_for(&pool, source.id, &global_smart_client).await,
                            std::sync::Arc::clone(&ext_cache),
                            ns,
                            prefs,
                            source.browser_enabled,
                        )
                    }
                    Err(errs) => {
                        let msg = errs
                            .iter()
                            .map(|e| e.to_string())
                            .collect::<Vec<_>>()
                            .join("; ");
                        tracing::error!("Failed to load YAML source '{}': {}", source.name, msg);
                        continue;
                    }
                }
            } else {
                let bytes = match tokio::fs::read(&wasm_path).await {
                    Ok(b) => b,
                    Err(e) => {
                        degradation_registry.register(
                            &degradations::ids::source_load(&source.name),
                            degradations::Severity::Error,
                            format!("Source '{}'", source.name),
                            format!("Cannot read {}: {e}", wasm_path.display()),
                            "Reinstall the extension, or remove the source if it is no longer wanted.",
                        );
                        continue;
                    }
                };
                live_wasm_hashes
                    .insert(kani_core::wasm::cache::WasmModuleCache::sha256_hex(&bytes));
                let compiled = wasm_runtime
                    .compile_component(&bytes)
                    .and_then(|component| {
                        wasm_runtime
                            .instantiate_pre(&component)
                            .map(|pre| (component, pre))
                    });
                let (component, instance_pre) = match compiled {
                    Ok(pair) => pair,
                    Err(e) => {
                        degradation_registry.register(
                            &degradations::ids::source_load(&source.name),
                            degradations::Severity::Error,
                            format!("Source '{}'", source.name),
                            format!("{} is not a loadable extension: {e}", wasm_path.display()),
                            "Reinstall the extension — the file is corrupt or built for another Kani version.",
                        );
                        continue;
                    }
                };

                let mut base_url = source.base_url.clone();
                let mut unrestricted_http = source.unrestricted_http;
                let (pure_registry, hook_registry, max_hook_requests) = {
                    let mut inst = kani_core::sources::SourceInstance::new(
                        global_smart_client.clone(),
                        None,
                        false,
                    );
                    if inst
                        .load(wasm_runtime.engine(), &component, wasm_runtime.linker())
                        .await
                        .is_ok()
                    {
                        let meta = inst.get_metadata().await.ok().and_then(|raw| {
                            serde_json::from_str::<kani_shared::ExtensionMetadata>(&raw).ok()
                        });
                        if let Some(m) = &meta {
                            let loadable = match crate::install_gating::check_dsl_schema_version(
                                m.dsl_schema_version,
                            ) {
                                Ok(()) => sources::reconcile_wasm_row(
                                    &pool,
                                    ext_cache.as_ref(),
                                    &source,
                                    m,
                                )
                                .await
                                .map_err(|e| e.to_string()),
                                Err(reason) => Err(reason),
                            };
                            if let Err(reason) = loadable {
                                degradation_registry.register(
                                    &degradations::ids::source_load(&source.name),
                                    degradations::Severity::Error,
                                    format!("Source '{}'", source.name),
                                    format!("{} cannot be loaded: {reason}", wasm_path.display()),
                                    "Reinstall the extension from its repository, or rebuild it.",
                                );
                                continue;
                            }
                            base_url.clone_from(&m.base_url);
                            unrestricted_http = m.unrestricted_http;
                        }
                        let max_hk = meta
                            .as_ref()
                            .and_then(|m| m.rate_limit.as_ref())
                            .map(|rl| rl.max_hook_requests)
                            .unwrap_or(3);
                        let pure_reg = meta.as_ref().and_then(|m| {
                            if m.scripts.is_empty() {
                                return None;
                            }
                            match kani_core::scripting::PureFunctionRegistry::compile(&m.scripts) {
                                Ok(reg) => Some(Arc::new(reg)),
                                Err(e) => {
                                    tracing::warn!(
                                        source = %source.name,
                                        "Failed to compile pure scripts: {e}"
                                    );
                                    None
                                }
                            }
                        });
                        let hook_reg = meta.as_ref().and_then(sources::compile_hook_registry);
                        (pure_reg, hook_reg, max_hk)
                    } else {
                        (None, None, 3u32)
                    }
                };

                loader::build_wasm_source(
                    wasm_runtime.engine().clone(),
                    instance_pre,
                    local_network::client_for(&pool, source.id, &global_smart_client).await,
                    Some(base_url),
                    unrestricted_http,
                    source.browser_enabled,
                    prefs,
                    std::sync::Arc::clone(&ext_cache),
                    ns,
                    pure_registry,
                    hook_registry,
                    max_hook_requests,
                )
            };

            sources_registry.insert(source.id, backend);
        }
        tracing::info!("Sources loaded");

        // Ids stored before the host kept them opaque decode to nothing, which
        // strips a request of its composite placeholders. Idempotent, so it
        // runs on every boot rather than needing a one-shot flag.
        id_repair::repair_composite_ids(&pool, &sources_registry).await;
        wasm_runtime.prune_module_cache(&live_wasm_hashes);

        let downloader = DownloaderManager::new(
            global_smart_client.clone(),
            DownloaderConfig {
                concurrent_pages: settings.concurrent_page_downloads.try_into()?,
                max_attempts: settings.max_retries,
                initial_retry_delay_ms: settings.initial_retry_delay_ms,
            },
        )
        .await
        .map_err(ServiceError::Core)?;
        tracing::info!("Downloader manager created");

        let proxy_client = kani_core::http::SmartClient::new_proxy(
            flaresolverr_url,
            global_smart_client.credentials.clone(),
            global_smart_client.solving.clone(),
            global_smart_client.host_circuits.clone(),
            global_smart_client.circuit_event_tx.clone(),
        )?;
        tracing::info!("Proxy client created");

        let (refresh_tx, _) =
            tokio::sync::broadcast::channel(crate::tuning::SSE_BROADCAST_CAPACITY);
        let refresh_task = Arc::new(tokio::sync::Mutex::new(None));

        {
            let mut rx = global_smart_client.subscribe_circuit_events();
            let tx = refresh_tx.clone();
            let cancel = shutdown_token.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        msg = rx.recv() => match msg {
                            Ok(ev) => {
                                let _ = tx.send(crate::events::AppEvent::CircuitOpen {
                                    host: ev.host,
                                    failure_count: ev.failure_count,
                                });
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        }
                    }
                }
            });
        }

        let tracker_registry = TrackerRegistry::new(&pool, enc.as_ref()).await?;

        let email_svc = email::EmailService::from_settings(&settings);
        let enc = enc.map(Arc::new);
        let webhook_service = webhooks::WebhookService::new(pool.clone(), read_pool.clone());

        let svc_cell: crate::jobs::framework::ServiceCell = Arc::new(std::sync::Mutex::new(None));

        let mut job_registry = crate::jobs::JobRegistry::new();
        job_registry.register::<crate::jobs::download::ChapterDownloadJob>();
        job_registry.register::<crate::jobs::download::MangaDownloadAllJob>();
        job_registry.register::<crate::jobs::download::SourceScanJob>();
        job_registry.register::<crate::jobs::download::LibraryScanJob>();
        job_registry.register::<crate::jobs::refresh::RefreshMangaJob>();
        job_registry.register::<crate::jobs::maintenance::AnalyzeJob>();
        job_registry.register::<crate::jobs::maintenance::VacuumJob>();
        job_registry.register::<crate::jobs::scan::AutoScanJob>();
        job_registry.register::<crate::jobs::backup::ScheduledBackupJob>();
        job_registry.register::<crate::jobs::storage::StorageMonitorJob>();
        job_registry.register::<crate::jobs::scrub::ScrubJob>();
        job_registry.register::<crate::jobs::archive_export::ArchiveExportJob>();
        job_registry.register::<crate::jobs::audit_prune::AuditPruneJob>();
        job_registry.register::<crate::jobs::trash_purge::TrashPurgeJob>();
        job_registry.register::<crate::jobs::pending_delete_retry::PendingDeleteRetryJob>();
        job_registry.register::<crate::jobs::thumbnail::ThumbnailGenerationJob>();
        job_registry.register::<crate::jobs::import_dedup::ImportDedupJob>();
        job_registry.register::<crate::jobs::import_resolve::ImportResolveJob>();
        job_registry.register::<crate::jobs::webhook_delivery::WebhookDeliveryJob>();
        job_registry.register::<crate::jobs::tracker_sync::TrackerSyncJob>();
        job_registry.register::<crate::jobs::v8_reap::V8ReapJob>();
        job_registry.register::<crate::jobs::update_check::UpdateCheckJob>();
        job_registry.register::<crate::jobs::manifest_backfill::ManifestBackfillJob>();
        job_registry.register::<crate::jobs::migration::MigrationJob>();

        let job_manager = crate::jobs::JobManager::new(
            pool.clone(),
            refresh_tx.clone(),
            shutdown_token.clone(),
            crate::jobs::JobManagerConfig {
                global_max_concurrent: settings
                    .max_concurrent_jobs
                    .try_into()
                    .unwrap_or(crate::tuning::DEFAULT_MAX_CONCURRENT_JOBS),
                job_shutdown_timeout: std::time::Duration::from_secs(
                    settings
                        .job_shutdown_timeout_secs
                        .try_into()
                        .unwrap_or(crate::tuning::DEFAULT_JOB_SHUTDOWN_TIMEOUT_SECS),
                ),
                type_configs: std::collections::HashMap::new(),
                registry: job_registry,
                max_history: settings
                    .job_max_history
                    .try_into()
                    .unwrap_or(crate::tuning::DEFAULT_JOB_MAX_HISTORY),
                concurrency: crate::jobs::ConcurrencyConfig {
                    page_concurrency: settings.concurrent_page_downloads.try_into()?,
                    per_source_download_concurrency: settings
                        .per_source_download_concurrency
                        .try_into()?,
                    scan_concurrency: settings.scan_concurrency.try_into()?,
                },
                svc_cell: Arc::clone(&svc_cell),
            },
        )
        .await?;
        tracing::info!("Job manager created");

        let svc = Self {
            db: pool,
            db_path,
            db_read: read_pool,
            wasm_runtime,
            sources: Arc::new(sources_registry),
            settings: Arc::new(tokio::sync::RwLock::new(settings)),
            downloader,
            smart_client: global_smart_client,
            proxy_client,
            source_clients: Arc::default(),
            refresh_tx,
            refresh_task,
            cache,
            ext_cache,
            shutdown_token,
            tracker_registry: Arc::new(tokio::sync::RwLock::new(tracker_registry)),
            cover_retry_queue: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            email_service: Arc::new(tokio::sync::RwLock::new(email_svc)),
            encryption: enc,
            degradations: Arc::clone(&degradation_registry),
            webhook_service,
            metadata_provider_registry: Arc::new(tokio::sync::RwLock::new(
                metadata_provider::MetadataProviderRegistry::new(),
            )),
            job_manager,
            latest_version: Arc::new(tokio::sync::RwLock::new(None)),
            install_locks: Arc::new(DashMap::new()),
            progress_buffer: progress::ReadProgressBuffer::default(),
            undo_tokens: Arc::new(DashMap::new()),
            #[cfg(any(test, feature = "test-util"))]
            mock_sources: Arc::new(DashMap::new()),
        };

        *svc_cell.lock().expect("svc_cell lock") = Some(svc.clone());

        // Encrypt any plaintext credentials left over from pre-encryption installs.
        if let Err(e) = svc.migrate_credentials_to_encrypted().await {
            degradation_registry.register(
                degradations::ids::CREDENTIAL_MIGRATION,
                degradations::Severity::Error,
                "Credential encryption migration",
                format!("Stored credentials could not be migrated to the current key: {e}"),
                "Check the server log for the failing row, then restart. Until this \
                 succeeds, some credentials remain in their previous form.",
            );
        } else if svc.encryption.is_some() {
            tracing::info!("Credential encryption migration complete");
        }

        degradations::log_startup_summary(&degradation_registry);

        Ok(svc)
    }

    #[cfg(any(test, feature = "test-util"))]
    pub async fn new_for_test(pool: SqlitePool) -> Self {
        use crate::models::Settings;

        let test_dir = tempfile::Builder::new()
            .prefix("kani-test-")
            .tempdir()
            .expect("tempdir")
            .keep();
        let settings = Settings {
            flaresolverr_url: String::new(),
            library_path: test_dir.clone(),
            wasm_storage_path: test_dir,
            concurrent_page_downloads: 4,
            max_retries: 3,
            initial_retry_delay_ms: 100,
            max_wasm_instances: 1,
            auto_scan: false,
            scan_interval_minutes: 60,
            scan_exclude_completed: false,
            auto_download_category_ids: "[]".to_string(),
            default_tracking_enabled: false,
            http_request_logging: false,
            v8_debug_logging: false,
            registration_enabled: true,
            cover_max_dimension: None,
            email_enabled: false,
            email_provider: String::new(),
            email_provider_config: String::new(),
            email_from_address: String::new(),
            app_url: String::new(),
            password_reset_enabled: false,
            email_verification_required: false,
            first_run_complete: false,
            scan_concurrency: crate::tuning::DEFAULT_SCAN_CONCURRENCY as i64,
            per_source_download_concurrency: crate::tuning::DEFAULT_PER_SOURCE_DOWNLOAD_CONCURRENCY
                as i64,
            job_max_history: crate::tuning::DEFAULT_JOB_MAX_HISTORY as i64,
            job_shutdown_timeout_secs: crate::tuning::DEFAULT_JOB_SHUTDOWN_TIMEOUT_SECS as i64,
            trash_retention_days: 30,
            audit_retention_days: 365,
            audit_security_retention_days: 0,
            disk_warn_threshold: 0.10,
            thumbnail_formats: "jpeg".to_string(),
            max_login_attempts: 5,
            max_ip_attempts: 20,
            login_lockout_seconds: 900,
            session_timeout_secs: 2592000,
            tracker_auto_sync_enabled: false,
            tracker_sync_interval_hours: 24,
            max_concurrent_jobs: crate::tuning::DEFAULT_MAX_CONCURRENT_JOBS as i64,
            db_maintenance_interval_hours: 24,
            db_vacuum_interval_hours: 168,
            audit_prune_interval_hours: 168,
            trash_purge_interval_hours: 168,
            integrity_quick_scrub_interval_hours: 24,
            integrity_deep_scrub_interval_hours: 168,
            scrub_on_startup: false,
            integrity_revalidate_after_days: 30,
            upgrade_detection_enabled: true,
            upgrade_min_res_gain: 1.2,
            upgrade_confirm_fetches: 3,
            upgrade_axis_resolution: "both".into(),
            upgrade_axis_colour: "both".into(),
            upgrade_axis_encoder: "both".into(),
            upgrade_axis_bitrate: "gain".into(),
            upgrade_show_downgrades: false,
            upgrade_auto_replace_reasons: "preferred_scanlator,resolution,colour".into(),
            v8_max_memory_mb: 512,
            v8_idle_timeout_s: 300,
            update_check_enabled: true,
            opds_page_index_zero_based: false,
            scan_barren_page_tolerance: 3,
            global_search_timeout_secs: 6,
        };

        let smart_client = kani_core::http::SmartClient::new(None)
            .expect("SmartClient::new failed in test")
            .with_allow_loopback_egress(true);
        let proxy_client = kani_core::http::SmartClient::new(None)
            .expect("proxy SmartClient::new failed in test")
            .with_allow_loopback_egress(true);
        let wasm_runtime =
            Arc::new(WasmRuntime::new_on_demand().expect("WasmRuntime::new failed in test"));
        let downloader = DownloaderManager::new(
            smart_client.clone(),
            DownloaderConfig {
                concurrent_pages: 1,
                max_attempts: 0,
                initial_retry_delay_ms: 0,
            },
        )
        .await
        .expect("DownloaderManager::new failed in test");
        let tracker_registry = TrackerRegistry::new(&pool, None)
            .await
            .expect("TrackerRegistry::new failed in test");
        // Small capacity: tests have at most a couple of SSE subscribers.
        let (refresh_tx, _) = tokio::sync::broadcast::channel(16);
        let shutdown_token = tokio_util::sync::CancellationToken::new();

        let svc_cell: crate::jobs::framework::ServiceCell = Arc::new(std::sync::Mutex::new(None));

        let mut registry = crate::jobs::JobRegistry::new();
        registry.register::<crate::jobs::test_jobs::TestJob>();
        registry.register::<crate::jobs::test_jobs::SlowTestJob>();
        registry.register::<crate::jobs::test_jobs::FailingDownloadJob>();
        registry.register::<crate::jobs::download::ChapterDownloadJob>();
        registry.register::<crate::jobs::download::MangaDownloadAllJob>();
        registry.register::<crate::jobs::download::SourceScanJob>();
        registry.register::<crate::jobs::download::LibraryScanJob>();
        registry.register::<crate::jobs::refresh::RefreshMangaJob>();
        registry.register::<crate::jobs::maintenance::AnalyzeJob>();
        registry.register::<crate::jobs::maintenance::VacuumJob>();
        registry.register::<crate::jobs::scan::AutoScanJob>();
        registry.register::<crate::jobs::backup::ScheduledBackupJob>();
        registry.register::<crate::jobs::storage::StorageMonitorJob>();
        registry.register::<crate::jobs::scrub::ScrubJob>();
        registry.register::<crate::jobs::archive_export::ArchiveExportJob>();
        registry.register::<crate::jobs::v8_reap::V8ReapJob>();
        registry.register::<crate::jobs::update_check::UpdateCheckJob>();
        registry.register::<crate::jobs::manifest_backfill::ManifestBackfillJob>();

        let job_manager = crate::jobs::JobManager::new(
            pool.clone(),
            refresh_tx.clone(),
            shutdown_token.clone(),
            crate::jobs::JobManagerConfig {
                global_max_concurrent: settings
                    .max_concurrent_jobs
                    .try_into()
                    .unwrap_or(crate::tuning::DEFAULT_MAX_CONCURRENT_JOBS),
                job_shutdown_timeout: std::time::Duration::from_secs(5),
                type_configs: std::collections::HashMap::new(),
                registry,
                max_history: crate::tuning::DEFAULT_JOB_MAX_HISTORY,
                concurrency: crate::jobs::ConcurrencyConfig {
                    page_concurrency: 1,
                    per_source_download_concurrency: 1,
                    scan_concurrency: 1,
                },
                svc_cell: Arc::clone(&svc_cell),
            },
        )
        .await
        .expect("JobManager::new failed in test");

        let svc = Self {
            db: pool.clone(),
            db_path: std::path::PathBuf::from("kani.db"),
            db_read: pool.clone(),
            wasm_runtime,
            sources: Arc::new(SourceRegistry::new()),
            settings: Arc::new(tokio::sync::RwLock::new(settings)),
            downloader,
            smart_client,
            proxy_client,
            source_clients: Arc::default(),
            refresh_tx,
            refresh_task: Arc::new(tokio::sync::Mutex::new(None)),
            cache: RequestCache::new(),
            ext_cache: std::sync::Arc::new(kani_core::cache::InMemoryCache::new()),
            shutdown_token,
            tracker_registry: Arc::new(tokio::sync::RwLock::new(tracker_registry)),
            cover_retry_queue: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            email_service: Arc::new(tokio::sync::RwLock::new(None)),
            encryption: None,
            degradations: Arc::new(degradations::DegradationRegistry::new()),
            webhook_service: webhooks::WebhookService::new(pool.clone(), pool),
            metadata_provider_registry: Arc::new(tokio::sync::RwLock::new(
                metadata_provider::MetadataProviderRegistry::new(),
            )),
            job_manager,
            latest_version: Arc::new(tokio::sync::RwLock::new(None)),
            install_locks: Arc::new(DashMap::new()),
            progress_buffer: progress::ReadProgressBuffer::default(),
            undo_tokens: Arc::new(DashMap::new()),
            mock_sources: Arc::new(DashMap::new()),
        };
        *svc_cell.lock().expect("svc_cell lock") = Some(svc.clone());
        svc
    }

    #[cfg(any(test, feature = "test-util"))]
    pub fn register_mock_source(
        &self,
        source_id: i64,
        fetcher: Arc<dyn kani_core::downloader::PageListFetcher>,
    ) {
        self.mock_sources.insert(source_id, fetcher);
    }

    #[cfg(any(test, feature = "test-util"))]
    pub async fn scan_and_load_yaml_dir_for_test(
        &self,
        wasm_dir: &std::path::Path,
    ) -> crate::error::Result<()> {
        let pref_schemas: DashMap<i64, Vec<kani_core::PreferenceSpec>> = DashMap::new();
        Self::scan_and_register_sources(
            &self.db,
            wasm_dir,
            self.smart_client.clone(),
            &self.wasm_runtime,
            &pref_schemas,
            self.ext_cache.as_ref(),
        )
        .await?;

        use sqlx::Row as _;
        let rows =
            sqlx::query("SELECT id, name FROM sources WHERE enabled = 1 AND deleted_at IS NULL")
                .fetch_all(&self.db_read)
                .await?;
        let sources: Vec<(i64, String)> = rows
            .into_iter()
            .filter_map(|r| {
                let id: i64 = r.try_get("id").ok()?;
                let name: String = r.try_get("name").ok()?;
                Some((id, name))
            })
            .collect();

        for (source_id, source_name) in sources {
            let yaml_path = wasm_dir.join(format!("{source_name}.yaml"));
            if !yaml_path.exists() {
                continue;
            }
            let text = tokio::fs::read_to_string(&yaml_path)
                .await
                .map_err(|e| crate::error::ServiceError::Core(kani_core::Error::Io(e)))?;
            match kani_yaml::parse_and_validate(&text, &yaml_path) {
                Ok(ext) => {
                    let prefs = Self::load_pref_map_static(&self.db, source_id).await?;
                    let backend = crate::source::loader::build_yaml_source(
                        std::sync::Arc::new(ext),
                        local_network::client_for(&self.db, source_id, &self.smart_client).await,
                        std::sync::Arc::clone(&self.ext_cache),
                        format!("{source_name}:"),
                        prefs,
                        true,
                    );
                    self.sources.insert(source_id, backend);
                }
                Err(errs) => {
                    let msg = errs
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>()
                        .join("; ");
                    tracing::warn!("YAML source '{source_name}' skipped in test: {msg}");
                }
            }
        }
        Ok(())
    }

    #[cfg(any(test, feature = "test-util"))]
    pub async fn load_yaml_sources_from_dir_for_test(
        &self,
        wasm_dir: &std::path::Path,
    ) -> crate::error::Result<()> {
        use sqlx::Row as _;
        let rows =
            sqlx::query("SELECT id, name FROM sources WHERE enabled = 1 AND deleted_at IS NULL")
                .fetch_all(&self.db_read)
                .await?;

        for row in rows {
            let source_id: i64 = row.try_get("id").unwrap_or_default();
            let source_name: String = row.try_get("name").unwrap_or_default();
            let yaml_path = wasm_dir.join(format!("{source_name}.yaml"));
            if !yaml_path.exists() {
                continue;
            }
            let text = tokio::fs::read_to_string(&yaml_path)
                .await
                .map_err(|e| crate::error::ServiceError::Core(kani_core::Error::Io(e)))?;
            match kani_yaml::parse_and_validate(&text, &yaml_path) {
                Ok(ext) => {
                    let prefs = Self::load_pref_map_static(&self.db, source_id).await?;
                    let backend = crate::source::loader::build_yaml_source(
                        std::sync::Arc::new(ext),
                        local_network::client_for(&self.db, source_id, &self.smart_client).await,
                        std::sync::Arc::clone(&self.ext_cache),
                        format!("{source_name}:"),
                        prefs,
                        true,
                    );
                    self.sources.insert(source_id, backend);
                }
                Err(errs) => {
                    let msg = errs
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>()
                        .join("; ");
                    tracing::warn!("YAML source '{source_name}' skipped in test: {msg}");
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn rebuild_email_service(&self) {
        let settings = self.settings.read().await;
        let svc = email::EmailService::from_settings(&settings);
        *self.email_service.write().await = svc;
    }

    /// Returns a clone of the email service if email is enabled and configured.
    pub(crate) async fn mailer(&self) -> Option<email::EmailService> {
        self.email_service.read().await.clone()
    }

    /// Spawns a background task to send an email. Logs errors but never fails the caller.
    pub(crate) fn send_email_bg(&self, to: String, subject: String, html: String) {
        let svc = self.email_service.clone();
        tokio::spawn(async move {
            let guard = svc.read().await;
            if let Some(mailer) = guard.as_ref()
                && let Err(e) = mailer.send(&to, &subject, &html).await
            {
                tracing::warn!("Email send failed to {to}: {e}");
            }
        });
    }

    /// Subscribes to the broadcast channel and dispatches webhook events. Call once from main.
    pub fn spawn_webhook_listener(&self) {
        let mut rx = self.refresh_tx.subscribe();
        let svc = self.clone();
        let token = self.shutdown_token.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    result = rx.recv() => match result {
                        Ok(event) => {
                            match event {
                                AppEvent::NewChapters {
                                    manga_id,
                                    manga_name,
                                    count,
                                    chapter_ids,
                                    chapter_names,
                                } => {
                                    svc.fire_webhooks(webhooks::WebhookPayload::ChapterNew {
                                        manga_id,
                                        manga_name,
                                        chapter_count: count,
                                        chapter_ids,
                                        chapter_names,
                                    })
                                    .await;
                                }
                                AppEvent::Refresh(RefreshProgressEvent::Completed { total, failed }) => {
                                    svc.fire_webhooks(webhooks::WebhookPayload::ScanCompleted {
                                        total_scanned: total,
                                        failed_count: failed,
                                    })
                                    .await;
                                }
                                _ => {}
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("Webhook listener lagged by {n} events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    },
                }
            }
        });
    }

    pub async fn run_auto_scan_once(&self) {
        let settings_snap = self.settings.read().await.clone();
        if !settings_snap.auto_scan {
            return;
        }
        let exclude_completed = settings_snap.scan_exclude_completed;
        let category_ids: Vec<i64> =
            serde_json::from_str(&settings_snap.auto_download_category_ids).unwrap_or_default();
        drop(settings_snap);

        let category_manga_ids: std::collections::HashSet<i64> = if category_ids.is_empty() {
            std::collections::HashSet::new()
        } else {
            let placeholders = category_ids
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 1))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT DISTINCT manga_id FROM manga_categories WHERE category_id IN ({})",
                placeholders
            );
            let mut q = sqlx::query_scalar::<_, i64>(&sql);
            for id in &category_ids {
                q = q.bind(*id);
            }
            q.fetch_all(&self.db)
                .await
                .unwrap_or_default()
                .into_iter()
                .collect()
        };

        let manga_to_scan: Vec<(i64, bool)> = {
            let base = "SELECT m.id, m.auto_download FROM manga m WHERE m.auto_scan = true AND m.deleted_at IS NULL AND m.is_orphaned = FALSE";
            let completed_clause = if exclude_completed {
                " AND m.status != 1"
            } else {
                ""
            };
            let sql = format!("{base}{completed_clause}");
            sqlx::query_as::<_, (i64, bool)>(&sql)
                .fetch_all(&self.db)
                .await
                .unwrap_or_default()
        };

        for (manga_db_id, auto_download) in manga_to_scan {
            let effective_auto_download =
                auto_download || category_manga_ids.contains(&manga_db_id);
            match self
                .scan_for_new_chapters(crate::ids::MangaId(manga_db_id))
                .await
            {
                Ok(new_ids) if !new_ids.is_empty() => {
                    tracing::info!(
                        "Found {} new chapters for manga {}",
                        new_ids.len(),
                        manga_db_id
                    );
                    if effective_auto_download {
                        let filtered_ids = self
                            .filter_chapters_by_rules(
                                crate::ids::MangaId(manga_db_id),
                                new_ids.iter().copied().map(crate::ids::ChapterId).collect(),
                            )
                            .await;

                        if filtered_ids.is_empty() {
                            tracing::info!(
                                "All new chapters for manga {} filtered out by download rules",
                                manga_db_id
                            );
                            let suppressed = new_ids.len() as i64;
                            if let Err(e) = sqlx::query!(
                                "UPDATE manga SET suppressed_chapter_count = ? WHERE id = ?",
                                suppressed,
                                manga_db_id
                            )
                            .execute(&self.db)
                            .await
                            {
                                tracing::warn!(
                                    "Failed to record suppressed-chapter count for manga {}: {}",
                                    manga_db_id,
                                    e
                                );
                            }
                        } else {
                            tracing::info!(
                                "{} new chapters passed download rules for manga {}",
                                filtered_ids.len(),
                                manga_db_id
                            );
                            if let Err(e) = sqlx::query!(
                                "UPDATE manga SET suppressed_chapter_count = 0 WHERE id = ?",
                                manga_db_id
                            )
                            .execute(&self.db)
                            .await
                            {
                                tracing::warn!(
                                    "Failed to clear suppressed-chapter count for manga {}: {}",
                                    manga_db_id,
                                    e
                                );
                            }
                            let mut join_set = tokio::task::JoinSet::new();
                            for new_id in filtered_ids {
                                let s = self.clone();
                                join_set.spawn(async move {
                                    match s.enqueue_claimed_chapter(new_id).await {
                                        Ok(_) => tracing::info!(
                                            "Chapter {} enqueued for download",
                                            new_id
                                        ),
                                        Err(e) => tracing::error!(
                                            "Failed to enqueue chapter {}: {}",
                                            new_id,
                                            e
                                        ),
                                    }
                                });
                            }
                            while let Some(result) = join_set.join_next().await {
                                if let Err(e) = result {
                                    tracing::error!("Chapter enqueue task panicked: {e}");
                                }
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(ServiceError::NotFound(_)) => {
                    tracing::debug!(
                        "Manga {} deleted or source gone during scan, skipping",
                        manga_db_id
                    );
                }
                Err(e) => tracing::error!("Scan failed for manga {}: {}", manga_db_id, e),
            }
        }

        if let Err(e) =
            sqlx::query!("DELETE FROM chapters WHERE is_orphaned = true AND download_status != 2")
                .execute(&self.db)
                .await
        {
            tracing::warn!("Orphan chapter cleanup failed: {}", e);
        }

        self.retry_missing_covers().await;
    }

    /// Schedules a cover download retry for the given manga ID.
    /// Called from library operations when a cover download fails.
    pub(crate) async fn schedule_cover_retry(&self, manga_id: MangaId) {
        self.cover_retry_queue.lock().await.insert(manga_id);
    }

    /// Spawns a background task that retries failed cover downloads every 30 seconds.
    /// Only retries manga IDs that have been enqueued via `schedule_cover_retry`.
    pub fn spawn_cover_retry(&self) {
        let state = self.clone();
        let token = self.shutdown_token.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        tracing::info!("Cover retry task shutting down");
                        break;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_secs(
                        crate::tuning::COVER_RETRY_INTERVAL_SECS,
                    )) => {}
                }

                let ids: Vec<MangaId> = state.cover_retry_queue.lock().await.drain().collect();
                if ids.is_empty() {
                    continue;
                }

                tracing::info!("Retrying cover downloads for {} manga", ids.len());
                for manga_id in ids {
                    match state.retry_single_cover(manga_id).await {
                        Ok(()) => tracing::info!("Cover retry succeeded for manga {manga_id}"),
                        Err(e) => {
                            tracing::debug!("Cover retry failed for manga {manga_id}: {e}");
                            state.cover_retry_queue.lock().await.insert(manga_id);
                        }
                    }
                }
            }
        });
    }

    pub fn spawn_cache_prune(&self) {
        let cache = std::sync::Arc::clone(&self.ext_cache);
        let token = self.shutdown_token.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(600));
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = interval.tick() => {}
                }
                cache.prune_expired().await;
            }
        });
    }

    /// Spawns a background task that proactively refreshes Cloudflare credentials before they expire.
    pub fn spawn_credential_refresh(&self) {
        let client = self.smart_client.clone();
        let token = self.shutdown_token.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                crate::tuning::CREDENTIAL_REFRESH_INTERVAL_SECS,
            ));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        tracing::info!("Credential refresh task shutting down");
                        break;
                    }
                    _ = interval.tick() => {
                        client.refresh_expiring_credentials().await;
                    }
                }
            }
        });
    }

    /// Record an auditable event. Non-fatal — a DB failure only produces a warning.
    pub async fn audit(
        &self,
        user_id: Option<crate::ids::UserId>,
        action: &str,
        target: Option<&str>,
        details: Option<serde_json::Value>,
    ) {
        let details_str = details.map(|d| d.to_string());
        if let Err(e) = sqlx::query!(
            "INSERT INTO audit_log (user_id, action, target, details) VALUES (?, ?, ?, ?)",
            user_id,
            action,
            target,
            details_str,
        )
        .execute(&self.db)
        .await
        {
            tracing::warn!("Audit log insert failed: {e}");
        }
    }

    pub fn subscribe_refresh(&self) -> tokio::sync::broadcast::Receiver<AppEvent> {
        self.refresh_tx.subscribe()
    }

    pub fn invalidate_library(&self) {
        self.cache.invalidate_library();
        let _ = self.refresh_tx.send(AppEvent::LibraryInvalidated);
    }
}

pub fn chapter_name(volume: Option<i64>, chapter_number: f64, title: Option<String>) -> String {
    let mut name = String::new();
    if let Some(vol) = volume {
        name.push_str(&format!("Vol. {vol} "));
    }
    if chapter_number.fract().abs() < f64::EPSILON {
        name.push_str(&format!("Ch. {}", chapter_number as i64));
    } else {
        name.push_str(&format!("Ch. {chapter_number:.1}"));
    }
    if let Some(title) = title
        && !title.is_empty()
    {
        name.push_str(&format!(" - {title}"));
    }
    name
}

pub(crate) async fn cleanup_staging_dirs(
    library_path: &std::path::Path,
    pool: &sqlx::sqlite::SqlitePool,
) -> std::io::Result<()> {
    let mut manga_dirs = tokio::fs::read_dir(library_path).await?;

    while let Ok(Some(manga_dir)) = manga_dirs.next_entry().await {
        if !manga_dir.file_type().await?.is_dir() {
            continue;
        }

        let Ok(mut inner_entries) = tokio::fs::read_dir(manga_dir.path()).await else {
            continue;
        };

        while let Ok(Some(entry)) = inner_entries.next_entry().await {
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };

            if !entry.file_type().await?.is_dir() || !name.starts_with(".tmp_staging_") {
                continue;
            }

            let suffix = &name[".tmp_staging_".len()..];
            if let Ok(chapter_id) = suffix.parse::<i64>() {
                let resume_offset: Option<i64> = sqlx::query_scalar!(
                    "SELECT resume_offset FROM chapters WHERE id = ?",
                    chapter_id
                )
                .fetch_optional(pool)
                .await
                .unwrap_or(None);

                if resume_offset.is_some_and(|o| o > 0) {
                    tracing::debug!(
                        "Keeping staging dir for chapter {} (resume_offset={})",
                        chapter_id,
                        resume_offset.unwrap_or(0)
                    );
                    continue;
                }
            }

            tracing::info!("Removing orphaned staging directory: {:?}", entry.path());
            if let Err(e) = tokio::fs::remove_dir_all(entry.path()).await {
                tracing::warn!("Failed to remove {:?}: {}", entry.path(), e);
            }
        }
    }

    Ok(())
}

pub(crate) fn unwrap_cache_err(e: Arc<ServiceError>) -> ServiceError {
    match Arc::try_unwrap(e) {
        Ok(err) => err,
        Err(arc) => ServiceError::Internal(arc.to_string()),
    }
}

fn convert_to_shared_manga_info(
    info: kani_core::wasm::kani::extension::types::MangaInfo,
) -> wit_types::MangaInfo {
    use kani_core::wasm::kani::extension::types::MangaStatus as CoreMangaStatus;
    use kani_shared::MangaStatus as SharedMangaStatus;

    let status = match info.status {
        CoreMangaStatus::Ongoing => SharedMangaStatus::Ongoing,
        CoreMangaStatus::Completed => SharedMangaStatus::Completed,
        CoreMangaStatus::Hiatus => SharedMangaStatus::Hiatus,
        CoreMangaStatus::Cancelled => SharedMangaStatus::Cancelled,
        CoreMangaStatus::Unknown => SharedMangaStatus::Unknown,
    };

    wit_types::MangaInfo {
        id: info.id,
        title: info.title,
        cover_url: info.cover_url,
        description: info.description,
        authors: info.authors,
        artists: info.artists,
        status,
        tags: info.tags,
    }
}

fn match_chapters_inner(
    existing: &[(i64, f64)],
    target: &[wit_types::ChapterInfo],
) -> (Vec<(i64, String)>, Vec<i64>, Vec<wit_types::ChapterInfo>) {
    use std::collections::HashMap;

    let mut by_number: HashMap<OrderedFloat<f64>, Vec<i64>> = HashMap::new();
    for &(id, num) in existing {
        by_number.entry(OrderedFloat(num)).or_default().push(id);
    }

    let mut matched = Vec::new();
    let mut unmatched_new = Vec::new();

    for ch in target {
        match by_number
            .get_mut(&OrderedFloat(ch.number))
            .and_then(|b| b.pop())
        {
            Some(existing_id) => matched.push((existing_id, ch.id.clone())),
            None => unmatched_new.push(ch.clone()),
        }
    }

    let orphaned_ids: Vec<i64> = by_number.into_values().flatten().collect();
    (matched, orphaned_ids, unmatched_new)
}
