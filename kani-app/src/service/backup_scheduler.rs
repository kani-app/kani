use crate::error::{Result, ServiceError};
use crate::ids::UserId;
use crate::service::AppService;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackupFrequency {
    Daily { hour: u8 },
    Weekly { weekday: u8, hour: u8 },
}

/// Default destination for scheduled backups, relative like `library_path` and
/// `wasm_storage_path`: the data volume under Docker, the working directory
/// otherwise.
pub const DEFAULT_BACKUP_DIR: &str = "./backups";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackupDestination {
    Local { path: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupScheduleConfig {
    pub enabled: bool,
    pub frequency: BackupFrequency,
    pub retain_n: u32,
    pub destination: BackupDestination,
    pub passphrase: Option<String>,
}

impl Default for BackupScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            frequency: BackupFrequency::Daily { hour: 2 },
            retain_n: 7,
            destination: BackupDestination::Local {
                path: PathBuf::from(DEFAULT_BACKUP_DIR),
            },
            passphrase: None,
        }
    }
}

async fn prune_old_backups(dir: &PathBuf, retain_n: u32) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    let mut files: Vec<(String, std::time::SystemTime)> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("kani-backup-") || !name.ends_with(".zip") {
            continue;
        }
        if let Ok(meta) = entry.metadata().await {
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            files.push((entry.path().to_string_lossy().into_owned(), mtime));
        }
    }
    files.sort_unstable_by_key(|b| std::cmp::Reverse(b.1));
    for (path, _) in files.into_iter().skip(retain_n as usize) {
        if let Err(e) = tokio::fs::remove_file(&path).await {
            tracing::warn!(path = %path, "Failed to prune old backup: {e}");
        }
    }
}

impl AppService {
    pub async fn get_backup_schedule(&self) -> Result<BackupScheduleConfig> {
        let row: Option<String> =
            sqlx::query_scalar("SELECT backup_schedule_json FROM settings WHERE id = 'singleton'")
                .fetch_one(&self.db_read)
                .await?;

        match row {
            None => Ok(BackupScheduleConfig::default()),
            Some(json) => {
                let mut config: BackupScheduleConfig = serde_json::from_str(&json)
                    .map_err(|e| ServiceError::Internal(e.to_string()))?;
                if let Some(ref encrypted) = config.passphrase {
                    config.passphrase = Some(
                        crate::service::encryption::maybe_decrypt(
                            self.encryption.as_deref(),
                            encrypted,
                        )
                        .map_err(|e| ServiceError::Internal(format!("Passphrase decrypt: {e}")))?,
                    );
                }
                Ok(config)
            }
        }
    }

    pub async fn set_backup_schedule(&self, config: &BackupScheduleConfig) -> Result<()> {
        let mut config = config.clone();
        if let Some(ref pass) = config.passphrase {
            config.passphrase = Some(crate::service::encryption::maybe_encrypt(
                self.encryption.as_deref(),
                pass,
            ));
        }
        let json =
            serde_json::to_string(&config).map_err(|e| ServiceError::Internal(e.to_string()))?;
        sqlx::query("UPDATE settings SET backup_schedule_json = ? WHERE id = 'singleton'")
            .bind(json)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub(crate) async fn run_scheduled_backup(&self) -> Result<()> {
        let config = self.get_backup_schedule().await?;
        if !config.enabled {
            return Ok(());
        }

        let BackupDestination::Local { path: dest_dir } = &config.destination;
        tokio::fs::create_dir_all(dest_dir)
            .await
            .map_err(|e| ServiceError::Internal(format!("Cannot create backup dir: {e}")))?;

        let bytes = self
            .export_backup(UserId(0), false, config.passphrase.clone())
            .await?;

        let now = time::OffsetDateTime::now_utc();
        let filename = format!(
            "kani-backup-{}-{:02}-{:02}T{:02}{:02}{:02}.zip",
            now.year(),
            now.month() as u8,
            now.day(),
            now.hour(),
            now.minute(),
            now.second(),
        );
        let dest_path = dest_dir.join(&filename);
        tokio::fs::write(&dest_path, &bytes)
            .await
            .map_err(|e| ServiceError::Internal(format!("Failed to write backup: {e}")))?;

        tracing::info!(path = %dest_path.display(), "Scheduled backup written");

        prune_old_backups(dest_dir, config.retain_n).await;
        Ok(())
    }
}
