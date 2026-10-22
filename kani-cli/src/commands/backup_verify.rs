use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::error::CliError;

/// Backup archive format this binary understands. A backup written by a newer
/// Kani may contain fields this build would silently drop, so restoring one is
/// refused rather than attempted.
pub(crate) const SUPPORTED_BACKUP_VERSION: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
/// Compatibility result for a backup's declared format version.
pub(crate) enum VersionCheck {
    Compatible(u32),
    TooNew { found: u32, supported: u32 },
    Unreadable(String),
}

pub(crate) fn check_backup_version(raw: &str) -> VersionCheck {
    match raw.trim().parse::<u32>() {
        Ok(v) if v <= SUPPORTED_BACKUP_VERSION => VersionCheck::Compatible(v),
        Ok(v) => VersionCheck::TooNew {
            found: v,
            supported: SUPPORTED_BACKUP_VERSION,
        },
        Err(_) => VersionCheck::Unreadable(raw.trim().to_string()),
    }
}

fn read_entry(archive: &mut zip::ZipArchive<File>, name: &str) -> Option<String> {
    let mut file = archive.by_name(name).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    Some(buf)
}

pub fn run(path: &Path) -> Result<(), CliError> {
    let file = File::open(path)
        .map_err(|e| CliError::Other(format!("cannot open {}: {e}", path.display())))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| CliError::Other(format!("{} is not a zip archive: {e}", path.display())))?;

    let raw = read_entry(&mut archive, "VERSION").ok_or_else(|| {
        CliError::Other("backup is missing its VERSION entry — not a Kani backup archive".into())
    })?;

    match check_backup_version(&raw) {
        VersionCheck::Compatible(v) => {
            println!(
                "backup format v{v} — compatible with this build (supports v{SUPPORTED_BACKUP_VERSION})"
            );
        }
        VersionCheck::TooNew { found, supported } => {
            return Err(CliError::Other(format!(
                "backup format v{found} was written by a newer Kani; this build supports \
                 v{supported}. Restoring it could silently drop data. Upgrade Kani first."
            )));
        }
        VersionCheck::Unreadable(raw) => {
            return Err(CliError::Other(format!(
                "backup VERSION entry is not a number: {raw:?}"
            )));
        }
    }

    if let Some(json) = read_entry(&mut archive, "backup.json")
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&json)
    {
        for key in ["manga", "categories", "sources", "settings"] {
            if let Some(arr) = value.get(key).and_then(|v| v.as_array()) {
                println!("  {key}: {} entries", arr.len());
            }
        }
    } else {
        println!("  (backup.json absent or unreadable — header check only)");
    }

    println!(
        "\nThis archive can be restored. Apply it from Settings → Storage, or POST it to \
         /rest/library/restore. Kani has no reverse migrations: restore only onto a build \
         at or newer than the one that wrote the backup, and back up the current data first."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn current_version_is_compatible() {
        assert_eq!(check_backup_version("1"), VersionCheck::Compatible(1));
    }

    #[test]
    fn whitespace_is_tolerated() {
        assert_eq!(check_backup_version(" 1\n"), VersionCheck::Compatible(1));
    }

    #[test]
    fn older_versions_are_compatible() {
        assert_eq!(check_backup_version("0"), VersionCheck::Compatible(0));
    }

    #[test]
    fn newer_version_is_refused() {
        assert_eq!(
            check_backup_version("2"),
            VersionCheck::TooNew {
                found: 2,
                supported: SUPPORTED_BACKUP_VERSION,
            }
        );
        assert!(matches!(
            check_backup_version("99"),
            VersionCheck::TooNew { .. }
        ));
    }

    #[test]
    fn garbage_is_refused_rather_than_assumed_compatible() {
        assert!(matches!(
            check_backup_version("one"),
            VersionCheck::Unreadable(_)
        ));
        assert!(matches!(
            check_backup_version(""),
            VersionCheck::Unreadable(_)
        ));
        assert!(matches!(
            check_backup_version("1.0"),
            VersionCheck::Unreadable(_)
        ));
        assert!(matches!(
            check_backup_version("-1"),
            VersionCheck::Unreadable(_)
        ));
    }
}
