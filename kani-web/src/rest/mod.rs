//! Plain Axum REST handlers — mounted at /rest in main.rs.

use crate::{
    auth::{AuthBackend, AuthSession, Credentials},
    error::AppError,
    models::{
        AddDownloadRuleRequest, AddRepoRequest, AdminCreateRoleRequest, AdminCreateUserRequest,
        AdminGrantRoleRequest, AdminUpdateRoleRequest, AdminUpdateUserRequest, BlockRepoRequest,
        BulkMigrationRequest, ChangePasswordRequest, ContinueReadingShelfQuery,
        CreateCategoryRequest, FetchWasmRequest, FetchYamlRequest, GlobalSearchQuery,
        InstallFromRepoRequest, InstallYamlRequest, LibraryQuery, ListItemRequest,
        LocalChaptersQuery, LoginRequest, MarkUpToRequest, MatchMigrationTargetsRequest,
        MigrateMangaRequest, MigrationStatusRequest, PageQuery, PasswordResetConfirmBody,
        PasswordResetRequestBody, PreviewDownloadRulesRequest, PreviewMigrationRequest, ProxyQuery,
        RenameCategoryRequest, ReorderCategoriesRequest, ReorderDownloadRulesRequest,
        ScanMangaRequest, SearchMangaRequest, SendTestEmailBody, SetChapterNoteRequest,
        SetChapterProgressRequest, SetMangaCategoriesRequest, SetMangaTrackingRequest,
        SetPreferenceRequest, SetReadStatusRequest, SetScanlatorModeRequest,
        SetScanlatorPrefRequest, SetTrackerConfigRequest, SetTrackerMappingRequest, SolverTestBody,
        ToggleAutoDownloadRequest, ToggleEnabledRequest, ToggleFavouritedRequest,
        ToggleSelectRequest, TokenQuery, TrackerAuthUrlQuery, TrackerCallbackQuery,
        TrackerSearchQuery, UpdateDownloadRuleRequest, UpdateFromRepoRequest, UpdateSource,
    },
    permissions::AuthRequirement,
    state::AppState,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, FromRef, Multipart, Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, patch, post, put},
};
use axum_login::AuthnBackend;
use axum_login::AuthzBackend;
use futures::TryStreamExt;
pub use kani_app::ids::{ChapterId, MangaId, UserId};
pub use kani_app::service::traits::{
    CategoryDomain, ChapterDomain, DownloadDomain, JobDomain, LibraryDomain, MangaDomain,
    ScanlatorDomain, SettingsDomain, SourceDomain, TrackerDomain,
};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::marker::PhantomData;
use std::{convert::Infallible, sync::Arc};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

pub(crate) mod admin;
pub(crate) mod api_tokens;
pub(crate) mod auth;
pub(crate) mod categories;
pub(crate) mod chapters;
pub(crate) mod collections;
pub(crate) mod downloads;
pub(crate) mod export;
pub(crate) mod filters;
pub(crate) mod jobs;
pub(crate) mod library;
pub(crate) mod manga;
pub(crate) mod saved_searches;
pub(crate) mod scanlators;
pub(crate) mod settings;
pub(crate) mod sources;
pub(crate) mod sse;
pub(crate) mod stats;
pub(crate) mod system;
pub(crate) mod trackers;
pub(crate) mod ui_themes;
pub(crate) mod volumes;
pub(crate) mod webhooks;

fn sign_image_url(url: &str, referer: &str, state: &AppState, transform: Option<&str>) -> String {
    crate::proxy::make_proxy_url(url, referer, &state.proxy_secret, transform)
}

/// Without `h`, [`serve_manga_cover`] can only answer `max-age=3600`; a matching
/// prefix is what lets it mark the thumbnail immutable.
fn local_cover_url(id: impl std::fmt::Display, size: &str, cover_hash: Option<&str>) -> String {
    let mut url = format!("/rest/manga/{id}/cover?size={size}");
    if let Some(hash) = cover_hash.filter(|h| !h.is_empty()) {
        url.push_str("&h=");
        url.push_str(&hash[..hash.len().min(16)]);
    }
    url
}

pub(crate) struct ValidatedJson<T>(T);

impl<S, T> axum::extract::FromRequest<S> for ValidatedJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + garde::Validate<Context = ()>,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|e| AppError::ValidationError(e.to_string()))?;
        value
            .validate()
            .map_err(|e| AppError::ValidationError(e.to_string()))?;
        Ok(ValidatedJson(value))
    }
}

pub(crate) struct ValidatedQuery<T>(T);

impl<S, T> axum::extract::FromRequestParts<S> for ValidatedQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned + garde::Validate<Context = ()>,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(|e| AppError::ValidationError(e.to_string()))?;
        value
            .validate()
            .map_err(|e| AppError::ValidationError(e.to_string()))?;
        Ok(ValidatedQuery(value))
    }
}

const MAX_WASM_BYTES: usize = 10 * 1024 * 1024;
const MAX_BACKUP_BYTES: usize = 100 * 1024 * 1024;
const MAX_TACHI_BYTES: usize = 50 * 1024 * 1024;

/// Extractor that authenticates by bearer token or session and enforces `P` before dispatch.
/// An invalid explicit bearer token never falls back to session authentication.
pub(crate) struct AuthGuard<P: AuthRequirement>(pub crate::auth::User, pub PhantomData<P>);

impl<S, P> axum::extract::FromRequestParts<S> for AuthGuard<P>
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
    P: AuthRequirement,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        // A programmatic caller presents a bearer token and has no session. An explicit bearer
        // that fails to authenticate is refused outright rather than falling through to session
        // auth: silently downgrading would make a broken integration look like a working one.
        if let Some(raw) = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        {
            let app_state = AppState::from_ref(state);
            let auth = app_state
                .service
                .authenticate_api_token(raw)
                .await
                .map_err(|e| AppError::InternalServerError(e.to_string()))?
                .ok_or_else(|| AppError::Unauthorized("Invalid API token".into()))?;

            // Acceptance keys on kind, never on scope contents: a reader token
            // must not reach the REST API even if its scopes were widened.
            if auth.kind != kani_app::service::api_tokens::TokenKind::Api {
                return Err(AppError::Forbidden(
                    "This token is only valid for OPDS endpoints".into(),
                ));
            }

            if let Some(perm) = P::required_permission()
                && !auth.scopes.contains(&perm)
            {
                return Err(AppError::Forbidden(format!(
                    "Token lacks permission: {perm}"
                )));
            }

            let backend = crate::auth::AuthBackend::new(app_state.service.db.clone());
            let user = backend
                .fetch_user_by_id(auth.user_id)
                .await?
                .ok_or_else(|| AppError::Unauthorized("Token owner no longer exists".into()))?;

            return Ok(Self(user, PhantomData));
        }

        let auth_session = crate::auth::AuthSession::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::InternalServerError("Session error".into()))?;

        let user = auth_session
            .user
            .ok_or(AppError::Unauthorized("User not authenticated".into()))?;

        if let Some(perm) = P::required_permission()
            && !auth_session
                .backend
                .has_perm(&user, perm)
                .await
                .unwrap_or(false)
        {
            tracing::warn!(
                user_id = user.id.0,
                username = %user.username,
                permission = %perm,
                "Permission denied"
            );
            let app_state = AppState::from_ref(state);
            app_state
                .audit(
                    Some(user.id),
                    "auth.permission_denied",
                    Some(&perm.to_string()),
                    Some(serde_json::json!({ "username": user.username })),
                )
                .await;
            return Err(AppError::Forbidden(format!(
                "User lacks permission: {}",
                perm
            )));
        }

        Ok(Self(user, PhantomData))
    }
}

pub fn routes(state: AppState) -> Router {
    Router::new()
        .merge(auth::router())
        .merge(sse::router())
        .merge(sources::router())
        .merge(library::router())
        .merge(manga::router())
        .merge(chapters::router())
        .merge(scanlators::router())
        .merge(categories::router())
        .merge(downloads::router())
        .merge(jobs::router())
        .merge(trackers::router())
        .merge(ui_themes::router())
        .merge(filters::router())
        .merge(settings::router())
        .merge(volumes::router())
        .merge(collections::router())
        .merge(saved_searches::router())
        .merge(admin::router())
        .merge(stats::router())
        .merge(export::router())
        .merge(webhooks::router())
        .merge(system::router())
        .merge(api_tokens::router())
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_WASM_BYTES))
}

/// Mounts image-serving routes under the permissive rate limiter. The reader
/// fires many concurrent requests for page images and proxy images; these must
/// not share a bucket with the JSON API.
pub fn image_proxy_route(state: AppState) -> Router {
    Router::new()
        .route("/image_proxy", get(image_proxy))
        .route("/chapter/{id}/page/{page_num}", get(serve_chapter_page))
        .route("/manga/{id}/cover", get(serve_manga_cover))
        .with_state(state)
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub(crate) struct PasswordStrengthRequest {
    pub(crate) password: String,
    pub(crate) identity: Option<String>,
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub(crate) struct TotpCodeRequest {
    pub(crate) code: String,
}

/// The client's socket address, when the server was started with connect info.
///
/// Never fails: a synthetic request in a test simply has none, which callers
/// treat as "not a local client".
pub(crate) struct PeerAddr(pub(crate) Option<std::net::SocketAddr>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for PeerAddr {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(PeerAddr(
            parts
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|c| c.0),
        ))
    }
}

/// First-run account creation. No captcha: the endpoint exists only while the
/// instance has no users, so there is nothing to spam.
#[derive(serde::Deserialize, utoipa::ToSchema)]
pub(crate) struct SetupRequest {
    pub(crate) username: String,
    #[serde(default)]
    pub(crate) email: String,
    pub(crate) password: String,
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub(crate) struct RegisterRequest {
    pub(crate) username: String,
    pub(crate) email: String,
    pub(crate) password: String,
    pub(crate) captcha_id: String,
    pub(crate) captcha_answer: i64,
}

async fn resolve_path_field(state: &AppState, field: &str) -> Result<std::path::PathBuf, AppError> {
    let settings = state.settings.read().await;
    match field {
        "library_path" => Ok(settings.library_path.clone()),
        "wasm_storage_path" => Ok(settings.wasm_storage_path.clone()),
        other => Err(AppError::ValidationError(format!(
            "unknown path field: {other}"
        ))),
    }
}

fn proxy_retry_delay(
    headers: Option<&rquest::header::HeaderMap>,
    attempt: u32,
    cfg: &crate::proxy::ProxyConfig,
) -> std::time::Duration {
    let backoff = cfg.base_delay * 2u32.pow(attempt);
    let cap_secs = cfg.retry_after_cap.as_secs();
    headers
        .and_then(|h| h.get(rquest::header::RETRY_AFTER))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.parse::<u64>()
                .ok()
                .map(|secs| secs.min(cap_secs))
                .or_else(|| {
                    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc2822)
                        .ok()
                        .map(|dt| {
                            let now = time::OffsetDateTime::now_utc();
                            (dt - now).whole_seconds().max(0) as u64
                        })
                        .map(|secs| secs.min(cap_secs))
                })
        })
        .map(std::time::Duration::from_secs)
        .unwrap_or(backoff)
}

fn proxy_jitter(cfg: &crate::proxy::ProxyConfig) -> std::time::Duration {
    use rand::RngExt;
    let max_ms = cfg.retry_jitter.as_millis() as u64;
    if max_ms == 0 {
        return std::time::Duration::ZERO;
    }
    std::time::Duration::from_millis(rand::rng().random_range(0u64..max_ms))
}

fn is_retryable_proxy_status(status: rquest::StatusCode) -> bool {
    matches!(
        status,
        rquest::StatusCode::TOO_MANY_REQUESTS
            | rquest::StatusCode::BAD_GATEWAY
            | rquest::StatusCode::SERVICE_UNAVAILABLE
            | rquest::StatusCode::GATEWAY_TIMEOUT
    )
}

async fn host_semaphore(
    cache: &moka::future::Cache<String, Arc<tokio::sync::Semaphore>>,
    host: &str,
    concurrency: usize,
) -> Arc<tokio::sync::Semaphore> {
    cache
        .get_with(host.to_string(), async {
            Arc::new(tokio::sync::Semaphore::new(concurrency))
        })
        .await
}

/// Only correct on a path that reaches the network: a cache hit sends nothing
/// upstream and owes the host no delay. Serialises callers for `host` while waiting.
async fn throttle_host(state: &AppState, host: &str) {
    let min_host_interval = state.proxy_config.min_host_interval;
    let throttle_mutex = state
        .proxy_throttle
        .get_with(host.to_string(), async {
            Arc::new(tokio::sync::Mutex::new(
                std::time::Instant::now()
                    .checked_sub(min_host_interval)
                    .unwrap_or_else(std::time::Instant::now),
            ))
        })
        .await;

    let mut last = throttle_mutex.lock().await;
    let elapsed = last.elapsed();
    if elapsed < min_host_interval {
        tokio::time::sleep(min_host_interval - elapsed).await;
    }
    *last = std::time::Instant::now();
}

fn proxy_max_mem_bytes() -> usize {
    std::env::var("KANI_IMAGE_PROXY_MAX_MEMORY_MB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(128)
        * 1024
        * 1024
}

fn transcode_image_sync(
    bytes: &[u8],
    target_w: Option<u32>,
    format: Option<&str>,
    quality: u8,
    max_mem_bytes: usize,
) -> Result<(bytes::Bytes, &'static str), String> {
    use image::ImageReader;
    use std::io::Cursor;

    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(max_mem_bytes as u64);

    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("format guess failed: {e}"))?;
    reader.limits(limits);
    let img = reader.decode().map_err(|e| format!("decode failed: {e}"))?;

    let img = match target_w {
        Some(w) if img.width() > w => {
            img.resize(w, u32::MAX, image::imageops::FilterType::Lanczos3)
        }
        _ => img,
    };

    let pixel_mem = img.width() as usize * img.height() as usize * 4;
    if pixel_mem > max_mem_bytes {
        return Err(format!(
            "decoded image exceeds memory limit ({pixel_mem} > {max_mem_bytes})"
        ));
    }

    let (encoded, ct): (Vec<u8>, &'static str) = match format {
        Some("webp") => {
            let mut out = Vec::new();
            img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::WebP)
                .map_err(|e| format!("webp encode failed: {e}"))?;
            (out, "image/webp")
        }
        Some("png") => {
            let mut out = Vec::new();
            img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
                .map_err(|e| format!("png encode failed: {e}"))?;
            (out, "image/png")
        }
        _ => {
            let mut out = Vec::new();
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
            img.write_with_encoder(enc)
                .map_err(|e| format!("jpeg encode failed: {e}"))?;
            (out, "image/jpeg")
        }
    };

    Ok((bytes::Bytes::from(encoded), ct))
}

fn record_bandwidth(
    bandwidth: &dashmap::DashMap<String, Arc<std::sync::atomic::AtomicU64>>,
    host: &str,
    bytes: u64,
) {
    use std::sync::atomic::Ordering;
    bandwidth
        .entry(host.to_string())
        .or_insert_with(|| Arc::new(std::sync::atomic::AtomicU64::new(0)))
        .fetch_add(bytes, Ordering::Relaxed);
}

#[utoipa::path(
    get, path = "/rest/image_proxy",
    params(("token" = String, Query, description = "Sealed proxy token naming the upstream URL"), ("w" = Option<u32>, Query, description = "Target width in pixels, up to 4096"), ("transform" = Option<String>, Query, description = "Source-specific transform hint")),
    responses(
        (status = 200, description = "The upstream image, re-encoded and resized as requested"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 400, description = "Invalid or expired proxy token"),
        (status = 502, description = "The upstream image could not be fetched"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(crate) async fn image_proxy(
    _: AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
    headers: HeaderMap,
    ValidatedQuery(query): ValidatedQuery<ProxyQuery>,
) -> Result<impl IntoResponse, AppError> {
    let (url, referer) = crate::proxy::unseal_proxy_token(&query.token, &state.proxy_secret)
        .ok_or_else(|| AppError::Other("Invalid or expired proxy token".into()))?;

    let transform_hint = query.transform.as_deref().filter(|s| !s.is_empty());
    let target_w: Option<u32> = query.w.filter(|&w| w > 0 && w <= 4096);
    let req_format: Option<String> = query.format.as_deref().and_then(|f| match f {
        "jpeg" | "png" | "webp" => Some(f.to_string()),
        _ => None,
    });
    let quality: u8 = query.q.map(|q| q.clamp(1, 100)).unwrap_or(85);
    let needs_transcode = target_w.is_some() || req_format.is_some();

    let etag = crate::proxy::compute_etag(&url, &referer, &state.proxy_secret);

    if let Some(if_none_match) = headers.get(header::IF_NONE_MATCH)
        && if_none_match.as_bytes() == etag.as_bytes()
    {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("public, max-age=31536000, immutable"),
                ),
                (
                    header::ETAG,
                    header::HeaderValue::from_str(&etag)
                        .map_err(|e| AppError::InternalServerError(e.to_string()))?,
                ),
            ],
            Body::empty(),
        )
            .into_response());
    }

    let host = url::Url::parse(&url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| url.clone());

    if let Some(range) = headers.get(header::RANGE) {
        return proxy_range_request(&state, &url, &referer, &host, &etag, range).await;
    }

    let canonical = crate::proxy::canonical_proxy_key(&url);
    let cache_key = {
        let mut key = canonical;
        if let Some(t) = transform_hint {
            key = format!("{}|t:{}", key, t);
        }
        if needs_transcode {
            key = format!(
                "{}|w:{}|f:{}|q:{}",
                key,
                target_w.unwrap_or(0),
                req_format.as_deref().unwrap_or(""),
                quality
            );
        }
        key
    };

    let fetched: Arc<(bytes::Bytes, String)> = state
        .proxy_coalesce
        .try_get_with(cache_key, {
            let url = url.clone();
            let referer = referer.clone();
            let state = state.clone();
            let host = host.clone();
            let transform_hint = transform_hint.map(str::to_string);
            async move {
                let cfg = state.proxy_config;
                throttle_host(&state, &host).await;
                let semaphore =
                    host_semaphore(&state.proxy_semaphores, &host, cfg.per_host_concurrency).await;
                let mut attempt = 0u32;

                let (response, ct_string, resolved) = loop {
                    let permit = Arc::clone(&semaphore)
                        .acquire_owned()
                        .await
                        .map_err(|_| AppError::InternalServerError("Semaphore closed".into()))?;

                    let mut req_headers = rquest::header::HeaderMap::new();
                    req_headers.insert(
                        rquest::header::REFERER,
                        rquest::header::HeaderValue::from_str(&referer)
                            .map_err(AppError::InvalidHeaderValue)?,
                    );
                    req_headers.insert(
                        rquest::header::ACCEPT,
                        rquest::header::HeaderValue::from_static(
                            "image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
                        ),
                    );

                    let fetch = tokio::time::timeout(
                        cfg.request_timeout,
                        state.proxy_client.safe_get(&url, Some(req_headers)),
                    )
                    .await;

                    match fetch {
                        Err(_elapsed) => {
                            if attempt < cfg.max_retries {
                                let delay = proxy_retry_delay(None, attempt, &cfg) + proxy_jitter(&cfg);
                                tracing::warn!(
                                    "Upstream image timed out, retrying in {:?} (attempt {}/{})",
                                    delay,
                                    attempt + 1,
                                    cfg.max_retries,
                                );
                                drop(permit);
                                tokio::time::sleep(delay).await;
                                attempt += 1;
                                continue;
                            }
                            return Err(AppError::Other("Upstream image fetch timed out".into()));
                        }
                        Ok(Err(e)) => {
                            if attempt < cfg.max_retries {
                                let delay = proxy_retry_delay(None, attempt, &cfg) + proxy_jitter(&cfg);
                                tracing::warn!(
                                    "Upstream image fetch error ({}), retrying in {:?} (attempt {}/{})",
                                    e,
                                    delay,
                                    attempt + 1,
                                    cfg.max_retries,
                                );
                                drop(permit);
                                tokio::time::sleep(delay).await;
                                attempt += 1;
                                continue;
                            }
                            return Err(AppError::Other(format!(
                                "Upstream image fetch failed: {e}"
                            )));
                        }
                        Ok(Ok(resp)) => {
                            let status = resp.status();
                            if is_retryable_proxy_status(status) && attempt < cfg.max_retries {
                                let delay =
                                    proxy_retry_delay(Some(resp.headers()), attempt, &cfg) + proxy_jitter(&cfg);
                                tracing::warn!(
                                    "Upstream returned {}, retrying in {:?} (attempt {}/{})",
                                    status.as_u16(),
                                    delay,
                                    attempt + 1,
                                    cfg.max_retries,
                                );
                                drop(permit);
                                tokio::time::sleep(delay).await;
                                attempt += 1;
                                continue;
                            }
                            if !status.is_success() {
                                tracing::warn!(
                                    "Upstream returned {} for proxied request",
                                    status.as_u16()
                                );
                                return Err(AppError::Other(format!(
                                    "Upstream returned {}",
                                    status.as_u16()
                                )));
                            }

                            let content_type = resp
                                .headers()
                                .get(rquest::header::CONTENT_TYPE)
                                .cloned()
                                .ok_or_else(|| {
                                    AppError::InternalServerError(
                                        "Upstream response missing Content-Type".into(),
                                    )
                                })?;
                            let ct_str = content_type.to_str().unwrap_or("");
                            if ct_str.starts_with("text/html") {
                                tracing::warn!(
                                    "Upstream proxy returned an HTML page instead of an image: {}",
                                    ct_str
                                );
                                return Err(AppError::Other(format!(
                                    "Expected image, upstream returned Content-Type: {}",
                                    ct_str
                                )));
                            }
                            let ct_string = ct_str.to_string();
                            let resolved = transform_hint.as_deref().and_then(|hint| {
                                kani_core::transform::registry().resolve(
                                    hint,
                                    kani_core::transform::TransformKind::Image,
                                    resp.headers(),
                                )
                            });
                            drop(permit);
                            break (resp, ct_string, resolved);
                        }
                    }
                };

                let max_image_bytes = cfg.max_image_bytes;
                let mut buf = bytes::BytesMut::new();
                let mut resp = response;
                while let Ok(Some(chunk)) = resp.chunk().await {
                    if buf.len() + chunk.len() > max_image_bytes {
                        return Err(AppError::Other("Upstream image exceeded size limit".into()));
                    }
                    buf.extend_from_slice(&chunk);
                }

                // What the bytes actually are. A generic or wrong label is
                // corrected here; anything that is not an image at all is
                // refused, whatever the upstream called it.
                let sniffed = kani_core::probe::sniff_image_mime(&buf);
                let ct_string = match sniffed {
                    Some(mime) => mime.to_string(),
                    None if resolved.is_some() => ct_string,
                    None => {
                        tracing::warn!(
                            "Upstream proxy body is not an image (declared {})",
                            ct_string
                        );
                        return Err(AppError::Other(format!(
                            "Expected image, upstream returned Content-Type: {ct_string}"
                        )));
                    }
                };

                let (processed_bytes, processed_ct) = if let Some(r) = &resolved {
                    let out = r.output();
                    let descrambled = r
                        .apply(&buf)
                        .map_err(|e| AppError::InternalServerError(format!("Descramble failed: {e}")))?;
                    (bytes::Bytes::from(descrambled), out.content_type.to_string())
                } else {
                    (buf.freeze(), ct_string)
                };

                let (final_bytes, final_ct) = if needs_transcode {
                    let fmt = req_format.clone();
                    let max_mem = proxy_max_mem_bytes();
                    let input = processed_bytes.clone();
                    let (b, ct_str) = tokio::task::spawn_blocking(move || {
                        transcode_image_sync(&input, target_w, fmt.as_deref(), quality, max_mem)
                    })
                    .await
                    .map_err(|e| AppError::InternalServerError(format!("Transcode panicked: {e}")))?
                    .map_err(|e| AppError::Other(format!("Transcode failed: {e}")))?;
                    (b, ct_str.to_string())
                } else {
                    (processed_bytes, processed_ct)
                };

                Ok::<_, AppError>(Arc::new((final_bytes, final_ct)))
            }
        })
        .await
        .map_err(|e: Arc<AppError>| match Arc::try_unwrap(e) {
            Ok(err) => err,
            Err(arc) => AppError::InternalServerError(arc.to_string()),
        })?;

    record_bandwidth(&state.proxy_bandwidth, &host, fetched.0.len() as u64);

    let (body, ct_str): &(bytes::Bytes, String) = &fetched;
    let ct_value = header::HeaderValue::from_str(ct_str)
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    let etag_value = header::HeaderValue::from_str(&etag)
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, ct_value),
            (header::ETAG, etag_value),
            (
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                header::HeaderValue::from_static("nosniff"),
            ),
        ],
        Body::from(body.clone()),
    )
        .into_response())
}

async fn proxy_range_request(
    state: &AppState,
    url: &str,
    referer: &str,
    host: &str,
    etag: &str,
    range: &header::HeaderValue,
) -> Result<axum::response::Response, AppError> {
    throttle_host(state, host).await;

    let semaphore = host_semaphore(
        &state.proxy_semaphores,
        host,
        state.proxy_config.per_host_concurrency,
    )
    .await;
    let _permit = Arc::clone(&semaphore)
        .acquire_owned()
        .await
        .map_err(|_| AppError::InternalServerError("Semaphore closed".into()))?;

    let mut req_headers = rquest::header::HeaderMap::new();
    req_headers.insert(
        rquest::header::REFERER,
        rquest::header::HeaderValue::from_str(referer).map_err(AppError::InvalidHeaderValue)?,
    );
    req_headers.insert(
        rquest::header::ACCEPT,
        rquest::header::HeaderValue::from_static(
            "image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
        ),
    );
    req_headers.insert(
        rquest::header::RANGE,
        rquest::header::HeaderValue::from_bytes(range.as_bytes())
            .map_err(|e| AppError::InternalServerError(e.to_string()))?,
    );

    let resp = tokio::time::timeout(
        state.proxy_config.request_timeout,
        state.proxy_client.safe_get(url, Some(req_headers)),
    )
    .await
    .map_err(|_| AppError::Other("Upstream timed out on range request".into()))?
    .map_err(|e| AppError::Other(format!("Upstream range fetch failed: {e}")))?;

    let upstream_status = resp.status();
    if !upstream_status.is_success() {
        return Err(AppError::Other(format!(
            "Upstream returned {} for range request",
            upstream_status.as_u16()
        )));
    }

    let is_partial = upstream_status == rquest::StatusCode::PARTIAL_CONTENT;
    let out_headers = crate::proxy::build_range_response_headers(resp.headers(), etag);

    let max_range_bytes = state.proxy_config.max_image_bytes;
    let mut buf = bytes::BytesMut::new();
    let mut resp = resp;
    while let Ok(Some(chunk)) = resp.chunk().await {
        if buf.len() + chunk.len() > max_range_bytes {
            return Err(AppError::Other("Range response exceeded size limit".into()));
        }
        buf.extend_from_slice(&chunk);
    }

    let body_bytes = buf.freeze();
    record_bandwidth(&state.proxy_bandwidth, host, body_bytes.len() as u64);

    Ok((
        crate::proxy::range_response_status(is_partial),
        out_headers,
        Body::from(body_bytes),
    )
        .into_response())
}

#[derive(serde::Deserialize, Default)]
pub(crate) struct ChapterListQuery {
    pub(crate) sort: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct SaveToLibraryQuery {
    pub(crate) force: Option<bool>,
}

#[derive(serde::Deserialize, Default)]
pub(crate) struct CoverQuery {
    size: Option<String>,
    h: Option<String>,
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/cover",
    params(("id" = i64, Path, description = "Manga id"), ("size" = Option<String>, Query, description = "One of xs, sm, md, lg; full size when omitted")),
    responses(
        (status = 200, description = "The cover image, at the requested size"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 304, description = "Not modified"),
        (status = 404, description = "No cover stored for this manga"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(crate) async fn serve_manga_cover(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
    Path(id): Path<MangaId>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<CoverQuery>,
) -> Result<impl IntoResponse, AppError> {
    const VALID_SIZES: &[&str] = &["xs", "sm", "md", "lg"];

    if let Some(ref size) = query.size
        && VALID_SIZES.contains(&size.as_str())
    {
        if let Some((thumb_path, format, cover_hash)) =
            state.get_thumbnail_for_size(id, size).await?
        {
            let thumb_etag = format!("\"{cover_hash}-{size}-{format}\"");
            let hash_matches = query
                .h
                .as_deref()
                .map(|h| !h.is_empty() && cover_hash.starts_with(h))
                .unwrap_or(false);

            if let Some(inm) = headers.get(header::IF_NONE_MATCH)
                && inm.as_bytes() == thumb_etag.as_bytes()
            {
                let mut resp_headers = axum::http::HeaderMap::new();
                resp_headers.insert(
                    header::ETAG,
                    header::HeaderValue::from_str(&thumb_etag)
                        .map_err(|e| AppError::InternalServerError(e.to_string()))?,
                );
                if hash_matches {
                    resp_headers.insert(
                        header::CACHE_CONTROL,
                        header::HeaderValue::from_static("public, max-age=31536000, immutable"),
                    );
                }
                return Ok((
                    StatusCode::NOT_MODIFIED,
                    resp_headers,
                    axum::body::Body::empty(),
                )
                    .into_response());
            }

            let bytes = tokio::fs::read(&thumb_path)
                .await
                .map_err(AppError::IoError)?;

            let content_type: &'static str = match format.as_str() {
                "webp" => "image/webp",
                _ => "image/jpeg",
            };

            let mut resp_headers = axum::http::HeaderMap::new();
            resp_headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static(content_type),
            );
            resp_headers.insert(
                header::ETAG,
                header::HeaderValue::from_str(&thumb_etag)
                    .map_err(|e| AppError::InternalServerError(e.to_string()))?,
            );
            resp_headers.insert(
                header::CONTENT_LENGTH,
                header::HeaderValue::from(bytes.len()),
            );
            resp_headers.insert(
                header::X_CONTENT_TYPE_OPTIONS,
                header::HeaderValue::from_static("nosniff"),
            );
            if hash_matches {
                resp_headers.insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("public, max-age=31536000, immutable"),
                );
            } else {
                resp_headers.insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("public, max-age=3600"),
                );
            }

            return Ok(
                (StatusCode::OK, resp_headers, axum::body::Body::from(bytes)).into_response(),
            );
        }

        state.spawn_thumbnail_generation(id).await;
    }

    let full_path = state.get_manga_cover_path(id).await?;

    let metadata = tokio::fs::metadata(&full_path)
        .await
        .map_err(|_| AppError::NotFound("Cover file not found on disk".into()))?;

    let mtime = metadata
        .modified()
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        })
        .unwrap_or(0);
    let etag = format!("\"{}\"", mtime);

    if let Some(inm) = headers.get(header::IF_NONE_MATCH)
        && inm.as_bytes() == etag.as_bytes()
    {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("public, max-age=31536000, immutable"),
                ),
                (
                    header::ETAG,
                    header::HeaderValue::from_str(&etag)
                        .map_err(|e| AppError::InternalServerError(e.to_string()))?,
                ),
            ],
            axum::body::Body::empty(),
        )
            .into_response());
    }

    let ext = full_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    let content_type = match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/jpeg",
    };

    let bytes = tokio::fs::read(&full_path)
        .await
        .map_err(AppError::IoError)?;

    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                header::HeaderValue::from_static(content_type),
            ),
            (
                header::ETAG,
                header::HeaderValue::from_str(&etag)
                    .map_err(|e| AppError::InternalServerError(e.to_string()))?,
            ),
            (
                header::CONTENT_LENGTH,
                header::HeaderValue::from(bytes.len()),
            ),
            (
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                header::HeaderValue::from_static("nosniff"),
            ),
        ],
        axum::body::Body::from(bytes),
    )
        .into_response())
}

#[derive(serde::Deserialize)]
pub(crate) struct DownloadHistoryQuery {
    #[serde(default = "default_history_limit")]
    limit: i64,
}
fn default_history_limit() -> i64 {
    50
}

fn map_refresh_request(
    req: crate::models::RefreshMangaRequest,
) -> Result<kani_app::models::RefreshOptions, AppError> {
    let fields = match req.fields.as_deref() {
        None | Some([]) => kani_app::models::RefreshFields::default(),
        Some(names) => {
            let mut f = kani_app::models::RefreshFields {
                cover: false,
                title: false,
                description: false,
                status: false,
                people: false,
                tags: false,
            };
            for name in names {
                match name.as_str() {
                    "cover" => f.cover = true,
                    "title" => f.title = true,
                    "description" => f.description = true,
                    "status" => f.status = true,
                    "people" => f.people = true,
                    "tags" => f.tags = true,
                    other => {
                        return Err(AppError::ValidationError(format!(
                            "unknown refresh field: {other}"
                        )));
                    }
                }
            }
            f
        }
    };
    Ok(kani_app::models::RefreshOptions {
        fields,
        fetch_chapters: req.fetch_chapters.unwrap_or(true),
        clear_overrides: req.clear_overrides.unwrap_or(false),
    })
}

#[utoipa::path(
    get, path = "/rest/chapter/{id}/page/{page_num}",
    params(("id" = i64, Path, description = "Chapter id"), ("page_num" = i64, Path, description = "Zero-based page index")),
    responses(
        (status = 200, description = "One page image out of the downloaded CBZ"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 304, description = "Not modified"),
        (status = 404, description = "The chapter is not downloaded, or has no such page"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(crate) async fn serve_chapter_page(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
    Path((id, page_num)): Path<(ChapterId, usize)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    // Resolve the path first so we can derive an ETag from the CBZ mtime.
    // This avoids reading image bytes on cache hits.
    let info = state.chapter_cbz_path(id).await?;

    let mtime = tokio::fs::metadata(&info.path)
        .await
        .map(|m| {
            m.modified()
                .map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                })
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let etag = format!("\"{mtime}-{page_num}\"");

    if let Some(inm) = headers.get(header::IF_NONE_MATCH)
        && inm.as_bytes() == etag.as_bytes()
    {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("public, max-age=31536000, immutable"),
                ),
                (
                    header::ETAG,
                    header::HeaderValue::from_str(&etag)
                        .map_err(|e| AppError::InternalServerError(e.to_string()))?,
                ),
            ],
            axum::body::Body::empty(),
        )
            .into_response());
    }

    let (bytes, ext) = state.read_chapter_page(id, page_num).await?;

    let content_type = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        _ => "image/jpeg",
    };

    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                header::HeaderValue::from_static(content_type),
            ),
            (
                header::ETAG,
                header::HeaderValue::from_str(&etag)
                    .map_err(|e| AppError::InternalServerError(e.to_string()))?,
            ),
            (
                header::CONTENT_LENGTH,
                header::HeaderValue::from(bytes.len()),
            ),
            (
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                header::HeaderValue::from_static("nosniff"),
            ),
        ],
        axum::body::Body::from(bytes),
    )
        .into_response())
}

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status": "ok"})))
}

pub async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => (StatusCode::OK, Json(json!({"status": "ready"}))).into_response(),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "not_ready",
                "reason": format!("database: {e}")
            })),
        )
            .into_response(),
    }
}

const OAUTH_SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html>
<head><title>Account Linked</title></head>
<body>
<script>
  if (window.opener) {
    window.opener.postMessage({ type: 'tracker_linked' }, window.location.origin);
    window.close();
  } else {
    document.body.innerText = 'Account linked successfully. You can close this window.';
  }
</script>
</body>
</html>"#;

#[derive(serde::Deserialize)]
pub(crate) struct UserActivityQuery {
    pub(crate) limit: Option<i64>,
    pub(crate) before: Option<String>,
}

#[derive(serde::Serialize)]
struct ActivityEvent {
    id: i64,
    action: String,
    target: Option<String>,
    details: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct UserActivityResponse {
    events: Vec<ActivityEvent>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use axum_login::{
        AuthManagerLayerBuilder,
        tower_sessions::{SessionManagerLayer, cookie::SameSite},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use tower_sessions_sqlx_store::SqliteStore;

    async fn test_app_state(pool: sqlx::SqlitePool) -> AppState {
        AppState::new_for_test(pool).await
    }

    /// Builds a full test router with an in-memory DB, a default test user, and
    /// the auth + session layers wired up — matching the production setup in main.rs
    /// but without rate-limiting or CORS.
    ///
    /// Returns `(router, username, password)`.
    async fn test_router() -> (axum::Router, String, String) {
        let pool = crate::auth::test_db().await;
        let backend = crate::auth::AuthBackend::new(pool.clone());
        backend
            .create_user("alice", "alice@test.com", "hunter2")
            .await
            .expect("create test user");

        let session_store = SqliteStore::new(pool.clone());
        session_store.migrate().await.expect("session migrate");

        let session_layer = SessionManagerLayer::new(session_store)
            .with_secure(false)
            .with_http_only(true)
            .with_same_site(SameSite::Lax);

        let auth_layer = AuthManagerLayerBuilder::new(
            crate::auth::AuthBackend::new(pool.clone()),
            session_layer,
        )
        .build();

        let state = test_app_state(pool).await;
        let router = routes(state).layer(auth_layer);
        (router, "alice".to_string(), "hunter2".to_string())
    }

    fn login_request(username: &str, password: &str) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri("/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"username": username, "password": password}).to_string(),
            ))
            .unwrap()
    }

    async fn body_json(body: Body) -> serde_json::Value {
        let bytes = body.collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Log in and return the raw `id=value` portion of the Set-Cookie header.
    async fn login_and_get_cookie(app: axum::Router, username: &str, password: &str) -> String {
        let resp = app
            .oneshot(login_request(username, password))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "expected login to succeed");
        resp.headers()
            .get("set-cookie")
            .expect("Set-Cookie header")
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn login_valid_credentials_returns_200() {
        let (app, user, pass) = test_router().await;
        let resp = app.oneshot(login_request(&user, &pass)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn login_sets_session_cookie() {
        let (app, user, pass) = test_router().await;
        let resp = app.oneshot(login_request(&user, &pass)).await.unwrap();
        assert!(resp.headers().contains_key("set-cookie"));
    }

    #[tokio::test]
    async fn login_response_body_has_ok_true() {
        let (app, user, pass) = test_router().await;
        let resp = app.oneshot(login_request(&user, &pass)).await.unwrap();
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["ok"], true);
    }

    #[tokio::test]
    async fn login_wrong_password_returns_401() {
        let (app, user, _) = test_router().await;
        let resp = app
            .oneshot(login_request(&user, "wrongpass"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_unknown_user_returns_401() {
        let (app, _, _) = test_router().await;
        let resp = app
            .oneshot(login_request("nobody", "whatever"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_failure_body_has_error_field() {
        let (app, user, _) = test_router().await;
        let resp = app.oneshot(login_request(&user, "bad")).await.unwrap();
        let json = body_json(resp.into_body()).await;
        assert!(json["error"].is_string());
    }

    #[tokio::test]
    async fn auth_me_without_session_returns_401() {
        let (app, _, _) = test_router().await;
        let req = axum::http::Request::builder()
            .uri("/auth/me")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_me_with_session_returns_200() {
        let (app, user, pass) = test_router().await;
        let cookie = login_and_get_cookie(app.clone(), &user, &pass).await;

        let req = axum::http::Request::builder()
            .uri("/auth/me")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_me_response_contains_username() {
        let (app, user, pass) = test_router().await;
        let cookie = login_and_get_cookie(app.clone(), &user, &pass).await;

        let req = axum::http::Request::builder()
            .uri("/auth/me")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["username"], user.as_str());
    }

    #[tokio::test]
    async fn logout_returns_200() {
        let (app, user, pass) = test_router().await;
        let cookie = login_and_get_cookie(app.clone(), &user, &pass).await;

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/auth/logout")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn after_logout_old_session_is_rejected() {
        let (app, user, pass) = test_router().await;
        let cookie = login_and_get_cookie(app.clone(), &user, &pass).await;

        let logout = axum::http::Request::builder()
            .method("POST")
            .uri("/auth/logout")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(logout).await.unwrap();

        let req = axum::http::Request::builder()
            .uri("/auth/me")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn boot_id_with_session_returns_expected_value() {
        let (app, user, pass) = test_router().await;
        let cookie = login_and_get_cookie(app.clone(), &user, &pass).await;

        let req = axum::http::Request::builder()
            .uri("/boot_id")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp.into_body()).await;
        assert_eq!(json["boot_id"], "test-boot-id");
    }

    async fn assert_401_without_session(uri: &'static str) {
        let (app, _, _) = test_router().await;
        let req = axum::http::Request::builder()
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "expected 401 for unauthenticated GET {uri}"
        );
    }

    #[tokio::test]
    async fn sources_unauthenticated_returns_401() {
        assert_401_without_session("/sources").await;
    }

    #[tokio::test]
    async fn library_unauthenticated_returns_401() {
        assert_401_without_session("/library").await;
    }

    #[tokio::test]
    async fn settings_unauthenticated_returns_401() {
        assert_401_without_session("/settings").await;
    }

    #[tokio::test]
    async fn categories_unauthenticated_returns_401() {
        assert_401_without_session("/categories").await;
    }

    #[tokio::test]
    async fn recent_updates_unauthenticated_returns_401() {
        assert_401_without_session("/recent_updates").await;
    }

    #[tokio::test]
    async fn global_search_unauthenticated_returns_401() {
        assert_401_without_session("/global_search").await;
    }

    #[tokio::test]
    async fn boot_id_unauthenticated_returns_401() {
        assert_401_without_session("/boot_id").await;
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct LibraryBackupQuery {
    pub(crate) include_chapter_progress: Option<bool>,
}

#[derive(serde::Deserialize)]
pub(crate) struct ResolvePendingImportBody {
    pub(crate) source_id: i64,
    pub(crate) source_manga_id: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct DismissDuplicatePath {
    pub(crate) a: MangaId,
    pub(crate) b: MangaId,
}

#[derive(serde::Deserialize)]
pub(crate) struct MergeDuplicateBody {
    pub(crate) keep_id: i64,
    pub(crate) discard_id: i64,
}

async fn collect_file_field(
    multipart: &mut Multipart,
    limit: usize,
) -> Result<bytes::Bytes, AppError> {
    let field = multipart
        .next_field()
        .await?
        .ok_or_else(|| AppError::ValidationError("No file field in upload".into()))?;

    let content_length = field
        .headers()
        .get(rquest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());

    Ok(kani_core::http::collect_bytes_limited(
        Box::pin(field.map_err(|e| kani_core::error::Error::Other(e.to_string()))),
        content_length,
        limit,
    )
    .await?)
}

fn parse_csv(s: Option<&str>) -> Vec<String> {
    match s {
        Some(v) if !v.is_empty() => v.split(',').map(|p| p.trim().to_string()).collect(),
        _ => vec![],
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[derive(serde::Deserialize, Default)]
pub(crate) struct ExportQuery {
    pub(crate) profile: Option<String>,
}

#[derive(serde::Deserialize, Default)]
pub(crate) struct KccExportQuery {
    pub(crate) profile: Option<String>,
    pub(crate) format: Option<String>,
    pub(crate) manga: Option<bool>,
}
