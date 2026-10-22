use crate::error::AppError;
use crate::logging::LogHandle;
use crate::rate_limit::AuthRateLimiter;
use bytes::Bytes;
use dashmap::DashMap;
use kani_app::AppService;
use kani_app::service::traits::{
    CategoryDomain, ChapterDomain, DownloadDomain, JobDomain, LibraryDomain, MangaDomain,
    ScanlatorDomain, SettingsDomain, SourceDomain, TrackerDomain,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

/// Helper that generates a random [u8; 32] for ephemeral per-process secrets.
fn random_secret() -> [u8; 32] {
    rand::random()
}

pub type Result<T, E = AppError> = std::result::Result<T, E>;

/// Web-layer application state.
///
/// Business logic lives in [`AppService`] (kani-app). This struct adds the
/// HTTP-specific extras — proxy cryptography, per-host rate-limit semaphores,
/// a boot-time identifier used by the SSE reconnect protocol, and the
/// in-memory log ring buffer handle.
/// How long a written `user_sessions.last_seen_at` suppresses the next write.
/// Bounds session-list staleness; below it, requests skip the writer pool.
pub const SESSION_TOUCH_WINDOW: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<AppService>,
    pub proxy_secret: Arc<[u8; 32]>,
    pub proxy_semaphores: moka::future::Cache<String, Arc<tokio::sync::Semaphore>>,
    pub proxy_throttle: moka::future::Cache<String, Arc<tokio::sync::Mutex<std::time::Instant>>>,
    pub proxy_coalesce: moka::future::Cache<String, Arc<(Bytes, String)>>,
    pub proxy_bandwidth: Arc<DashMap<String, Arc<AtomicU64>>>,
    pub proxy_config: crate::proxy::ProxyConfig,
    pub boot_id: String,
    /// Set to `true` by the restart handler; causes `main` to exit with code 42
    /// instead of 0, so an entrypoint wrapper can restart the process.
    pub restart_requested: Arc<AtomicBool>,
    pub log_handle: Arc<LogHandle>,
    /// Per-identity and per-IP login-attempt rate limiter.
    pub rate_limiter: Arc<AuthRateLimiter>,
    /// Ephemeral per-process key for signing CSRF double-submit tokens.
    pub csrf_secret: Arc<[u8; 32]>,
    /// Whether `KANI_PUBLIC_INSTANCE=true` is set, exposed to the UI as a feature flag.
    pub public_instance: bool,
    /// Records responses to writes carrying an `Idempotency-Key`, so a client's
    /// retry replays the original result instead of repeating the write.
    pub idempotency: crate::idempotency::IdempotencyStore,
    /// Sessions whose sidecar row was written within [`SESSION_TOUCH_WINDOW`].
    pub session_touch_seen: moka::future::Cache<String, ()>,
    /// Networks whose `X-Forwarded-For` is believed. Empty unless `KANI_TRUSTED_PROXIES` is set,
    /// so a direct deployment cannot have its client address forged.
    pub trusted_proxies: Arc<crate::client_ip::TrustedProxies>,
}

impl AppState {
    pub async fn new(log_handle: Arc<LogHandle>, data_dir: std::path::PathBuf) -> Result<Self> {
        let service = Arc::new(AppService::new(&data_dir).await?);
        let public_instance = std::env::var("KANI_PUBLIC_INSTANCE")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let (trusted_proxies, rejected) = crate::client_ip::TrustedProxies::from_env();
        for entry in &rejected {
            tracing::warn!("KANI_TRUSTED_PROXIES: ignoring unparseable entry {entry:?}");
        }
        if !trusted_proxies.is_empty() {
            tracing::info!(
                "trusting X-Forwarded-For from {} configured network(s)",
                trusted_proxies.len()
            );
        }

        Ok(Self {
            proxy_secret: Arc::new(crate::proxy::load_or_persist_secret(&data_dir)),
            proxy_semaphores: moka::future::Cache::builder()
                .max_capacity(1_000)
                .time_to_idle(std::time::Duration::from_secs(3_600))
                .build(),
            proxy_throttle: moka::future::Cache::builder()
                .max_capacity(1_000)
                .time_to_idle(std::time::Duration::from_secs(3_600))
                .build(),
            proxy_coalesce: moka::future::Cache::builder()
                .max_capacity(50 * 1024 * 1024)
                .time_to_live(std::time::Duration::from_secs(30))
                .weigher(|_k, v: &Arc<(Bytes, String)>| v.0.len().min(u32::MAX as usize) as u32)
                .build(),
            proxy_bandwidth: Arc::new(DashMap::new()),
            proxy_config: crate::proxy::ProxyConfig::default(),
            boot_id: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .to_string(),
            restart_requested: Arc::new(AtomicBool::new(false)),
            log_handle,
            rate_limiter: Arc::new(AuthRateLimiter::new(
                service.db.clone(),
                service.settings.clone(),
            )),
            csrf_secret: Arc::new(random_secret()),
            public_instance,
            idempotency: crate::idempotency::IdempotencyStore::new(),
            session_touch_seen: moka::future::Cache::builder()
                .time_to_live(SESSION_TOUCH_WINDOW)
                .build(),
            trusted_proxies: Arc::new(trusted_proxies),
            service,
        })
    }
}

impl AppState {
    /// Spawns a daily task to prune expired login attempts from the database.
    pub fn spawn_login_attempt_prune(&self) {
        let limiter = self.rate_limiter.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                limiter.prune_old_attempts().await;
            }
        });
    }
}

/// Allows REST handlers to call `state.method()` without `state.service.method()`.
impl std::ops::Deref for AppState {
    type Target = AppService;
    fn deref(&self) -> &Self::Target {
        &self.service
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn SourceDomain> {
    fn from_ref(s: &AppState) -> Self {
        s.service.clone() as Arc<dyn SourceDomain>
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn DownloadDomain> {
    fn from_ref(s: &AppState) -> Self {
        s.service.clone() as Arc<dyn DownloadDomain>
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn ChapterDomain> {
    fn from_ref(s: &AppState) -> Self {
        s.service.clone() as Arc<dyn ChapterDomain>
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn LibraryDomain> {
    fn from_ref(s: &AppState) -> Self {
        s.service.clone() as Arc<dyn LibraryDomain>
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn MangaDomain> {
    fn from_ref(s: &AppState) -> Self {
        s.service.clone() as Arc<dyn MangaDomain>
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn TrackerDomain> {
    fn from_ref(s: &AppState) -> Self {
        s.service.clone() as Arc<dyn TrackerDomain>
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn CategoryDomain> {
    fn from_ref(s: &AppState) -> Self {
        s.service.clone() as Arc<dyn CategoryDomain>
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn ScanlatorDomain> {
    fn from_ref(s: &AppState) -> Self {
        s.service.clone() as Arc<dyn ScanlatorDomain>
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn SettingsDomain> {
    fn from_ref(s: &AppState) -> Self {
        s.service.clone() as Arc<dyn SettingsDomain>
    }
}

impl axum::extract::FromRef<AppState> for Arc<dyn JobDomain> {
    fn from_ref(s: &AppState) -> Self {
        s.service.clone() as Arc<dyn JobDomain>
    }
}

pub use kani_app::chapter_name;

#[cfg(any(test, feature = "test-util"))]
impl AppState {
    pub async fn new_for_test(pool: sqlx::SqlitePool) -> Self {
        let service = Arc::new(AppService::new_for_test(pool.clone()).await);
        let (_, log_handle) = crate::logging::RingBufferLayer::new(100);
        let rate_limiter = Arc::new(AuthRateLimiter::new(pool, service.settings.clone()));
        Self {
            rate_limiter,
            csrf_secret: Arc::new([0u8; 32]),
            public_instance: false,
            service,
            proxy_secret: Arc::new([0u8; 32]),
            proxy_semaphores: moka::future::Cache::builder().max_capacity(100).build(),
            proxy_throttle: moka::future::Cache::builder().max_capacity(100).build(),
            proxy_coalesce: moka::future::Cache::builder().max_capacity(100).build(),
            proxy_bandwidth: Arc::new(DashMap::new()),
            proxy_config: crate::proxy::ProxyConfig::default(),
            boot_id: "test-boot-id".to_string(),
            restart_requested: Arc::new(AtomicBool::new(false)),
            log_handle,
            idempotency: crate::idempotency::IdempotencyStore::new(),
            session_touch_seen: moka::future::Cache::builder()
                .time_to_live(SESSION_TOUCH_WINDOW)
                .build(),
            trusted_proxies: Arc::new(crate::client_ip::TrustedProxies::default()),
        }
    }
}
