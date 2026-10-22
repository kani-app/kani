pub mod anilist;
pub mod mal;
pub mod service;
pub mod sync;

pub use service::{TrackerMappingItem, TrackerStatusItem};

use crate::error::{Result, ServiceError};
use crate::ids::{MangaId, UserId};
use crate::service::encryption::{CredentialCipher, maybe_decrypt, maybe_encrypt};
use kani_shared::types::MangaTrackingStatus;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;

/// If the response is an HTTP 429, returns a `ServiceError::RateLimited` carrying the
/// parsed `Retry-After` (seconds) header when present. Callers should propagate this so
/// the periodic sync job can back off per access token.
pub(crate) fn rate_limited_error(resp: &rquest::Response) -> Option<ServiceError> {
    if resp.status().as_u16() != 429 {
        return None;
    }
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok());
    Some(ServiceError::RateLimited {
        retry_after_secs: retry_after,
    })
}

/// If the response rejects our credentials (401/403), returns a
/// `ServiceError::TrackerAuthExpired`. The token is dead regardless of what
/// `expires_at` claims, so the caller must flag the link for re-authentication
/// rather than retry it.
pub(crate) fn auth_expired_error(resp: &rquest::Response, provider: &str) -> Option<ServiceError> {
    let code = resp.status().as_u16();
    match code {
        401 | 403 => Some(ServiceError::TrackerAuthExpired(format!(
            "{provider} rejected the stored credentials (HTTP {code})"
        ))),
        _ => None,
    }
}

/// Classifies rate limits and rejected credentials before provider-specific body parsing.
pub(crate) fn check_tracker_response(
    resp: rquest::Response,
    provider: &str,
) -> Result<rquest::Response> {
    if let Some(e) = rate_limited_error(&resp) {
        return Err(e);
    }
    if let Some(e) = auth_expired_error(&resp, provider) {
        return Err(e);
    }
    Ok(resp)
}

/// Record that a tracker link can no longer authenticate, so the settings page
/// can prompt the user to re-link. Best-effort: a failure here must not mask
/// the original error.
pub(crate) async fn mark_needs_reauth(db: &SqlitePool, user_id: UserId, tracker_id: i64) {
    let _ = sqlx::query!(
        "UPDATE user_tracker_credentials SET needs_reauth = TRUE \
         WHERE user_id = ? AND tracker_id = ?",
        user_id,
        tracker_id,
    )
    .execute(db)
    .await;
}

/// Token response from an OAuth exchange or refresh.
#[derive(Debug, Clone)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<time::OffsetDateTime>,
}

/// A search result from an external tracker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerMangaResult {
    pub tracker_manga_id: String,
    pub title: String,
    pub cover_url: Option<String>,
}

/// Reading status as reported by an external tracker.
#[derive(Debug, Clone)]
pub struct TrackerMangaStatus {
    pub status: Option<MangaTrackingStatus>,
    pub score: Option<f64>,
    pub chapters_read: i64,
}

/// HTTP client for tracker API calls with a 30-second bound on stalled providers.
pub(super) fn tracker_http_client() -> rquest::Client {
    rquest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| rquest::Client::new())
}

/// Trait that all external tracker integrations must implement.
#[async_trait::async_trait]
pub trait ExternalTracker: Send + Sync {
    /// Human-readable name (e.g. "AniList", "MyAnimeList").
    fn name(&self) -> &'static str;

    /// Whether this tracker requires PKCE (S256) for its OAuth flow.
    fn requires_pkce(&self) -> bool {
        false
    }

    /// Build the OAuth authorization URL the user should visit.
    /// `code_challenge` is the base64url(SHA-256(code_verifier)) for PKCE providers,
    /// and None for standard authorization-code providers.
    fn auth_url(&self, redirect_uri: &str, state: &str, code_challenge: Option<&str>) -> String;

    /// Exchange an authorization code for tokens.
    /// `code_verifier` is provided for PKCE providers and None otherwise.
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        code_verifier: Option<&str>,
    ) -> Result<TokenResponse>;

    /// Refresh an expired access token.
    async fn refresh_token(&self, refresh_token: &str) -> Result<TokenResponse>;

    /// Search the tracker's catalog by title.
    async fn search_manga(
        &self,
        access_token: &str,
        query: &str,
    ) -> Result<Vec<TrackerMangaResult>>;

    /// Push local reading status to the remote tracker.
    async fn update_status(
        &self,
        access_token: &str,
        tracker_manga_id: &str,
        status: MangaTrackingStatus,
        score: Option<f64>,
        chapters_read: i64,
    ) -> Result<()>;

    /// Pull the current status from the remote tracker.
    async fn get_status(
        &self,
        access_token: &str,
        tracker_manga_id: &str,
    ) -> Result<TrackerMangaStatus>;
}

/// Registry of all available trackers, keyed by their DB id.
pub struct TrackerRegistry {
    pub trackers: HashMap<i64, Box<dyn ExternalTracker>>,
}

impl TrackerRegistry {
    /// Create a new registry. Checks DB config first, falls back to env vars.
    /// Always ensures rows exist for all known tracker types so the UI can
    /// reference them by stable ID even before credentials are configured.
    pub async fn new(db: &SqlitePool, cipher: Option<&CredentialCipher>) -> Result<Self> {
        let mut trackers: HashMap<i64, Box<dyn ExternalTracker>> = HashMap::new();

        let anilist_id = Self::ensure_tracker_row(db, "AniList").await?;
        let anilist = Self::load_tracker_config(db, "AniList", cipher).await?;
        let anilist_creds = anilist
            .and_then(|(id, secret)| secret.map(|s| (id, s)))
            .or_else(|| {
                let id = std::env::var("KANI_ANILIST_CLIENT_ID").ok()?;
                let secret = std::env::var("KANI_ANILIST_CLIENT_SECRET").ok()?;
                Some((id, secret))
            });
        if let Some((client_id, client_secret)) = anilist_creds {
            trackers.insert(
                anilist_id,
                Box::new(anilist::AnilistTracker::new(client_id, client_secret)),
            );
        }

        let mal_id = Self::ensure_tracker_row(db, "MyAnimeList").await?;
        let mal = Self::load_tracker_config(db, "MyAnimeList", cipher).await?;
        let mal_client_id = mal
            .map(|(id, _)| id)
            .or_else(|| std::env::var("KANI_MAL_CLIENT_ID").ok());
        if let Some(client_id) = mal_client_id {
            trackers.insert(mal_id, Box::new(mal::MalTracker::new(client_id)));
        }

        Ok(Self { trackers })
    }

    /// Ensure a tracker row exists in the DB, returning its id.
    async fn ensure_tracker_row(db: &SqlitePool, name: &str) -> Result<i64> {
        sqlx::query!("INSERT OR IGNORE INTO trackers (name) VALUES (?)", name)
            .execute(db)
            .await?;
        let id = sqlx::query_scalar!("SELECT id FROM trackers WHERE name = ?", name)
            .fetch_one(db)
            .await?
            .ok_or_else(|| {
                ServiceError::Internal(format!("Tracker '{name}' not found after insert"))
            })?;
        Ok(id)
    }

    /// Load tracker app config from the DB. Returns `(client_id, client_secret)` or None.
    /// `client_secret` is decrypted if a cipher is provided.
    async fn load_tracker_config(
        db: &SqlitePool,
        name: &str,
        cipher: Option<&CredentialCipher>,
    ) -> Result<Option<(String, Option<String>)>> {
        let row = sqlx::query!(
            r#"SELECT tac.client_id, tac.client_secret
               FROM tracker_app_config tac
               JOIN trackers t ON t.id = tac.tracker_id
               WHERE t.name = ?"#,
            name
        )
        .fetch_optional(db)
        .await?;
        Ok(row.map(|r| {
            let secret = r
                .client_secret
                .as_deref()
                .and_then(|s| match maybe_decrypt(cipher, s) {
                    Ok(plain) => Some(plain),
                    Err(e) => {
                        tracing::warn!(
                            "Cannot decrypt client_secret for {name}: {e}. Treating as unset."
                        );
                        None
                    }
                });
            (r.client_id, secret)
        }))
    }

    pub fn get(&self, tracker_id: i64) -> Option<&dyn ExternalTracker> {
        self.trackers.get(&tracker_id).map(|b| b.as_ref())
    }
}

/// Returns `(client_id, secret_is_configured)`. Never returns the secret value.
pub(crate) async fn get_tracker_app_config(
    db: &SqlitePool,
    tracker_id: i64,
) -> Result<Option<(String, bool)>> {
    let row = sqlx::query!(
        "SELECT client_id, client_secret FROM tracker_app_config WHERE tracker_id = ?",
        tracker_id
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| (r.client_id, r.client_secret.is_some())))
}

/// Upsert tracker app config. Ensures the tracker row exists first.
/// `client_secret` is encrypted before storage if a cipher is provided.
pub(crate) async fn set_tracker_app_config(
    db: &SqlitePool,
    tracker_id: i64,
    client_id: &str,
    client_secret: Option<&str>,
    cipher: Option<&CredentialCipher>,
) -> Result<()> {
    let encrypted_secret = client_secret.map(|s| maybe_encrypt(cipher, s));
    sqlx::query!(
        r#"INSERT INTO tracker_app_config (tracker_id, client_id, client_secret)
           VALUES (?1, ?2, ?3)
           ON CONFLICT (tracker_id) DO UPDATE SET
               client_id = excluded.client_id,
               client_secret = COALESCE(excluded.client_secret, tracker_app_config.client_secret)"#,
        tracker_id,
        client_id,
        encrypted_secret,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Delete tracker app config and all associated user credentials.
pub(crate) async fn delete_tracker_app_config(db: &SqlitePool, tracker_id: i64) -> Result<()> {
    sqlx::query!(
        "DELETE FROM tracker_app_config WHERE tracker_id = ?",
        tracker_id
    )
    .execute(db)
    .await?;
    // Cascade via FK will delete user_tracker_credentials rows, but
    // tracker_manga_mappings references trackers(id) not tracker_app_config.
    sqlx::query!(
        "DELETE FROM user_tracker_credentials WHERE tracker_id = ?",
        tracker_id
    )
    .execute(db)
    .await?;
    sqlx::query!(
        "DELETE FROM tracker_manga_mappings WHERE tracker_id = ?",
        tracker_id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Persist a server-generated OAuth state token (and optional code_verifier for PKCE).
pub async fn store_pkce_state(
    db: &SqlitePool,
    state: &str,
    code_verifier: Option<&str>,
    tracker_id: i64,
    redirect_uri: &str,
) -> Result<()> {
    sqlx::query!("DELETE FROM oauth_pkce_state WHERE created_at < datetime('now', '-10 minutes')")
        .execute(db)
        .await?;

    sqlx::query!(
        r#"INSERT INTO oauth_pkce_state (state, code_verifier, tracker_id, redirect_uri)
           VALUES (?1, ?2, ?3, ?4)"#,
        state,
        code_verifier,
        tracker_id,
        redirect_uri,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Consume (look up and delete) a state token. Returns None if not found or expired.
pub struct PkceState {
    pub code_verifier: Option<String>,
    pub tracker_id: i64,
    pub redirect_uri: String,
}

pub async fn consume_pkce_state(db: &SqlitePool, state: &str) -> Result<Option<PkceState>> {
    // Compare inside SQLite because this column uses SQLite's datetime text format,
    // not the RFC 3339 representation used at API boundaries.
    let row = sqlx::query!(
        r#"SELECT code_verifier, tracker_id, redirect_uri
           FROM oauth_pkce_state
           WHERE state = ? AND created_at >= datetime('now', '-10 minutes')"#,
        state
    )
    .fetch_optional(db)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    // Delete on consume (single-use).
    sqlx::query!("DELETE FROM oauth_pkce_state WHERE state = ?", state)
        .execute(db)
        .await?;

    Ok(Some(PkceState {
        code_verifier: row.code_verifier,
        tracker_id: row.tracker_id,
        redirect_uri: row.redirect_uri,
    }))
}

/// Store OAuth tokens for a user+tracker. Encrypts tokens before storage if a cipher is provided.
pub async fn store_credentials(
    db: &SqlitePool,
    user_id: UserId,
    tracker_id: i64,
    tokens: &TokenResponse,
    cipher: Option<&CredentialCipher>,
) -> Result<()> {
    let expires_str = tokens.expires_at.map(|t| {
        t.format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default()
    });
    let access_token = maybe_encrypt(cipher, &tokens.access_token);
    let refresh_token = tokens
        .refresh_token
        .as_deref()
        .map(|t| maybe_encrypt(cipher, t));
    sqlx::query!(
        r#"INSERT INTO user_tracker_credentials (user_id, tracker_id, access_token, refresh_token, expires_at)
           VALUES (?1, ?2, ?3, ?4, ?5)
           ON CONFLICT (user_id, tracker_id) DO UPDATE SET
               access_token = excluded.access_token,
               refresh_token = COALESCE(excluded.refresh_token, user_tracker_credentials.refresh_token),
               expires_at = excluded.expires_at,
               needs_reauth = FALSE"#,
        user_id,
        tracker_id,
        access_token,
        refresh_token,
        expires_str,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Fetch an access token, refreshing automatically if expired.
/// Decrypts tokens from storage if a cipher is provided.
pub async fn get_access_token(
    db: &SqlitePool,
    tracker_id: i64,
    user_id: UserId,
    tracker: &dyn ExternalTracker,
    cipher: Option<&CredentialCipher>,
) -> Result<String> {
    let row = sqlx::query!(
        r#"SELECT access_token, refresh_token, expires_at AS "expires_at: String"
           FROM user_tracker_credentials WHERE user_id = ? AND tracker_id = ?"#,
        user_id,
        tracker_id,
    )
    .fetch_optional(db)
    .await?
    .ok_or_else(|| ServiceError::NotFound(format!("No credentials for tracker {tracker_id}")))?;

    let needs_refresh = row
        .expires_at
        .as_deref()
        .and_then(|exp| {
            time::OffsetDateTime::parse(exp, &time::format_description::well_known::Rfc3339).ok()
        })
        .map(|t| t < time::OffsetDateTime::now_utc() + time::Duration::seconds(60))
        .unwrap_or(false);

    if needs_refresh {
        let refresh_plain = row
            .refresh_token
            .as_deref()
            .map(|t| maybe_decrypt(cipher, t))
            .transpose()
            .map_err(|e| ServiceError::Internal(format!("Cannot decrypt refresh token: {e}")))?;
        if let Some(ref refresh) = refresh_plain {
            match tracker.refresh_token(refresh).await {
                Ok(new_tokens) => {
                    store_credentials(db, user_id, tracker_id, &new_tokens, cipher).await?;
                    return Ok(new_tokens.access_token);
                }
                Err(e) => {
                    // A refresh the provider rejects will keep being rejected.
                    // Flag the link so the user is told to re-authenticate,
                    // instead of retrying the same doomed call on every sync.
                    mark_needs_reauth(db, user_id, tracker_id).await;
                    return Err(e);
                }
            }
        }
    }

    let stored = row
        .access_token
        .ok_or_else(|| ServiceError::Internal("Missing access token".into()))?;
    maybe_decrypt(cipher, &stored)
        .map_err(|e| ServiceError::Internal(format!("Cannot decrypt access token: {e}")))
}

pub(crate) async fn delete_credentials(
    db: &SqlitePool,
    user_id: UserId,
    tracker_id: i64,
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM user_tracker_credentials WHERE user_id = ? AND tracker_id = ?",
        user_id,
        tracker_id,
    )
    .execute(db)
    .await?;
    sqlx::query!(
        "DELETE FROM tracker_manga_mappings WHERE user_id = ? AND tracker_id = ?",
        user_id,
        tracker_id,
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn set_mapping(
    db: &SqlitePool,
    user_id: UserId,
    tracker_id: i64,
    manga_id: MangaId,
    tracker_manga_id: &str,
) -> Result<()> {
    sqlx::query!(
        r#"INSERT INTO tracker_manga_mappings (user_id, tracker_id, manga_id, tracker_manga_id)
           VALUES (?1, ?2, ?3, ?4)
           ON CONFLICT (user_id, tracker_id, manga_id) DO UPDATE SET
               tracker_manga_id = excluded.tracker_manga_id"#,
        user_id,
        tracker_id,
        manga_id,
        tracker_manga_id,
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_mapping(
    db: &SqlitePool,
    user_id: UserId,
    tracker_id: i64,
    manga_id: MangaId,
) -> Result<Option<String>> {
    let row = sqlx::query_scalar!(
        "SELECT tracker_manga_id FROM tracker_manga_mappings WHERE user_id = ? AND tracker_id = ? AND manga_id = ?",
        user_id,
        tracker_id,
        manga_id,
    )
    .fetch_optional(db)
    .await?;
    Ok(row)
}

/// A mapping plus the last time it synced, for display.
pub(crate) async fn get_mapping_row(
    db: &SqlitePool,
    user_id: UserId,
    tracker_id: i64,
    manga_id: MangaId,
) -> Result<Option<(String, Option<time::OffsetDateTime>)>> {
    let row = sqlx::query!(
        "SELECT tracker_manga_id, last_synced_at FROM tracker_manga_mappings \
         WHERE user_id = ? AND tracker_id = ? AND manga_id = ?",
        user_id,
        tracker_id,
        manga_id,
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| (r.tracker_manga_id, r.last_synced_at)))
}

/// Ids this series is known by on external catalogues, keyed by provider.
///
/// Written by metadata enrichment and backfilled from the `anilist_id` /
/// `mal_id` columns that `20260614000002` dropped. Nothing read them until the
/// tracker panel began offering them as link suggestions.
pub(crate) async fn get_external_ids(
    db: &SqlitePool,
    manga_id: MangaId,
) -> Result<Vec<(String, String)>> {
    let rows = sqlx::query!(
        "SELECT provider, external_id FROM manga_external_ids WHERE manga_id = ?",
        manga_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.provider, r.external_id))
        .collect())
}

pub async fn delete_mapping(
    db: &SqlitePool,
    user_id: UserId,
    tracker_id: i64,
    manga_id: MangaId,
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM tracker_manga_mappings WHERE user_id = ? AND tracker_id = ? AND manga_id = ?",
        user_id,
        tracker_id,
        manga_id,
    )
    .execute(db)
    .await?;
    Ok(())
}
