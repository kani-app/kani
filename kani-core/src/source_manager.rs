use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use wasmtime::Store;

use crate::error::Result;
use crate::execute_wasm;
use crate::wasm::{AllowedHost, HostState, KaniExtensionPre};

/// Manages concurrent access to WASM source extensions via InstancePre.
pub(crate) struct SourceManager {
    engine: wasmtime::Engine,
    instance_pre: KaniExtensionPre<HostState>,
    semaphore: Arc<Semaphore>,
    smart_client: crate::http::SmartClient,
    base_url: Option<String>,
    unrestricted_http: bool,
    preferences: Arc<std::sync::RwLock<std::collections::HashMap<String, String>>>,
    v8_process: crate::v8_process::V8ProcessHandle,
    ext_cache: Arc<dyn crate::cache::CacheBackend>,
    ext_cache_namespace: String,
    pure_fn_registry: Option<Arc<crate::scripting::PureFunctionRegistry>>,
    hook_registry: Option<Arc<crate::scripting::HookRegistry>>,
    limits: crate::budget::SourceLimits,
}

impl SourceManager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: wasmtime::Engine,
        instance_pre: KaniExtensionPre<HostState>,
        smart_client: crate::http::SmartClient,
        base_url: Option<String>,
        unrestricted_http: bool,
        max_concurrent: usize,
        preferences: std::collections::HashMap<String, String>,
        ext_cache: Arc<dyn crate::cache::CacheBackend>,
        ext_cache_namespace: String,
        pure_fn_registry: Option<Arc<crate::scripting::PureFunctionRegistry>>,
        hook_registry: Option<Arc<crate::scripting::HookRegistry>>,
        limits: impl Into<crate::budget::SourceLimits>,
    ) -> Self {
        Self {
            engine,
            instance_pre,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            smart_client,
            base_url,
            unrestricted_http,
            preferences: Arc::new(std::sync::RwLock::new(preferences)),
            v8_process: crate::v8_process::new_handle(),
            ext_cache,
            ext_cache_namespace,
            pure_fn_registry,
            hook_registry,
            limits: limits.into(),
        }
    }

    pub fn update_preferences(&self, prefs: std::collections::HashMap<String, String>) {
        if let Ok(mut lock) = self.preferences.write() {
            *lock = prefs;
        }
    }

    pub async fn lease_instance(&self) -> Result<OwnedSourceInstance> {
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| crate::error::Error::Internal("Semaphore closed".into()))?;

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
        let mut store = Store::try_new(&self.engine, host_state)?;

        store.set_epoch_deadline(crate::sources::EPOCH_DEADLINE_TICKS);
        store.epoch_deadline_callback(|ctx| {
            let data = ctx.data();
            if data
                .last_io_at
                .map(|t| {
                    t.elapsed().as_millis() < (crate::sources::EPOCH_DEADLINE_TICKS as u128 * 10)
                })
                .unwrap_or(false)
            {
                Ok(wasmtime::UpdateDeadline::Continue(
                    crate::sources::EPOCH_DEADLINE_TICKS,
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
        })
    }
}

/// A leased WASM source instance. Dropping it releases the concurrency permit.
pub struct OwnedSourceInstance {
    store: Store<HostState>,
    bindings: crate::wasm::KaniExtension,
    _permit: OwnedSemaphorePermit,
}

impl OwnedSourceInstance {
    pub fn set_preference_map(&mut self, prefs: std::collections::HashMap<String, String>) {
        self.store.data_mut().preferences = prefs;
    }

    /// Calls the `get_popular_manga` function in the WASM module.
    pub async fn get_popular_manga(
        &mut self,
        page: i32,
        page_size: i32,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> Result<crate::wasm::kani::extension::types::MangaList> {
        let wit_filters = crate::wasm::filter_conversions::to_wit_active_filters(filters);
        execute_wasm!(self, call_get_popular_manga, page, page_size, &wit_filters)
    }

    /// Calls the `search_manga` function in the WASM module.
    pub async fn search_manga(
        &mut self,
        query: &str,
        page: i32,
        page_size: i32,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> Result<crate::wasm::kani::extension::types::MangaList> {
        let wit_filters = crate::wasm::filter_conversions::to_wit_active_filters(filters);
        execute_wasm!(
            self,
            call_search_manga,
            query,
            page,
            page_size,
            &wit_filters
        )
    }

    /// Calls the `get_manga_details` function in the WASM module.
    pub async fn get_manga_details(
        &mut self,
        manga_id: &str,
    ) -> Result<crate::wasm::kani::extension::types::MangaInfo> {
        execute_wasm!(self, call_get_manga_details, manga_id)
    }

    /// Calls the `get_chapter_list` function in the WASM module.
    pub async fn get_chapter_list(
        &mut self,
        manga_id: &str,
        page: i32,
        page_size: Option<i32>,
        sort: Option<String>,
    ) -> Result<crate::wasm::kani::extension::types::ChapterList> {
        execute_wasm!(
            self,
            call_get_chapter_list,
            manga_id,
            page,
            page_size,
            sort.as_deref()
        )
    }

    /// Calls the `get_chapter_sort_list` function in the WASM module.
    pub async fn get_chapter_sort_list(
        &mut self,
    ) -> Result<Vec<crate::wasm::kani::extension::types::SortOption>> {
        execute_wasm!(self, call_get_chapter_sort_list)
    }

    /// Calls the `get_pages` function in the WASM module.
    pub async fn get_pages(
        &mut self,
        manga_id: &str,
        chapter_id: &str,
    ) -> Result<crate::wasm::kani::extension::types::Chapter> {
        execute_wasm!(self, call_get_pages, manga_id, chapter_id)
    }

    /// Calls `get_fetched_option_sets`. Returns a JSON-encoded list of
    /// `kani_shared::FilterFetchDef`.
    pub async fn get_fetched_option_sets(&mut self) -> Result<String> {
        execute_wasm!(self, call_get_fetched_option_sets)
    }

    /// Calls the `get_metadata` function in the WASM module. Returns the
    /// JSON-encoded `kani_shared::ExtensionMetadata` string.
    pub async fn get_metadata(&mut self) -> Result<String> {
        execute_wasm!(self, call_get_metadata)
    }

    /// Calls the `get_filter_list` function in the WASM module.
    pub async fn get_filter_list(
        &mut self,
    ) -> Result<crate::wasm::kani::extension::types::FilterList> {
        execute_wasm!(self, call_get_filter_list)
    }

    /// Calls the `get_preferences` function in the WASM module.
    pub async fn get_preferences(
        &mut self,
    ) -> Result<Vec<crate::wasm::kani::extension::types::PreferenceSpec>> {
        execute_wasm!(self, call_get_preferences)
    }

    /// Calls the `get_url` function in the WASM module.
    pub async fn get_url(&mut self, manga_id: &str) -> Result<String> {
        execute_wasm!(self, call_get_url, manga_id)
    }
}
