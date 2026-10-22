use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, ServiceError};
use crate::events::AppEvent;
use crate::ids::UserId;
use crate::service::AppService;

#[derive(Debug, Serialize, Deserialize)]
pub struct MigrationEstimate {
    pub current_bytes: u64,
    pub available_bytes: u64,
    pub can_migrate: bool,
    pub reason: Option<String>,
}

/// Validates whether the migration is feasible without copying anything.
pub async fn estimate_path_migration(current: &Path, new: &Path) -> Result<MigrationEstimate> {
    if path_has_null(current) || path_has_null(new) {
        return Err(ServiceError::Validation("path contains null byte".into()));
    }

    let canonical_current = dunce::canonicalize(current).map_err(|_| {
        ServiceError::Validation("source path does not exist or is inaccessible".into())
    })?;

    let new_parent = new.parent().unwrap_or(new);
    let canonical_new_parent = dunce::canonicalize(new_parent).map_err(|_| {
        ServiceError::Validation("destination parent directory does not exist".into())
    })?;

    let new_last = new.file_name().unwrap_or_default();
    let canonical_new = canonical_new_parent.join(new_last);

    if canonical_new.starts_with(&canonical_current) {
        return Ok(MigrationEstimate {
            current_bytes: 0,
            available_bytes: 0,
            can_migrate: false,
            reason: Some("destination is inside source directory".into()),
        });
    }

    let current_bytes = dir_size(&canonical_current).await?;

    let avail_path = canonical_new_parent.clone();
    let available_bytes = tokio::task::spawn_blocking(move || fs2::available_space(&avail_path))
        .await
        .map_err(|e| ServiceError::Internal(e.to_string()))?
        .map_err(|_| ServiceError::Internal("could not determine available disk space".into()))?;

    const SAFETY_MARGIN: u64 = 100 * 1024 * 1024;
    if available_bytes < current_bytes.saturating_add(SAFETY_MARGIN) {
        return Ok(MigrationEstimate {
            current_bytes,
            available_bytes,
            can_migrate: false,
            reason: Some(format!(
                "insufficient disk space: need {}, available {}",
                fmt_bytes(current_bytes + SAFETY_MARGIN),
                fmt_bytes(available_bytes),
            )),
        });
    }

    Ok(MigrationEstimate {
        current_bytes,
        available_bytes,
        can_migrate: true,
        reason: None,
    })
}

/// Spawns the migration in the background. Returns immediately.
/// Progress is reported via `service.refresh_tx` SSE events.
pub fn spawn_path_migration(
    service: AppService,
    field: String,
    current: PathBuf,
    new: PathBuf,
    user_id: UserId,
) {
    tokio::spawn(async move {
        if let Err(e) = run_migration(&service, &field, &current, &new, user_id).await {
            tracing::error!("Path migration for {field} failed: {e}");
            let _ = service.refresh_tx.send(AppEvent::PathMigrationFailed {
                field,
                error: normalize_io_error(&e),
            });
        }
    });
}

async fn run_migration(
    service: &AppService,
    field: &str,
    current: &Path,
    new: &Path,
    user_id: UserId,
) -> Result<()> {
    let total_bytes = dir_size(current).await.unwrap_or(0);
    let _ = service.refresh_tx.send(AppEvent::PathMigrationStarted {
        field: field.to_owned(),
        total_bytes,
    });

    if let Err(e) = copy_tree(service, field, current, new, total_bytes).await {
        if let Err(cleanup_err) = tokio::fs::remove_dir_all(new).await {
            tracing::warn!("Rollback cleanup failed for {new:?}: {cleanup_err}");
        }
        return Err(e);
    }

    let new_str = new.to_string_lossy();
    match field {
        "library_path" => {
            sqlx::query!(
                "UPDATE settings SET library_path = ? WHERE id = 'singleton'",
                new_str
            )
            .execute(&service.db)
            .await?;
            service.settings.write().await.library_path = new.to_path_buf();
        }
        "wasm_storage_path" => {
            sqlx::query!(
                "UPDATE settings SET wasm_storage_path = ? WHERE id = 'singleton'",
                new_str
            )
            .execute(&service.db)
            .await?;
            service.settings.write().await.wasm_storage_path = new.to_path_buf();
            tracing::info!(
                "WASM storage path updated to {new:?}; loaded sources unchanged in memory"
            );
        }
        other => {
            return Err(ServiceError::Validation(format!(
                "unknown path field: {other}"
            )));
        }
    }

    service
        .audit(
            Some(user_id),
            "settings.migrate_path",
            Some(field),
            Some(serde_json::json!({
                "old": current.to_string_lossy().as_ref(),
                "new": new_str.as_ref(),
            })),
        )
        .await;

    if let Err(e) = tokio::fs::remove_dir_all(current).await {
        tracing::warn!("Could not remove old directory {current:?} after migration: {e}");
    }

    let _ = service.refresh_tx.send(AppEvent::PathMigrationCompleted {
        field: field.to_owned(),
        new_path: new.to_string_lossy().into_owned(),
    });

    Ok(())
}

async fn copy_tree(
    service: &AppService,
    field: &str,
    src: &Path,
    dst: &Path,
    total_bytes: u64,
) -> Result<()> {
    let mut bytes_copied: u64 = 0;
    let mut files_since_last: u32 = 0;
    let mut bytes_since_last: u64 = 0;
    copy_dir(
        service,
        field,
        src,
        dst,
        total_bytes,
        &mut bytes_copied,
        &mut files_since_last,
        &mut bytes_since_last,
    )
    .await
}

// Recursive async calls require an indirection boundary.
#[allow(clippy::too_many_arguments)]
fn copy_dir<'a>(
    service: &'a AppService,
    field: &'a str,
    src: &'a Path,
    dst: &'a Path,
    total_bytes: u64,
    bytes_copied: &'a mut u64,
    files_since_last: &'a mut u32,
    bytes_since_last: &'a mut u64,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        tokio::fs::create_dir_all(dst).await?;

        let mut entries = tokio::fs::read_dir(src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let entry_path = entry.path();
            let file_type = entry.file_type().await?;

            if file_type.is_symlink() {
                tracing::debug!("Skipping symlink during migration: {entry_path:?}");
                continue;
            }

            let name = entry.file_name();
            let dest = dst.join(&name);

            if file_type.is_dir() {
                copy_dir(
                    service,
                    field,
                    &entry_path,
                    &dest,
                    total_bytes,
                    bytes_copied,
                    files_since_last,
                    bytes_since_last,
                )
                .await?;
            } else if file_type.is_file() {
                let copied = tokio::fs::copy(&entry_path, &dest).await?;
                *bytes_copied += copied;
                *files_since_last += 1;
                *bytes_since_last += copied;

                if *bytes_since_last >= 1_048_576 || *files_since_last >= 100 {
                    let _ = service.refresh_tx.send(AppEvent::PathMigrationProgress {
                        field: field.to_owned(),
                        bytes_copied: *bytes_copied,
                        total_bytes,
                    });
                    *bytes_since_last = 0;
                    *files_since_last = 0;
                }
            }
        }
        Ok(())
    })
}

async fn dir_size(path: &Path) -> Result<u64> {
    let mut total: u64 = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let ft = entry.file_type().await?;
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                total += entry.metadata().await?.len();
            }
        }
    }
    Ok(total)
}

fn path_has_null(p: &Path) -> bool {
    p.to_string_lossy().contains('\0')
}

fn normalize_io_error(e: &ServiceError) -> String {
    match e {
        ServiceError::Validation(msg) => msg.clone(),
        ServiceError::Io(io) => match io.kind() {
            std::io::ErrorKind::PermissionDenied => "permission denied".into(),
            std::io::ErrorKind::NotFound => "path not found".into(),
            std::io::ErrorKind::AlreadyExists => "destination already exists".into(),
            _ => "I/O error during migration".into(),
        },
        _ => "migration failed".into(),
    }
}

fn fmt_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes >= GIB {
        format!("{:.1} GB", bytes as f64 / GIB as f64)
    } else {
        format!("{:.0} MB", bytes as f64 / MIB as f64)
    }
}
