use base64::Engine;
use sha2::Digest;

use super::{
    TrackerMangaResult, TrackerRegistry, consume_pkce_state, delete_credentials,
    delete_tracker_app_config, get_access_token, get_external_ids, get_mapping_row,
    get_tracker_app_config, set_tracker_app_config, store_credentials, store_pkce_state,
};
use crate::error::{Result, ServiceError};
use crate::ids::{MangaId, UserId};
use crate::service::AppService;

/// Item returned by `list_trackers_status`.
pub struct TrackerStatusItem {
    pub id: i64,
    pub name: String,
    /// Whether app credentials (client_id / secret) have been configured.
    pub configured: bool,
    /// Whether this user has linked their account.
    pub linked: bool,
    /// The stored credentials were rejected (refresh refused, or a 401 during
    /// sync) — the link exists but cannot authenticate until re-linked.
    pub needs_reauth: bool,
}

/// Mapping entry returned by `get_tracker_mappings`.
pub struct TrackerMappingItem {
    pub tracker_id: i64,
    pub tracker_name: &'static str,
    pub tracker_manga_id: Option<String>,
    /// When this mapping last synced. `None` while it never has.
    pub last_synced_at: Option<time::OffsetDateTime>,
    /// Catalogue ID inferred from metadata enrichment when no user mapping exists.
    pub suggested_manga_id: Option<String>,
}

/// The `manga_external_ids.provider` key a tracker's entries are stored under.
fn provider_key(tracker_name: &str) -> Option<&'static str> {
    match tracker_name {
        "AniList" => Some("anilist"),
        "MyAnimeList" => Some("mal"),
        _ => None,
    }
}

impl AppService {
    /// List all known trackers with per-user link status.
    pub async fn list_trackers_status(&self, user_id: UserId) -> Result<Vec<TrackerStatusItem>> {
        let rows = sqlx::query!("SELECT id, name FROM trackers ORDER BY name")
            .fetch_all(&self.db_read)
            .await?;

        let registry = self.tracker_registry.read().await;
        let mut items = Vec::new();

        for row in rows {
            let tracker_id = row
                .id
                .ok_or_else(|| ServiceError::Internal("tracker row missing id".into()))?;
            let configured = registry.trackers.contains_key(&tracker_id);
            let cred = sqlx::query!(
                "SELECT needs_reauth AS \"needs_reauth: bool\" FROM user_tracker_credentials \
                 WHERE user_id = ? AND tracker_id = ?",
                user_id,
                tracker_id,
            )
            .fetch_optional(&self.db_read)
            .await
            .ok()
            .flatten();

            items.push(TrackerStatusItem {
                id: tracker_id,
                name: row.name,
                configured,
                linked: cred.is_some(),
                needs_reauth: cred.map(|c| c.needs_reauth).unwrap_or(false),
            });
        }

        Ok(items)
    }

    /// Build the OAuth authorization URL and persist the CSRF/PKCE state.
    /// Returns the URL the frontend should open.
    pub async fn get_tracker_auth_url(
        &self,
        tracker_id: i64,
        redirect_uri: &str,
    ) -> Result<String> {
        let registry = self.tracker_registry.read().await;
        let tracker = registry
            .get(tracker_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Tracker {tracker_id} not found")))?;

        let state_bytes: [u8; 32] = rand::random();
        let state_token = hex::encode(state_bytes);

        let (code_verifier, code_challenge) = if tracker.requires_pkce() {
            let verifier_bytes: [u8; 32] = rand::random();
            let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);
            let hash = sha2::Sha256::digest(verifier.as_bytes());
            let challenge =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash.as_slice());
            (Some(verifier), Some(challenge))
        } else {
            (None, None)
        };

        let url = tracker.auth_url(redirect_uri, &state_token, code_challenge.as_deref());
        drop(registry);

        store_pkce_state(
            &self.db,
            &state_token,
            code_verifier.as_deref(),
            tracker_id,
            redirect_uri,
        )
        .await?;

        Ok(url)
    }

    /// Complete the OAuth callback: validate state, exchange code, store tokens.
    pub async fn complete_tracker_oauth(
        &self,
        user_id: UserId,
        tracker_id: i64,
        code: &str,
        state: &str,
    ) -> Result<()> {
        let pkce = consume_pkce_state(&self.db, state).await?.ok_or_else(|| {
            ServiceError::Validation("OAuth state is invalid or has expired".into())
        })?;

        if pkce.tracker_id != tracker_id {
            return Err(ServiceError::Validation(
                "Tracker ID mismatch in OAuth state".into(),
            ));
        }

        let tokens = {
            let registry = self.tracker_registry.read().await;
            let tracker = registry
                .get(tracker_id)
                .ok_or_else(|| ServiceError::NotFound(format!("Tracker {tracker_id} not found")))?;
            tracker
                .exchange_code(code, &pkce.redirect_uri, pkce.code_verifier.as_deref())
                .await?
        };

        store_credentials(
            &self.db,
            user_id,
            tracker_id,
            &tokens,
            self.encryption.as_deref(),
        )
        .await?;
        Ok(())
    }

    /// Unlink a tracker account for a user.
    pub async fn unlink_tracker(&self, user_id: UserId, tracker_id: i64) -> Result<()> {
        delete_credentials(&self.db, user_id, tracker_id).await
    }

    /// Search a tracker's catalog by title.
    pub async fn search_tracker_manga(
        &self,
        user_id: UserId,
        tracker_id: i64,
        query: &str,
    ) -> Result<Vec<TrackerMangaResult>> {
        let registry = self.tracker_registry.read().await;
        let tracker = registry
            .get(tracker_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Tracker {tracker_id} not found")))?;
        let access_token = get_access_token(
            &self.db,
            tracker_id,
            user_id,
            tracker,
            self.encryption.as_deref(),
        )
        .await?;
        let results = tracker.search_manga(&access_token, query).await?;
        Ok(results)
    }

    /// Includes an entry for every configured tracker, even when no mapping exists.
    pub async fn get_tracker_mappings(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<Vec<TrackerMappingItem>> {
        let registry = self.tracker_registry.read().await;
        let external = get_external_ids(&self.db_read, manga_id)
            .await
            .unwrap_or_default();
        let mut mappings = Vec::new();
        for (&tid, tracker) in &registry.trackers {
            let row = get_mapping_row(&self.db, user_id, tid, manga_id).await?;
            let (tracker_manga_id, last_synced_at) = match row {
                Some((id, synced)) => (Some(id), synced),
                None => (None, None),
            };
            // Only worth offering while nothing is mapped; once the user has
            // chosen, their choice stands.
            let suggested_manga_id = if tracker_manga_id.is_some() {
                None
            } else {
                provider_key(tracker.name()).and_then(|key| {
                    external
                        .iter()
                        .find(|(p, _)| p == key)
                        .map(|(_, id)| id.clone())
                })
            };
            mappings.push(TrackerMappingItem {
                tracker_id: tid,
                tracker_name: tracker.name(),
                tracker_manga_id,
                last_synced_at,
                suggested_manga_id,
            });
        }
        Ok(mappings)
    }

    /// Returns the client ID and whether a secret is set, never the secret itself.
    pub async fn get_tracker_config(&self, tracker_id: i64) -> Result<Option<(String, bool)>> {
        get_tracker_app_config(&self.db, tracker_id).await
    }

    /// Save tracker app config and hot-reload the registry.
    pub async fn set_tracker_config(
        &self,
        tracker_id: i64,
        client_id: &str,
        client_secret: Option<&str>,
    ) -> Result<()> {
        let exists = sqlx::query_scalar!("SELECT COUNT(*) FROM trackers WHERE id = ?", tracker_id)
            .fetch_one(&self.db_read)
            .await
            .unwrap_or(0)
            > 0;
        if !exists {
            return Err(ServiceError::NotFound(format!(
                "Tracker {tracker_id} not found"
            )));
        }

        set_tracker_app_config(
            &self.db,
            tracker_id,
            client_id,
            client_secret,
            self.encryption.as_deref(),
        )
        .await?;
        self.reload_tracker_registry().await
    }

    /// Delete tracker app config and hot-reload the registry.
    pub async fn delete_tracker_config(&self, tracker_id: i64) -> Result<()> {
        delete_tracker_app_config(&self.db, tracker_id).await?;
        self.reload_tracker_registry().await
    }

    pub async fn set_tracker_mapping(
        &self,
        user_id: UserId,
        tracker_id: i64,
        manga_id: MangaId,
        tracker_manga_id: &str,
    ) -> Result<()> {
        super::set_mapping(&self.db, user_id, tracker_id, manga_id, tracker_manga_id).await
    }

    pub async fn delete_tracker_mapping(
        &self,
        user_id: UserId,
        tracker_id: i64,
        manga_id: MangaId,
    ) -> Result<()> {
        super::delete_mapping(&self.db, user_id, tracker_id, manga_id).await
    }

    /// Rebuild the tracker registry in-place (hot-reload after credential changes).
    pub(crate) async fn reload_tracker_registry(&self) -> Result<()> {
        *self.tracker_registry.write().await =
            TrackerRegistry::new(&self.db, self.encryption.as_deref()).await?;
        Ok(())
    }
}
