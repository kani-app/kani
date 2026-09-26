use std::sync::Arc;

use serde::{Deserialize, Serialize};

use kani_core::http::SmartResponse;

use crate::{
    events::AppEvent,
    models::{BlockedRepo, RepoRow},
    source::{loader, signing},
};

use super::{AppService, Result, ServiceError};

const MAX_INDEX_BYTES: usize = 1024 * 1024;
const MAX_ARTIFACT_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepoIndex {
    pub name: String,
    pub maintainer_key: String,
    pub extensions: Vec<RepoExtensionEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepoExtensionEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub format: String,
    pub sha256: String,
    pub signature: String,
    pub author_key: String,
    pub min_kani_version: Option<String>,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nsfw: bool,
}

#[derive(Debug)]
pub enum RepoAddResult {
    Added {
        id: i64,
        name: String,
    },
    ConfirmationRequired {
        fingerprint: String,
        repo_url: String,
    },
    KeyChanged {
        old_fingerprint: String,
        new_fingerprint: String,
        repo_url: String,
    },
}

impl AppService {
    pub async fn list_repos(&self) -> Result<Vec<RepoRow>> {
        let rows = sqlx::query_as!(
            RepoRow,
            r#"SELECT id as "id!", url, name, maintainer_key, trusted_level, last_refreshed_at, index_cache, created_at FROM repo_trust ORDER BY name"#
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows)
    }

    pub async fn get_repo(&self, id: i64) -> Result<RepoRow> {
        sqlx::query_as!(
            RepoRow,
            r#"SELECT id as "id!", url, name, maintainer_key, trusted_level, last_refreshed_at, index_cache, created_at FROM repo_trust WHERE id = ?"#,
            id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Repository {id} not found")))
    }

    pub async fn add_repo(
        &self,
        url: &str,
        confirm_fingerprint: Option<&str>,
        user_id: Option<crate::ids::UserId>,
    ) -> Result<RepoAddResult> {
        self.check_repo_blocked(url).await?;

        let (index, index_bytes) = self.fetch_and_verify_index(url).await?;
        let new_key = index.maintainer_key.clone();
        let new_fp = fingerprint_from_b64(&new_key).map_err(ServiceError::Validation)?;

        let existing = sqlx::query!(
            r#"SELECT id as "id!", maintainer_key FROM repo_trust WHERE url = ?"#,
            url
        )
        .fetch_optional(&self.db_read)
        .await?;

        if let Some(row) = existing {
            if row.maintainer_key != new_key {
                let old_fp =
                    fingerprint_from_b64(&row.maintainer_key).map_err(ServiceError::Validation)?;
                return Ok(RepoAddResult::KeyChanged {
                    old_fingerprint: old_fp,
                    new_fingerprint: new_fp,
                    repo_url: url.to_string(),
                });
            }
            let index_json =
                serde_json::to_string(&index).map_err(|e| ServiceError::Internal(e.to_string()))?;
            let name = index.name.clone();
            sqlx::query!(
                "UPDATE repo_trust SET name = ?, last_refreshed_at = datetime('now'), \
                 index_cache = ? WHERE id = ?",
                name,
                index_json,
                row.id
            )
            .execute(&self.db)
            .await?;
            let _ = index_bytes;
            self.audit(user_id, "repo.refresh", Some(url), None).await;
            return Ok(RepoAddResult::Added { id: row.id, name });
        }

        if let Some(fp) = confirm_fingerprint {
            if fp != new_fp {
                return Ok(RepoAddResult::ConfirmationRequired {
                    fingerprint: new_fp,
                    repo_url: url.to_string(),
                });
            }
            let index_json =
                serde_json::to_string(&index).map_err(|e| ServiceError::Internal(e.to_string()))?;
            let name = index.name.clone();
            let id = sqlx::query_scalar!(
                "INSERT INTO repo_trust (url, name, maintainer_key, index_cache) \
                 VALUES (?, ?, ?, ?) RETURNING id",
                url,
                name,
                new_key,
                index_json
            )
            .fetch_one(&self.db)
            .await?;
            self.audit(
                user_id,
                "repo.trust",
                Some(url),
                Some(serde_json::json!({ "fingerprint": new_fp })),
            )
            .await;
            Ok(RepoAddResult::Added { id, name })
        } else {
            Ok(RepoAddResult::ConfirmationRequired {
                fingerprint: new_fp,
                repo_url: url.to_string(),
            })
        }
    }

    pub async fn refresh_repo(&self, id: i64, user_id: Option<crate::ids::UserId>) -> Result<()> {
        let repo = self.get_repo(id).await?;
        self.check_repo_blocked(&repo.url).await?;

        let base = repo.url.trim_end_matches('/');
        let index_url = format!("{base}/index.json");

        let index_resp = self
            .proxy_client
            .safe_get_conditional(&index_url, None)
            .await
            .map_err(|e| ServiceError::Internal(format!("Failed to fetch index.json: {e}")))?;

        let index = if matches!(index_resp, SmartResponse::NotModified { .. }) {
            sqlx::query!(
                "UPDATE repo_trust SET last_refreshed_at = datetime('now') WHERE id = ?",
                id
            )
            .execute(&self.db)
            .await?;
            parse_index_cache(&repo)?
        } else {
            let index_bytes = index_resp
                .bytes_limited(MAX_INDEX_BYTES)
                .await
                .map_err(|e| ServiceError::Internal(format!("Failed to read index.json: {e}")))?
                .to_vec();
            let parsed: RepoIndex = serde_json::from_slice(&index_bytes)
                .map_err(|e| ServiceError::Validation(format!("Invalid index.json: {e}")))?;

            if parsed.maintainer_key != repo.maintainer_key {
                return Err(ServiceError::Validation(
                    "Repository maintainer key changed since last trust — re-add the repo to confirm the new key.".to_string(),
                ));
            }

            let sig_url = format!("{base}/index.json.sig");
            let sig_raw = self
                .proxy_client
                .safe_get(&sig_url, None)
                .await
                .map_err(|e| {
                    ServiceError::Internal(format!("Failed to fetch index.json.sig: {e}"))
                })?
                .bytes_limited(512)
                .await
                .map_err(|e| {
                    ServiceError::Internal(format!("Failed to read index.json.sig: {e}"))
                })?;
            let sig_b64 = std::str::from_utf8(&sig_raw)
                .map_err(|_| {
                    ServiceError::Validation("index.json.sig is not valid UTF-8".to_string())
                })?
                .trim()
                .to_string();

            signing::verify_artifact(&index_bytes, &repo.maintainer_key, &sig_b64)
                .map_err(|e| ServiceError::Validation(format!("Index signature invalid: {e}")))?;

            let index_json = serde_json::to_string(&parsed)
                .map_err(|e| ServiceError::Internal(e.to_string()))?;
            sqlx::query!(
                "UPDATE repo_trust SET name = ?, last_refreshed_at = datetime('now'), \
                 index_cache = ? WHERE id = ?",
                parsed.name,
                index_json,
                id
            )
            .execute(&self.db)
            .await?;

            parsed
        };

        self.audit(user_id, "repo.refresh", Some(&repo.url), None)
            .await;
        let _ = self.refresh_tx.send(AppEvent::RepoRefreshed {
            repo_id: id,
            repo_name: index.name.clone(),
        });

        for entry in &index.extensions {
            let installed = sqlx::query!(
                "SELECT id as \"id!\", version FROM sources WHERE name = ? AND deleted_at IS NULL",
                entry.id
            )
            .fetch_optional(&self.db_read)
            .await?;
            if let Some(row) = installed
                && is_newer_version(&entry.version, &row.version)
            {
                let _ = self.refresh_tx.send(AppEvent::UpdateAvailable {
                    source_id: row.id,
                    source_name: entry.name.clone(),
                    installed_version: row.version,
                    available_version: entry.version.clone(),
                    repo_id: id,
                });
            }
        }
        Ok(())
    }

    pub async fn remove_repo(&self, id: i64, user_id: Option<crate::ids::UserId>) -> Result<()> {
        let repo = self.get_repo(id).await?;
        sqlx::query!("DELETE FROM repo_trust WHERE id = ?", id)
            .execute(&self.db)
            .await?;
        self.audit(user_id, "repo.remove", Some(&repo.url), None)
            .await;
        Ok(())
    }

    pub async fn list_repo_extensions(&self, id: i64) -> Result<Vec<RepoExtensionEntry>> {
        let repo = self.get_repo(id).await?;
        let index = parse_index_cache(&repo)?;
        Ok(index.extensions)
    }

    pub async fn install_source_from_repo(
        &self,
        repo_id: i64,
        extension_id: &str,
        user_id: Option<crate::ids::UserId>,
    ) -> Result<i64> {
        self.install_or_update_from_repo(repo_id, extension_id, None, user_id)
            .await
    }

    pub async fn update_source_from_repo(
        &self,
        repo_id: i64,
        extension_id: &str,
        existing_source_id: i64,
        user_id: Option<crate::ids::UserId>,
    ) -> Result<()> {
        self.install_or_update_from_repo(repo_id, extension_id, Some(existing_source_id), user_id)
            .await?;
        Ok(())
    }

    pub async fn list_blocked_repos(&self) -> Result<Vec<BlockedRepo>> {
        let rows = sqlx::query_as!(
            BlockedRepo,
            r#"SELECT id as "id!", url, reason, created_at FROM blocked_repos ORDER BY url"#
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows)
    }

    pub async fn block_repo(
        &self,
        url: &str,
        reason: &str,
        user_id: Option<crate::ids::UserId>,
    ) -> Result<()> {
        sqlx::query!(
            "INSERT INTO blocked_repos (url, reason) VALUES (?, ?) \
             ON CONFLICT(url) DO UPDATE SET reason = excluded.reason",
            url,
            reason
        )
        .execute(&self.db)
        .await?;
        self.audit(
            user_id,
            "repo.block",
            Some(url),
            Some(serde_json::json!({ "reason": reason })),
        )
        .await;
        Ok(())
    }

    pub async fn delete_blocked_repo(
        &self,
        id: i64,
        user_id: Option<crate::ids::UserId>,
    ) -> Result<()> {
        let url = sqlx::query_scalar!(r#"SELECT url FROM blocked_repos WHERE id = ?"#, id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Blocked repo {id} not found")))?;
        sqlx::query!("DELETE FROM blocked_repos WHERE id = ?", id)
            .execute(&self.db)
            .await?;
        self.audit(user_id, "repo.unblock", Some(&url), None).await;
        Ok(())
    }

    pub async fn bootstrap_official_repo(&self, url: &str, maintainer_key_b64: &str) -> Result<()> {
        let existing = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM repo_trust WHERE url = ? AND trusted_level = 'official'",
            url
        )
        .fetch_one(&self.db_read)
        .await?;
        if existing > 0 {
            return Ok(());
        }

        let fp = fingerprint_from_b64(maintainer_key_b64).map_err(ServiceError::Validation)?;

        match self.add_repo(url, Some(&fp), None).await? {
            RepoAddResult::Added { id, .. } => {
                sqlx::query!(
                    "UPDATE repo_trust SET trusted_level = 'official' WHERE id = ?",
                    id
                )
                .execute(&self.db)
                .await?;
                tracing::info!("Bootstrapped official repo: {url}");
            }
            RepoAddResult::ConfirmationRequired { fingerprint, .. } => {
                tracing::warn!(
                    "Official repo bootstrap skipped: key mismatch (expected {fp}, fetched {fingerprint})"
                );
            }
            RepoAddResult::KeyChanged {
                new_fingerprint, ..
            } => {
                tracing::warn!(
                    "Official repo bootstrap skipped: key changed since last trust \
                     (expected {fp}, fetched {new_fingerprint})"
                );
            }
        }

        Ok(())
    }

    async fn check_repo_blocked(&self, url: &str) -> Result<()> {
        let blocked = sqlx::query_scalar!("SELECT reason FROM blocked_repos WHERE url = ?", url)
            .fetch_optional(&self.db_read)
            .await?;
        if let Some(reason) = blocked {
            return Err(ServiceError::Forbidden(format!(
                "Repository is blocked: {reason}"
            )));
        }
        Ok(())
    }

    async fn fetch_and_verify_index(&self, repo_url: &str) -> Result<(RepoIndex, Vec<u8>)> {
        let base = repo_url.trim_end_matches('/');
        let index_url = format!("{base}/index.json");
        let sig_url = format!("{base}/index.json.sig");

        let index_bytes = self
            .proxy_client
            .safe_get(&index_url, None)
            .await
            .map_err(|e| ServiceError::Internal(format!("Failed to fetch index.json: {e}")))?
            .bytes_limited(MAX_INDEX_BYTES)
            .await
            .map_err(|e| ServiceError::Internal(format!("Failed to read index.json: {e}")))?
            .to_vec();

        let index: RepoIndex = serde_json::from_slice(&index_bytes)
            .map_err(|e| ServiceError::Validation(format!("Invalid index.json: {e}")))?;

        let sig_raw = self
            .proxy_client
            .safe_get(&sig_url, None)
            .await
            .map_err(|e| ServiceError::Internal(format!("Failed to fetch index.json.sig: {e}")))?
            .bytes_limited(512)
            .await
            .map_err(|e| ServiceError::Internal(format!("Failed to read index.json.sig: {e}")))?;
        let sig_b64 = std::str::from_utf8(&sig_raw)
            .map_err(|_| ServiceError::Validation("index.json.sig is not valid UTF-8".to_string()))?
            .trim()
            .to_string();

        signing::verify_artifact(&index_bytes, &index.maintainer_key, &sig_b64)
            .map_err(|e| ServiceError::Validation(format!("Index signature invalid: {e}")))?;

        Ok((index, index_bytes))
    }

    async fn install_or_update_from_repo(
        &self,
        repo_id: i64,
        extension_id: &str,
        existing_source_id: Option<i64>,
        user_id: Option<crate::ids::UserId>,
    ) -> Result<i64> {
        let lock = self
            .install_locks
            .entry(extension_id.to_string())
            .or_default()
            .clone();
        let _install_guard = lock.lock().await;

        let repo = self.get_repo(repo_id).await?;
        self.check_repo_blocked(&repo.url).await?;

        let index = parse_index_cache(&repo)?;
        let entry = index
            .extensions
            .iter()
            .find(|e| e.id == extension_id)
            .ok_or_else(|| {
                ServiceError::NotFound(format!(
                    "Extension '{extension_id}' not found in repository"
                ))
            })?
            .clone();

        crate::install_gating::check_min_kani_version(
            entry.min_kani_version.as_deref(),
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(ServiceError::Validation)?;

        if let Some(sid) = existing_source_id {
            let _ = self.refresh_tx.send(AppEvent::SourceUpdating {
                source_id: sid,
                source_name: entry.name.clone(),
            });
        }

        let artifact_url = resolve_url(&repo.url, &entry.url);
        // The repo is the trust anchor; its artifacts must live on its own host.
        // A cross-host artifact URL is an SSRF/redirection vector (and would slip
        // an IP-literal past the DNS-only resolver), so refuse it before dialling.
        if url_authority(&artifact_url) != url_authority(&repo.url) {
            return Err(ServiceError::Validation(format!(
                "Extension artifact host does not match the repository host: {artifact_url}"
            )));
        }
        let artifact_bytes = self
            .proxy_client
            .safe_get(&artifact_url, None)
            .await
            .map_err(|e| ServiceError::Internal(format!("Failed to download extension: {e}")))?
            .bytes_limited(MAX_ARTIFACT_BYTES)
            .await
            .map_err(|e| ServiceError::Internal(format!("Failed to read extension: {e}")))?
            .to_vec();

        signing::verify_sha256(&artifact_bytes, &entry.sha256)
            .map_err(|e| ServiceError::Validation(format!("Integrity check failed: {e}")))?;
        signing::verify_artifact(&artifact_bytes, &entry.author_key, &entry.signature)
            .map_err(|e| ServiceError::Validation(format!("Signature verification failed: {e}")))?;

        let settings = self.settings.read().await;
        let storage_path = settings
            .wasm_storage_path
            .to_str()
            .ok_or_else(|| ServiceError::Internal("Failed to convert storage path".to_string()))?
            .to_string();
        drop(settings);

        let source_id = match entry.format.as_str() {
            "yaml" => {
                self.install_yaml_artifact(
                    &artifact_bytes,
                    &storage_path,
                    existing_source_id,
                    Some(extension_id),
                )
                .await?
            }
            "wasm" => {
                self.install_wasm_artifact(
                    &artifact_bytes,
                    &storage_path,
                    existing_source_id,
                    Some(extension_id),
                )
                .await?
            }
            fmt => {
                return Err(ServiceError::Validation(format!(
                    "Unknown extension format: {fmt}"
                )));
            }
        };

        let action = if existing_source_id.is_some() {
            "source.update"
        } else {
            "source.install"
        };
        self.audit(
            user_id,
            action,
            Some(extension_id),
            Some(serde_json::json!({ "repo": repo.url, "version": entry.version })),
        )
        .await;
        let _ = self.refresh_tx.send(AppEvent::SourceInstalled {
            source_id,
            source_name: extension_id.to_string(),
            from_repo: repo.url,
        });

        Ok(source_id)
    }

    /// Install a compiled WASM extension from raw bytes: the manual counterpart to
    /// [`Self::install_yaml_source`], finding or creating the source row by the
    /// artifact's own id. Runs the same pipeline as a repository install.
    pub async fn install_wasm_source(&self, bytes: &[u8]) -> Result<i64> {
        let storage_path = self.storage_path_string().await?;
        self.install_wasm_artifact(bytes, &storage_path, None, None)
            .await
    }

    /// Replace the artifact of an existing source with compiled WASM bytes. The
    /// artifact must declare the source's own id, exactly as a repository update must.
    pub async fn update_wasm_source(&self, source_id: i64, bytes: &[u8]) -> Result<i64> {
        let storage_path = self.storage_path_string().await?;
        self.install_wasm_artifact(bytes, &storage_path, Some(source_id), None)
            .await
    }

    async fn storage_path_string(&self) -> Result<String> {
        let settings = self.settings.read().await;
        settings
            .wasm_storage_path
            .to_str()
            .map(str::to_string)
            .ok_or_else(|| ServiceError::Internal("Failed to convert storage path".to_string()))
    }

    /// Install an interpreted-YAML extension from raw YAML bytes — the manual
    /// add-source counterpart to [`Self::install_wasm_source`]. Validates, saves to
    /// storage, upserts the source row (find-or-create/revive by the YAML's own
    /// id, so reinstalling a previously-removed source works), and hot-loads the
    /// backend. Returns the source id.
    pub async fn install_yaml_source(&self, bytes: &[u8]) -> Result<i64> {
        let settings = self.settings.read().await;
        let storage_path = settings
            .wasm_storage_path
            .to_str()
            .ok_or_else(|| ServiceError::Internal("Failed to convert storage path".to_string()))?
            .to_string();
        drop(settings);
        self.install_yaml_artifact(bytes, &storage_path, None, None)
            .await
    }

    async fn install_yaml_artifact(
        &self,
        bytes: &[u8],
        storage_path: &str,
        existing_id: Option<i64>,
        expected_id: Option<&str>,
    ) -> Result<i64> {
        let text = std::str::from_utf8(bytes).map_err(|_| {
            ServiceError::Validation("YAML artifact is not valid UTF-8".to_string())
        })?;
        let text_owned = text.to_owned();
        let validated = tokio::task::spawn_blocking(move || {
            stacker::grow(16 * 1024 * 1024, || {
                kani_yaml::parse_and_validate(&text_owned, std::path::Path::new("extension.yaml"))
            })
        })
        .await
        .map_err(|e| ServiceError::Internal(format!("YAML validation task panicked: {e}")))?
        .map_err(|errs| {
            let msg = errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            ServiceError::Validation(format!("Invalid YAML extension: {msg}"))
        })?;

        self.check_artifact_identity(&validated.id, expected_id, existing_id)
            .await?;
        self.check_artifact(&crate::install_gating::ArtifactFacts {
            id: &validated.id,
            min_kani_version: validated.min_kani_version.as_deref(),
            dsl_schema_version: None,
            requires_capabilities: &validated.requires_capabilities,
        })
        .await?;

        let previous_version = self.stored_version(existing_id, &validated.id).await?;
        let previous = kani_core::file_storage::snapshot_artifacts(storage_path, &validated.id)
            .await
            .map_err(ServiceError::Core)?;
        let written = async {
            kani_core::file_storage::save_yaml(storage_path, &validated.id, text)
                .await
                .map_err(ServiceError::Core)?;
            kani_core::file_storage::delete_wasm_file(storage_path, &validated.id)
                .await
                .map_err(ServiceError::Core)?;
            self.upsert_yaml_source_row(&validated, existing_id).await
        }
        .await;
        let sid = match written {
            Ok(sid) => sid,
            Err(e) => {
                restore_after_failed_install(storage_path, &validated.id, previous).await;
                return Err(e);
            }
        };
        if previous_version.is_some_and(|v| v != validated.version) {
            crate::cache::invalidate_extension_cache(self.ext_cache.as_ref(), &validated.id, sid)
                .await;
        }

        let prefs = self.load_pref_map(sid).await.unwrap_or_default();
        let ns = format!("{}:", validated.id);
        let browser_enabled = self.browser_enabled_flag(sid).await;
        let backend = loader::build_yaml_source(
            Arc::new(validated),
            crate::service::local_network::client_for(&self.db, sid, &self.smart_client).await,
            Arc::clone(&self.ext_cache),
            ns,
            prefs,
            browser_enabled,
        );
        if existing_id.is_some() {
            self.sources.hot_swap(sid, backend).await;
        } else {
            self.sources.insert(sid, backend);
        }
        self.cache.invalidate_source(sid);
        Ok(sid)
    }

    async fn install_wasm_artifact(
        &self,
        bytes: &[u8],
        storage_path: &str,
        existing_id: Option<i64>,
        expected_id: Option<&str>,
    ) -> Result<i64> {
        let bytes_owned = bytes.to_vec();
        let runtime_clone = self.wasm_runtime.clone();
        let component =
            tokio::task::spawn_blocking(move || runtime_clone.compile_component(&bytes_owned))
                .await
                .map_err(|e| ServiceError::Internal(format!("Compile task panicked: {e}")))?
                .map_err(|e| {
                    ServiceError::Validation(format!("Not a loadable WASM extension: {e}"))
                })?;

        let (metadata, raw_schema) = {
            let mut inst =
                kani_core::sources::SourceInstance::new(self.smart_client.clone(), None, false);
            inst.load(
                self.wasm_runtime.engine(),
                &component,
                self.wasm_runtime.linker(),
            )
            .await
            .map_err(ServiceError::Core)?;
            let raw = inst.get_metadata().await.map_err(ServiceError::Core)?;
            let schema = inst.get_preferences().await.ok();
            let meta: kani_shared::ExtensionMetadata = serde_json::from_str(&raw).map_err(|e| {
                ServiceError::Validation(format!("Invalid extension metadata: {e}"))
            })?;
            (meta, schema)
        };

        crate::install_gating::check_extension_id(&metadata.id)
            .map_err(ServiceError::Validation)?;
        self.check_artifact_identity(&metadata.id, expected_id, existing_id)
            .await?;
        self.check_artifact(&crate::install_gating::ArtifactFacts {
            id: &metadata.id,
            min_kani_version: metadata.min_kani_version.as_deref(),
            dsl_schema_version: metadata.dsl_schema_version,
            requires_capabilities: &metadata.requires_capabilities,
        })
        .await?;
        let instance_pre = self
            .wasm_runtime
            .instantiate_pre(&component)
            .map_err(ServiceError::Core)?;

        let previous_version = self.stored_version(existing_id, &metadata.id).await?;
        let previous = kani_core::file_storage::snapshot_artifacts(storage_path, &metadata.id)
            .await
            .map_err(ServiceError::Core)?;
        let written = async {
            kani_core::file_storage::save_wasm(storage_path, &metadata.id, bytes)
                .await
                .map_err(ServiceError::Core)?;
            kani_core::file_storage::delete_yaml_file(storage_path, &metadata.id)
                .await
                .map_err(ServiceError::Core)?;
            self.upsert_wasm_source_row(&metadata, existing_id).await
        }
        .await;
        let sid = match written {
            Ok(sid) => sid,
            Err(e) => {
                restore_after_failed_install(storage_path, &metadata.id, previous).await;
                return Err(e);
            }
        };
        if previous_version.is_some_and(|v| v != metadata.version) {
            crate::cache::invalidate_extension_cache(self.ext_cache.as_ref(), &metadata.id, sid)
                .await;
        }

        let prefs = self.load_pref_map(sid).await.unwrap_or_default();
        let ns = format!("{}:", metadata.id);
        let pure_reg = super::sources::compile_pure_registry(&metadata);
        let hook_reg = super::sources::compile_hook_registry(&metadata);
        let max_hk = metadata
            .rate_limit
            .as_ref()
            .map(|rl| rl.max_hook_requests)
            .unwrap_or(3);
        let backend = loader::build_wasm_source(
            self.wasm_runtime.engine().clone(),
            instance_pre,
            crate::service::local_network::client_for(&self.db, sid, &self.smart_client).await,
            Some(metadata.base_url),
            metadata.unrestricted_http,
            self.browser_enabled_flag(sid).await,
            prefs,
            Arc::clone(&self.ext_cache),
            ns,
            pure_reg,
            hook_reg,
            max_hk,
        );
        if existing_id.is_some() {
            self.sources.hot_swap(sid, backend).await;
        } else {
            self.sources.insert(sid, backend);
        }
        if let Some(schema) = raw_schema {
            self.cache.insert_preference_schema(sid, schema);
        }
        self.cache.invalidate_source(sid);
        Ok(sid)
    }

    /// Refuses an artifact that fails any check every install path applies to the artifact
    /// itself, whichever way it arrived.
    async fn check_artifact(&self, facts: &crate::install_gating::ArtifactFacts<'_>) -> Result<()> {
        let solver =
            crate::install_gating::solver_for(facts.requires_capabilities, &self.smart_client)
                .await;
        match crate::install_gating::check_artifact(facts, env!("CARGO_PKG_VERSION"), solver)
            .into_iter()
            .next()
        {
            Some(problem) => Err(ServiceError::Validation(problem)),
            None => Ok(()),
        }
    }

    /// Refuses an artifact whose own id differs from the repository entry it was installed
    /// from, or from the source it is updating. Files and rows are keyed by the artifact's
    /// id, so a mismatch would overwrite a different source.
    async fn check_artifact_identity(
        &self,
        artifact_id: &str,
        expected_id: Option<&str>,
        existing_id: Option<i64>,
    ) -> Result<()> {
        if let Some(expected) = expected_id
            && artifact_id != expected
        {
            return Err(ServiceError::Validation(format!(
                "Artifact declares id '{artifact_id}' but the repository lists it as '{expected}'"
            )));
        }
        if let Some(id) = existing_id {
            let name: Option<String> = sqlx::query_scalar("SELECT name FROM sources WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.db_read)
                .await?;
            if name.as_deref() != Some(artifact_id) {
                return Err(ServiceError::Validation(format!(
                    "Artifact declares id '{artifact_id}' but is updating source {id} ({})",
                    name.as_deref().unwrap_or("missing")
                )));
            }
        }
        Ok(())
    }

    async fn stored_version(
        &self,
        existing_id: Option<i64>,
        extension_id: &str,
    ) -> Result<Option<String>> {
        let version: Option<String> = match existing_id {
            Some(id) => {
                sqlx::query_scalar("SELECT version FROM sources WHERE id = ?")
                    .bind(id)
                    .fetch_optional(&self.db_read)
                    .await?
            }
            None => {
                sqlx::query_scalar("SELECT version FROM sources WHERE name = ?")
                    .bind(extension_id)
                    .fetch_optional(&self.db_read)
                    .await?
            }
        };
        Ok(version)
    }

    async fn upsert_yaml_source_row(
        &self,
        ext: &kani_yaml::ValidatedExtension,
        existing_id: Option<i64>,
    ) -> Result<i64> {
        let languages_json = serde_json::to_string(&ext.metadata.languages)
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let schema_version = ext.schema_version as i64;

        if let Some(id) = existing_id {
            sqlx::query!(
                "UPDATE sources SET name = ?, version = ?, base_url = ?, unrestricted_http = ?, \
                 schema_version = ?, languages = ?, description = ?, icon = ?, \
                 load_error = NULL, enabled = 1, deleted_at = NULL WHERE id = ?",
                ext.id,
                ext.version,
                ext.base_url,
                ext.unrestricted_http,
                schema_version,
                languages_json,
                ext.metadata.description,
                ext.metadata.icon,
                id
            )
            .execute(&self.db)
            .await?;
            return Ok(id);
        }

        let existing = sqlx::query_scalar!("SELECT id FROM sources WHERE name = ?", ext.id)
            .fetch_optional(&self.db_read)
            .await?;

        if let Some(id) = existing {
            sqlx::query!(
                "UPDATE sources SET version = ?, base_url = ?, unrestricted_http = ?, \
                 schema_version = ?, languages = ?, description = ?, icon = ?, \
                 load_error = NULL, enabled = 1, deleted_at = NULL WHERE id = ?",
                ext.version,
                ext.base_url,
                ext.unrestricted_http,
                schema_version,
                languages_json,
                ext.metadata.description,
                ext.metadata.icon,
                id
            )
            .execute(&self.db)
            .await?;
            return Ok(id);
        }

        let id = sqlx::query_scalar!(
            "INSERT INTO sources (name, version, base_url, enabled, unrestricted_http, \
             schema_version, languages, description, icon) \
             VALUES (?, ?, ?, 1, ?, ?, ?, ?, ?) RETURNING id",
            ext.id,
            ext.version,
            ext.base_url,
            ext.unrestricted_http,
            schema_version,
            languages_json,
            ext.metadata.description,
            ext.metadata.icon
        )
        .fetch_one(&self.db)
        .await?;
        Ok(id)
    }

    async fn upsert_wasm_source_row(
        &self,
        meta: &kani_shared::ExtensionMetadata,
        existing_id: Option<i64>,
    ) -> Result<i64> {
        let languages_json = serde_json::to_string(&meta.languages)
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let schema_version = meta.schema_version as i64;

        if let Some(id) = existing_id {
            sqlx::query!(
                "UPDATE sources SET name = ?, version = ?, base_url = ?, unrestricted_http = ?, \
                 icon = ?, description = ?, languages = ?, schema_version = ?, \
                 load_error = NULL, enabled = 1, deleted_at = NULL WHERE id = ?",
                meta.id,
                meta.version,
                meta.base_url,
                meta.unrestricted_http,
                meta.icon,
                meta.description,
                languages_json,
                schema_version,
                id
            )
            .execute(&self.db)
            .await?;
            return Ok(id);
        }

        let existing = sqlx::query_scalar!("SELECT id FROM sources WHERE name = ?", meta.id)
            .fetch_optional(&self.db_read)
            .await?;

        let id = if let Some(id) = existing {
            sqlx::query!(
                "UPDATE sources SET version = ?, base_url = ?, unrestricted_http = ?, \
                 icon = ?, description = ?, languages = ?, schema_version = ?, \
                 load_error = NULL, enabled = 1, deleted_at = NULL WHERE id = ?",
                meta.version,
                meta.base_url,
                meta.unrestricted_http,
                meta.icon,
                meta.description,
                languages_json,
                schema_version,
                id
            )
            .execute(&self.db)
            .await?;
            id
        } else {
            sqlx::query_scalar!(
                "INSERT INTO sources (name, version, base_url, enabled, unrestricted_http, \
                 icon, description, languages, schema_version) \
                 VALUES (?, ?, ?, 1, ?, ?, ?, ?, ?) RETURNING id",
                meta.id,
                meta.version,
                meta.base_url,
                meta.unrestricted_http,
                meta.icon,
                meta.description,
                languages_json,
                schema_version,
            )
            .fetch_one(&self.db)
            .await?
        };

        Ok(id)
    }
}

fn parse_index_cache(repo: &RepoRow) -> Result<RepoIndex> {
    let json = repo.index_cache.as_deref().ok_or_else(|| {
        ServiceError::Internal("Repository index not yet fetched — refresh first".to_string())
    })?;
    serde_json::from_str(json)
        .map_err(|e| ServiceError::Internal(format!("Failed to parse repo index: {e}")))
}

/// The `host[:port]` authority of an absolute URL, used to keep an extension
/// artifact on the same host as the repository that vouches for it.
fn url_authority(u: &str) -> Option<&str> {
    let rest = u.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    Some(authority.rsplit('@').next().unwrap_or(authority))
}

fn resolve_url(base: &str, artifact_url: &str) -> String {
    if artifact_url.starts_with("http://") || artifact_url.starts_with("https://") {
        artifact_url.to_string()
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            artifact_url.trim_start_matches('/')
        )
    }
}

fn is_newer_version(candidate: &str, installed: &str) -> bool {
    match (
        semver::Version::parse(candidate.trim_start_matches('v')),
        semver::Version::parse(installed.trim_start_matches('v')),
    ) {
        (Ok(c), Ok(i)) => c > i,
        _ => candidate != installed,
    }
}

fn fingerprint_from_b64(b64: &str) -> std::result::Result<String, String> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    let bytes = B64
        .decode(b64)
        .map_err(|e| format!("Invalid base64 public key: {e}"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "Public key must be 32 bytes".to_string())?;
    Ok(signing::key_fingerprint(&arr))
}

async fn restore_after_failed_install(
    storage_path: &str,
    extension_id: &str,
    previous: kani_core::file_storage::ArtifactSnapshot,
) {
    if let Err(e) =
        kani_core::file_storage::restore_artifacts(storage_path, extension_id, previous).await
    {
        tracing::error!(
            "Install of '{extension_id}' failed and its previous artifacts could not be restored: {e}"
        );
    }
}
