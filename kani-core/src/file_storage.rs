//! File storage utilities for WASM files.

use std::path::PathBuf;
use tokio::fs;

use crate::error::{Error, Result};

/// WASM magic bytes: \0asm
const WASM_MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6D];

/// Validates WASM magic and stores the module under a sanitized name confined
/// to `wasm_storage_path`.
pub async fn save_wasm(wasm_storage_path: &str, name: &str, bytes: &[u8]) -> Result<PathBuf> {
    let name = crate::utilities::sanitize_filename(name);

    if !validate_wasm_magic(bytes) {
        return Err(Error::InvalidWasm);
    }

    let dir = PathBuf::from(wasm_storage_path);
    fs::create_dir_all(&dir).await?;

    let filename = format!("{}.wasm", name);
    let path = dir.join(&filename);

    let canonical_dir = dir.canonicalize()?;

    if let Some(parent) = path.parent()
        && parent.canonicalize()? != canonical_dir
    {
        return Err(Error::PathTraversal(name));
    }

    write_atomic(&path, bytes).await?;
    Ok(path)
}

/// Stores YAML under a sanitized name confined to `wasm_storage_path`.
pub async fn save_yaml(wasm_storage_path: &str, name: &str, content: &str) -> Result<PathBuf> {
    let name = crate::utilities::sanitize_filename(name);

    let dir = PathBuf::from(wasm_storage_path);
    fs::create_dir_all(&dir).await?;

    let filename = format!("{}.yaml", name);
    let path = dir.join(&filename);

    let canonical_dir = dir.canonicalize()?;

    if let Some(parent) = path.parent()
        && parent.canonicalize()? != canonical_dir
    {
        return Err(Error::PathTraversal(name));
    }

    write_atomic(&path, content.as_bytes()).await?;
    Ok(path)
}

async fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let mut staging = path.as_os_str().to_owned();
    staging.push(".tmp");
    let staging = PathBuf::from(staging);
    fs::write(&staging, bytes).await?;
    if let Err(e) = fs::rename(&staging, path).await {
        let _ = fs::remove_file(&staging).await;
        return Err(e.into());
    }
    Ok(())
}

/// Both stored artifacts for one source, as they were before an install touched them.
#[derive(Debug, Default)]
pub struct ArtifactSnapshot {
    yaml: Option<Vec<u8>>,
    wasm: Option<Vec<u8>>,
}

fn confined_artifact_path(wasm_storage_path: &str, name: &str, ext: &str) -> Result<PathBuf> {
    let name = crate::utilities::sanitize_filename(name);
    let dir = PathBuf::from(wasm_storage_path);
    let path = dir.join(format!("{name}.{ext}"));
    if dir.exists()
        && let Some(parent) = path.parent()
        && parent.canonicalize()? != dir.canonicalize()?
    {
        return Err(Error::PathTraversal(name));
    }
    Ok(path)
}

async fn read_if_present(path: &std::path::Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn snapshot_artifacts(wasm_storage_path: &str, name: &str) -> Result<ArtifactSnapshot> {
    Ok(ArtifactSnapshot {
        yaml: read_if_present(&confined_artifact_path(wasm_storage_path, name, "yaml")?).await?,
        wasm: read_if_present(&confined_artifact_path(wasm_storage_path, name, "wasm")?).await?,
    })
}

/// Puts both artifacts back exactly as `snapshot` recorded them, deleting any that did not exist.
pub async fn restore_artifacts(
    wasm_storage_path: &str,
    name: &str,
    snapshot: ArtifactSnapshot,
) -> Result<()> {
    for (ext, previous) in [("yaml", snapshot.yaml), ("wasm", snapshot.wasm)] {
        let path = confined_artifact_path(wasm_storage_path, name, ext)?;
        match previous {
            Some(bytes) => write_atomic(&path, &bytes).await?,
            None => {
                if let Err(e) = fs::remove_file(&path).await
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    return Err(e.into());
                }
            }
        }
    }
    Ok(())
}

/// Deletes the confined, sanitized module path and succeeds if it is absent.
pub async fn delete_wasm_file(wasm_storage_path: &str, name: &str) -> Result<()> {
    let name = crate::utilities::sanitize_filename(name);
    let dir = PathBuf::from(wasm_storage_path);
    let filename = format!("{}.wasm", name);
    let path = dir.join(&filename);

    if path.exists() {
        let canonical_dir = dir.canonicalize()?;

        if let Some(parent) = path.parent()
            && parent.canonicalize()? != canonical_dir
        {
            return Err(Error::PathTraversal(name));
        }

        fs::remove_file(&path).await?;
    }
    Ok(())
}

pub async fn delete_yaml_file(wasm_storage_path: &str, name: &str) -> Result<()> {
    let name = crate::utilities::sanitize_filename(name);
    let dir = PathBuf::from(wasm_storage_path);
    let filename = format!("{}.yaml", name);
    let path = dir.join(&filename);

    if path.exists() {
        let canonical_dir = dir.canonicalize()?;

        if let Some(parent) = path.parent()
            && parent.canonicalize()? != canonical_dir
        {
            return Err(Error::PathTraversal(name));
        }

        fs::remove_file(&path).await?;
    }
    Ok(())
}

/// Validates that the bytes start with WASM magic bytes.
pub(crate) fn validate_wasm_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == WASM_MAGIC
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn valid_wasm() -> Vec<u8> {
        b"\0asm\x01\0\0\0".to_vec()
    }

    #[test]
    fn valid_magic_accepted() {
        assert!(validate_wasm_magic(&valid_wasm()));
    }

    #[test]
    fn random_bytes_rejected() {
        assert!(!validate_wasm_magic(&[0x01, 0x02, 0x03, 0x04]));
    }

    #[test]
    fn empty_bytes_rejected() {
        assert!(!validate_wasm_magic(&[]));
    }

    #[test]
    fn too_short_rejected() {
        assert!(!validate_wasm_magic(&[0x00, 0x61, 0x73]));
    }

    #[tokio::test]
    async fn save_wasm_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_wasm(dir.path().to_str().unwrap(), "test_ext", &valid_wasm())
            .await
            .unwrap();
        assert!(path.exists());
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("wasm"));
    }

    #[tokio::test]
    async fn save_wasm_rejects_non_wasm() {
        let dir = tempfile::tempdir().unwrap();
        let result = save_wasm(
            dir.path().to_str().unwrap(),
            "bad_ext",
            &[0x01, 0x02, 0x03, 0x04],
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_wasm_file_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().to_str().unwrap();
        save_wasm(storage, "ext_to_delete", &valid_wasm())
            .await
            .unwrap();
        delete_wasm_file(storage, "ext_to_delete").await.unwrap();
        assert!(!dir.path().join("ext_to_delete.wasm").exists());
    }

    #[tokio::test]
    async fn delete_wasm_file_nonexistent_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let result = delete_wasm_file(dir.path().to_str().unwrap(), "nonexistent").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn delete_yaml_file_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().to_str().unwrap();
        save_yaml(storage, "ext_to_delete", "id: ext_to_delete\n")
            .await
            .unwrap();
        delete_yaml_file(storage, "ext_to_delete").await.unwrap();
        assert!(!dir.path().join("ext_to_delete.yaml").exists());
    }

    #[tokio::test]
    async fn delete_yaml_file_nonexistent_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let result = delete_yaml_file(dir.path().to_str().unwrap(), "nonexistent").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn restore_undoes_a_format_switch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().to_str().unwrap();
        save_yaml(storage, "ext", "version: 1\n").await.unwrap();
        let snapshot = snapshot_artifacts(storage, "ext").await.unwrap();

        save_wasm(storage, "ext", &valid_wasm()).await.unwrap();
        delete_yaml_file(storage, "ext").await.unwrap();
        restore_artifacts(storage, "ext", snapshot).await.unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join("ext.yaml")).unwrap(),
            "version: 1\n"
        );
        assert!(
            !dir.path().join("ext.wasm").exists(),
            "an artifact absent from the snapshot must be removed"
        );
    }

    #[tokio::test]
    async fn saving_leaves_no_staging_file() {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().to_str().unwrap();
        save_yaml(storage, "ext", "version: 2\n").await.unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["ext.yaml".to_string()]);
    }

    #[test]
    fn exactly_four_magic_bytes_is_valid() {
        assert!(validate_wasm_magic(&WASM_MAGIC));
    }

    #[test]
    fn partial_magic_rejected() {
        assert!(!validate_wasm_magic(&[0x00, 0x61, 0x73]));
        assert!(!validate_wasm_magic(&[0x00, 0x61]));
        assert!(!validate_wasm_magic(&[0x00]));
    }

    #[test]
    fn correct_prefix_with_wrong_bytes_rejected() {
        assert!(!validate_wasm_magic(&[0x00, 0x62, 0x73, 0x6D]));
    }

    #[tokio::test]
    async fn save_wasm_name_with_special_chars_is_sanitized_and_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_wasm(dir.path().to_str().unwrap(), "my:source/ext", &valid_wasm())
            .await
            .unwrap();
        assert!(path.exists());
        assert_eq!(path.parent(), Some(dir.path()));
    }

    #[tokio::test]
    async fn save_wasm_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().to_str().unwrap();
        save_wasm(storage, "ext", &valid_wasm()).await.unwrap();
        let result = save_wasm(storage, "ext", &valid_wasm()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn save_yaml_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_yaml(dir.path().to_str().unwrap(), "my-source", "id: my-source\n")
            .await
            .unwrap();
        assert!(path.exists());
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("yaml"));
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "id: my-source\n");
    }

    #[tokio::test]
    async fn save_yaml_sanitizes_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_yaml(dir.path().to_str().unwrap(), "my:source/ext", "content")
            .await
            .unwrap();
        assert!(path.exists());
        assert_eq!(path.parent(), Some(dir.path()));
    }

    #[tokio::test]
    async fn save_yaml_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().to_str().unwrap();
        save_yaml(storage, "src", "v1").await.unwrap();
        let path = save_yaml(storage, "src", "v2").await.unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "v2");
    }
}
