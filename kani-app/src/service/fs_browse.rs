use std::path::{Component, Path, PathBuf};

use crate::error::ServiceError;

pub struct FsBrowseResult {
    pub canonical_path: PathBuf,
    pub segments: Vec<String>,
    pub dirs: Vec<String>,
    pub drives: Vec<String>,
}

/// Lists subdirectories of `raw_path`. Security: canonicalizes the path,
/// rejects null bytes, and returns only directory names (no files).
pub fn browse_directory(raw_path: &str) -> Result<FsBrowseResult, ServiceError> {
    if raw_path.contains('\0') {
        return Err(ServiceError::Validation("path contains null byte".into()));
    }

    let canonical = dunce::canonicalize(raw_path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => {
            ServiceError::NotFound(format!("path not found: {raw_path}"))
        }
        std::io::ErrorKind::PermissionDenied => {
            ServiceError::Validation("permission denied".into())
        }
        _ => ServiceError::Validation("path is inaccessible".into()),
    })?;

    if !canonical.is_dir() {
        return Err(ServiceError::Validation("path is not a directory".into()));
    }

    let mut dirs = Vec::new();
    let read_dir = std::fs::read_dir(&canonical).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            ServiceError::Validation("permission denied".into())
        }
        _ => ServiceError::Internal("unable to read directory".into()),
    })?;

    for entry in read_dir.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir()
            && let Some(name) = entry.file_name().to_str()
        {
            dirs.push(name.to_owned());
        }
    }
    dirs.sort_by_key(|a| a.to_lowercase());

    let segments = path_segments(&canonical);
    let drives = list_drives();

    Ok(FsBrowseResult {
        canonical_path: canonical,
        segments,
        dirs,
        drives,
    })
}

/// Creates a single directory named `name` inside `parent_raw`.
/// Security: canonicalizes parent, rejects separators and null bytes in name,
/// verifies no traversal occurred, creates one level only.
pub fn create_directory(parent_raw: &str, name: &str) -> Result<PathBuf, ServiceError> {
    if parent_raw.contains('\0') || name.contains('\0') {
        return Err(ServiceError::Validation("path contains null byte".into()));
    }
    if name.is_empty() {
        return Err(ServiceError::Validation(
            "directory name cannot be empty".into(),
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(ServiceError::Validation(
            "directory name cannot contain path separators".into(),
        ));
    }

    let canonical_parent = dunce::canonicalize(parent_raw).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            ServiceError::Validation("permission denied".into())
        }
        _ => ServiceError::Validation("parent directory is inaccessible".into()),
    })?;

    if !canonical_parent.is_dir() {
        return Err(ServiceError::Validation("parent is not a directory".into()));
    }

    let full = canonical_parent.join(name);

    // Guard: ensure no traversal slipped through
    if full.parent() != Some(canonical_parent.as_path()) {
        return Err(ServiceError::Validation("invalid directory name".into()));
    }

    std::fs::create_dir(&full).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => {
            ServiceError::Validation("directory already exists".into())
        }
        std::io::ErrorKind::PermissionDenied => {
            ServiceError::Validation("permission denied".into())
        }
        _ => ServiceError::Internal("unable to create directory".into()),
    })?;

    Ok(full)
}

fn path_segments(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(n) => n.to_str().map(|s| s.to_owned()),
            Component::RootDir => Some("/".to_owned()),
            Component::Prefix(p) => p.as_os_str().to_str().map(|s| s.to_owned()),
            _ => None,
        })
        .collect()
}

#[cfg(windows)]
fn list_drives() -> Vec<String> {
    (b'A'..=b'Z')
        .filter_map(|c| {
            let drive = format!("{}:\\", c as char);
            std::path::Path::new(&drive).exists().then_some(drive)
        })
        .collect()
}

#[cfg(not(windows))]
fn list_drives() -> Vec<String> {
    Vec::new()
}
