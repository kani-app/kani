use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use wasmtime::Store;

use kani_core::{
    error::Result,
    wasm::{AllowedHost, HostState, KaniExtensionPre},
};

pub struct WasmSource {
    engine: wasmtime::Engine,
    instance_pre: KaniExtensionPre<HostState>,
    semaphore: Arc<Semaphore>,
    smart_client: kani_core::http::SmartClient,
    base_url: Option<String>,
    unrestricted_http: bool,
    browser_enabled: Arc<AtomicBool>,
    preferences: Arc<std::sync::RwLock<std::collections::HashMap<String, String>>>,
    v8_process: kani_core::v8_process::V8ProcessHandle,
    ext_cache: Arc<dyn kani_core::cache::CacheBackend>,
    ext_cache_namespace: String,
    pure_fn_registry: Option<Arc<kani_core::scripting::PureFunctionRegistry>>,
    hook_registry: Option<Arc<kani_core::scripting::HookRegistry>>,
    limits: kani_core::budget::SourceLimits,
    lease: Arc<kani_lease::LeaseCoordinator>,
}

impl WasmSource {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: wasmtime::Engine,
        instance_pre: KaniExtensionPre<HostState>,
        smart_client: kani_core::http::SmartClient,
        base_url: Option<String>,
        unrestricted_http: bool,
        browser_enabled: bool,
        max_concurrent: usize,
        preferences: std::collections::HashMap<String, String>,
        ext_cache: Arc<dyn kani_core::cache::CacheBackend>,
        ext_cache_namespace: String,
        pure_fn_registry: Option<Arc<kani_core::scripting::PureFunctionRegistry>>,
        hook_registry: Option<Arc<kani_core::scripting::HookRegistry>>,
        limits: impl Into<kani_core::budget::SourceLimits>,
    ) -> Self {
        Self {
            engine,
            instance_pre,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            smart_client,
            base_url,
            unrestricted_http,
            browser_enabled: Arc::new(AtomicBool::new(browser_enabled)),
            preferences: Arc::new(std::sync::RwLock::new(preferences)),
            v8_process: kani_core::v8_process::new_handle(),
            ext_cache,
            ext_cache_namespace,
            pure_fn_registry,
            hook_registry,
            limits: limits.into(),
            lease: Arc::new(kani_lease::LeaseCoordinator::new()),
        }
    }

    pub fn extension_id(&self) -> &str {
        self.ext_cache_namespace.trim_end_matches(':')
    }

    pub fn update_preferences(&self, prefs: std::collections::HashMap<String, String>) {
        if let Ok(mut lock) = self.preferences.write() {
            *lock = prefs;
        }
    }

    /// `idle_for` governs the local V8 worker only; solver sessions expire on the
    /// solver's own TTL, so the host has nothing to configure for them.
    pub(crate) async fn reap_idle_v8(&self, idle_for: std::time::Duration) -> bool {
        let (local, solver) = tokio::join!(
            kani_core::v8_process::reap_if_idle(&self.v8_process, idle_for),
            self.smart_client
                .reap_solver_sessions(self.extension_id(), kani_core::http::solver_session_ttl())
        );
        local || solver > 0
    }

    pub(crate) async fn shutdown_v8(&self, reason: &str) -> bool {
        let (local, solver) = tokio::join!(
            kani_core::v8_process::shutdown(&self.v8_process, reason),
            self.smart_client
                .destroy_solver_sessions(self.extension_id())
        );
        local || solver > 0
    }

    pub(crate) async fn retire_v8(&self, reason: &str) -> bool {
        let (local, solver) = tokio::join!(
            kani_core::v8_process::retire(&self.v8_process, reason),
            self.smart_client
                .destroy_solver_sessions(self.extension_id())
        );
        local || solver > 0
    }

    pub fn set_browser_enabled(&self, enabled: bool) {
        self.browser_enabled.store(enabled, Ordering::Relaxed);
    }

    pub async fn lease_instance(&self) -> Result<OwnedSourceInstance> {
        // Lease/drain coordination is a single-word CAS in the kani-lease crate
        // (loom-verified). try_acquire fails while the source is draining.
        if !self.lease.try_acquire() {
            return Err(kani_core::error::Error::Extension(
                kani_shared::extension::ExtensionError::source_updating(),
            ));
        }

        // Created immediately after a successful acquire so any `?` below releases
        // the lease on the way out.
        let lease_guard = LeaseGuard(Arc::clone(&self.lease));

        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| kani_core::error::Error::Internal("Semaphore closed".into()))?;

        let allowed_host = match (self.base_url.as_deref(), self.unrestricted_http) {
            (_, true) => AllowedHost::Unrestricted,
            (Some(url), false) => AllowedHost::Restricted(url.to_string()),
            (None, false) => AllowedHost::MetadataOnly,
        };

        let mut host_state = HostState::new(
            self.smart_client.clone(),
            allowed_host,
            Arc::clone(&self.ext_cache),
            self.ext_cache_namespace.clone(),
            Arc::clone(&self.v8_process),
        )?;
        host_state.pure_fn_registry = self.pure_fn_registry.clone();
        host_state.hook_registry = self.hook_registry.clone();
        host_state.max_hook_requests = self.limits.max_hook_requests;
        host_state.operation_limits = self.limits.operation;
        host_state.browser_enabled = self.browser_enabled.load(Ordering::Relaxed);
        let mut store = Store::new(&self.engine, host_state);

        store.set_epoch_deadline(kani_core::sources::EPOCH_DEADLINE_TICKS);
        store.epoch_deadline_callback(|ctx| {
            let data = ctx.data();
            if data
                .last_io_at
                .map(|t| {
                    t.elapsed().as_millis()
                        < (kani_core::sources::EPOCH_DEADLINE_TICKS as u128 * 10)
                })
                .unwrap_or(false)
            {
                Ok(wasmtime::UpdateDeadline::Continue(
                    kani_core::sources::EPOCH_DEADLINE_TICKS,
                ))
            } else {
                Err(wasmtime::Error::msg("WASM computation deadline exceeded"))
            }
        });

        {
            let prefs = self.preferences.read().unwrap_or_else(|e| e.into_inner());
            store.data_mut().preferences = prefs.clone();
        }

        let bindings = self.instance_pre.instantiate_async(&mut store).await?;

        Ok(OwnedSourceInstance {
            store,
            bindings,
            _permit: permit,
            _lease_guard: lease_guard,
        })
    }

    pub async fn drain(&self, timeout: std::time::Duration) {
        self.lease.start_drain();
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self.lease.active() == 0 {
                break;
            }
            if std::time::Instant::now() >= deadline {
                tracing::warn!(
                    "WasmSource drain timed out after {:?}, forcing swap",
                    timeout
                );
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    pub(super) async fn fetch_page_list_impl(
        &self,
        manga_id: &str,
        chapter_id: &str,
    ) -> Result<(kani_core::wasm::kani::extension::types::Chapter, String)> {
        let mut attempts = 0u32;
        loop {
            let result = async {
                let mut instance = self.lease_instance().await?;
                let pages = instance.get_pages(manga_id, chapter_id).await?;
                let raw_metadata = instance.get_metadata().await?;
                let metadata: kani_shared::ExtensionMetadata = serde_json::from_str(&raw_metadata)?;
                Ok::<_, kani_core::error::Error>((pages, metadata.base_url))
            }
            .await;

            let err = match result {
                Ok(data) => return Ok(data),
                Err(e) => e,
            };

            let retry = if let kani_core::error::Error::Extension(ref ext_err) = err {
                kani_core::downloader::ext_retry_params(ext_err.kind)
            } else {
                None
            };

            match retry {
                Some((max_attempts, delay_ms)) if attempts < max_attempts => {
                    attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                _ => return Err(err),
            }
        }
    }
}

struct LeaseGuard(Arc<kani_lease::LeaseCoordinator>);

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        self.0.release();
    }
}

pub struct OwnedSourceInstance {
    store: Store<HostState>,
    bindings: kani_core::wasm::KaniExtension,
    _permit: OwnedSemaphorePermit,
    _lease_guard: LeaseGuard,
}

impl OwnedSourceInstance {
    pub fn set_preference_map(&mut self, prefs: std::collections::HashMap<String, String>) {
        self.store.data_mut().preferences = prefs;
    }

    pub async fn get_popular_manga(
        &mut self,
        page: i32,
        page_size: i32,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> Result<kani_core::wasm::kani::extension::types::MangaList> {
        let wit_filters = kani_core::wasm::filter_conversions::to_wit_active_filters(filters);
        kani_core::execute_wasm!(self, call_get_popular_manga, page, page_size, &wit_filters)
    }

    pub async fn search_manga(
        &mut self,
        query: &str,
        page: i32,
        page_size: i32,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> Result<kani_core::wasm::kani::extension::types::MangaList> {
        let wit_filters = kani_core::wasm::filter_conversions::to_wit_active_filters(filters);
        kani_core::execute_wasm!(
            self,
            call_search_manga,
            query,
            page,
            page_size,
            &wit_filters
        )
    }

    pub async fn get_manga_details(
        &mut self,
        manga_id: &str,
    ) -> Result<kani_core::wasm::kani::extension::types::MangaInfo> {
        kani_core::execute_wasm!(self, call_get_manga_details, manga_id)
    }

    pub async fn get_chapter_list(
        &mut self,
        manga_id: &str,
        page: i32,
        page_size: Option<i32>,
        sort: Option<String>,
    ) -> Result<kani_core::wasm::kani::extension::types::ChapterList> {
        kani_core::execute_wasm!(
            self,
            call_get_chapter_list,
            manga_id,
            page,
            page_size,
            sort.as_deref()
        )
    }

    pub async fn get_chapter_sort_list(
        &mut self,
    ) -> Result<Vec<kani_core::wasm::kani::extension::types::SortOption>> {
        kani_core::execute_wasm!(self, call_get_chapter_sort_list)
    }

    pub async fn get_pages(
        &mut self,
        manga_id: &str,
        chapter_id: &str,
    ) -> Result<kani_core::wasm::kani::extension::types::Chapter> {
        kani_core::execute_wasm!(self, call_get_pages, manga_id, chapter_id)
    }

    pub async fn get_fetched_option_sets(&mut self) -> Result<String> {
        kani_core::execute_wasm!(self, call_get_fetched_option_sets)
    }

    pub async fn get_metadata(&mut self) -> Result<String> {
        kani_core::execute_wasm!(self, call_get_metadata)
    }

    pub async fn get_filter_list(
        &mut self,
    ) -> Result<kani_core::wasm::kani::extension::types::FilterList> {
        kani_core::execute_wasm!(self, call_get_filter_list)
    }

    pub async fn get_preferences(
        &mut self,
    ) -> Result<Vec<kani_core::wasm::kani::extension::types::PreferenceSpec>> {
        kani_core::execute_wasm!(self, call_get_preferences)
    }

    pub async fn get_url(&mut self, manga_id: &str) -> Result<String> {
        kani_core::execute_wasm!(self, call_get_url, manga_id)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::LeaseGuard;
    use kani_lease::LeaseCoordinator;
    use std::sync::Arc;

    #[test]
    fn a_lease_guard_releases_on_drop() {
        let coord = Arc::new(LeaseCoordinator::new());
        assert!(coord.try_acquire());
        {
            let _guard = LeaseGuard(Arc::clone(&coord));
            assert_eq!(coord.active(), 1);
        }
        assert_eq!(coord.active(), 0);
    }

    #[test]
    fn a_lease_is_refused_once_draining() {
        let coord = LeaseCoordinator::new();
        assert!(coord.try_acquire());
        coord.release();
        coord.start_drain();
        assert!(!coord.try_acquire(), "no lease after draining begins");
        assert_eq!(
            coord.active(),
            0,
            "a refused lease leaves the count at zero"
        );
    }
}
