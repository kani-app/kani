use super::*;
use crate::ids::UserId;
use crate::models::SourceHealthRow;
use crate::source::loader;
use sqlx::Row as _;

const DEFAULT_GLOBAL_SEARCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);

pub(super) fn compile_pure_registry(
    metadata: &kani_shared::ExtensionMetadata,
) -> Option<std::sync::Arc<kani_core::scripting::PureFunctionRegistry>> {
    if metadata.scripts.is_empty() {
        return None;
    }
    match kani_core::scripting::PureFunctionRegistry::compile(&metadata.scripts) {
        Ok(reg) => Some(std::sync::Arc::new(reg)),
        Err(e) => {
            tracing::warn!(
                source = %metadata.id,
                "Failed to compile pure scripts, running without script support: {e}"
            );
            None
        }
    }
}

pub(super) fn compile_hook_registry(
    metadata: &kani_shared::ExtensionMetadata,
) -> Option<std::sync::Arc<kani_core::scripting::HookRegistry>> {
    let scripts = kani_core::scripting::HookScripts {
        shared: metadata.scripts.clone(),
        pre_request: metadata.pre_request.clone(),
        on_status: metadata.on_status.clone(),
        endpoint_pre_request: metadata.endpoint_pre_request.clone(),
        endpoint_on_status: metadata.endpoint_on_status.clone(),
        cache: metadata.cache.clone(),
    };
    if scripts.is_empty() {
        return None;
    }
    match kani_core::scripting::HookRegistry::compile(&scripts) {
        Ok(reg) => Some(std::sync::Arc::new(reg)),
        Err(e) => {
            tracing::warn!(
                source = %metadata.id,
                "Failed to compile hook scripts, running without hook support: {e}"
            );
            None
        }
    }
}

pub(super) async fn resolve_option_set(
    cache: &dyn kani_core::cache::CacheBackend,
    client: &kani_core::http::SmartClient,
    source_id: i64,
    base_url: &str,
    unrestricted_http: bool,
    def: &kani_shared::FilterFetchDef,
) -> Option<Vec<(String, String)>> {
    use std::time::Duration;
    let cache_ns = format!("fetched_opts:{source_id}");
    let cache_key = def.cache_key.as_deref().unwrap_or(&def.option_set_name);
    let ttl = Duration::from_secs(def.cache_ttl as u64);
    if let Some(cached) = cache.get(&cache_ns, cache_key).await {
        return Some(kani_shared::serde_json::from_slice(&cached).unwrap_or_default());
    }
    match kani_core::option_set_fetcher::fetch_option_set(client, def, base_url, unrestricted_http)
        .await
    {
        Ok(opts) => {
            if let Ok(bytes) = kani_shared::serde_json::to_vec(&opts) {
                cache.put(&cache_ns, cache_key, bytes, ttl).await;
            }
            Some(opts)
        }
        Err(e) => {
            tracing::warn!(source_id, option_set = %def.option_set_name, "failed to fetch option set: {e}");
            None
        }
    }
}

impl AppService {
    pub async fn get_source(&self, id: i64) -> Result<Source> {
        let source = sqlx::query_as!(
            Source,
            "SELECT s.id, s.name, s.version, s.base_url, s.enabled, s.favourited, \
             s.unrestricted_http, s.browser_enabled, s.download_concurrency, \
             s.icon, s.description, s.languages, s.schema_version, \
             scb.state as circuit_state \
             FROM sources s \
             LEFT JOIN source_circuit_breakers scb ON scb.source_id = s.id \
             WHERE s.id = ? AND s.deleted_at IS NULL",
            id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound("Source not found".into()))?;

        Ok(source)
    }

    pub async fn list_sources(&self) -> Result<Vec<Source>> {
        sqlx::query_as!(
            Source,
            "SELECT s.id, s.name, s.version, s.base_url, s.enabled, s.favourited, \
             s.unrestricted_http, s.browser_enabled, s.download_concurrency, \
             s.icon, s.description, s.languages, s.schema_version, \
             scb.state as circuit_state \
             FROM sources s \
             LEFT JOIN source_circuit_breakers scb ON scb.source_id = s.id \
             WHERE s.deleted_at IS NULL \
             LIMIT 1000"
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(Into::into)
    }

    /// Updates the name and/or version of an existing source.
    pub async fn update_source(
        &self,
        id: i64,
        name: Option<String>,
        version: Option<String>,
    ) -> Result<()> {
        sqlx::query!(
            "UPDATE sources SET name = COALESCE(?, name), version = COALESCE(?, version) WHERE id = ?",
            name,
            version,
            id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Reads the persisted browser-capability gate for a source, defaulting to
    /// enabled when the row is missing (e.g. during install before the row lands).
    pub(crate) async fn browser_enabled_flag(&self, id: i64) -> bool {
        sqlx::query_scalar!("SELECT browser_enabled FROM sources WHERE id = ?", id)
            .fetch_optional(&self.db_read)
            .await
            .ok()
            .flatten()
            .unwrap_or(true)
    }

    /// Flips the operator gate for a source's browser capability, persisting it
    /// and applying it to the loaded source immediately.
    pub async fn set_source_browser_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        sqlx::query!(
            "UPDATE sources SET browser_enabled = ? WHERE id = ? AND deleted_at IS NULL",
            enabled,
            id
        )
        .execute(&self.db)
        .await?;
        if let Some(backend) = self.sources.get_backend(id) {
            backend.set_browser_enabled(enabled);
        }
        Ok(())
    }

    /// Sets or clears the per-source download concurrency override.
    /// `None` clears the override, falling back to the global setting.
    pub async fn set_source_download_concurrency(
        &self,
        id: i64,
        concurrency: Option<i64>,
    ) -> Result<()> {
        sqlx::query!(
            "UPDATE sources SET download_concurrency = ? WHERE id = ? AND deleted_at IS NULL",
            concurrency,
            id
        )
        .execute(&self.db)
        .await?;
        self.job_manager.invalidate_source_semaphore(id);
        Ok(())
    }

    /// Soft-deletes a source: marks manga as orphaned, sets deleted_at on the source
    /// row, removes the WASM file, and evicts the source from the runtime. The source
    /// row is kept so manga.source_id FKs remain valid.
    pub async fn delete_source(&self, id: i64, user_id: UserId) -> Result<()> {
        let row = sqlx::query!(
            "SELECT name FROM sources WHERE id = ? AND deleted_at IS NULL",
            id
        )
        .fetch_optional(&self.db_read)
        .await?;

        if let Some(row) = row {
            let mut tx = self.db.begin().await?;

            let orphaned = sqlx::query_scalar!(
                "UPDATE manga SET is_orphaned = TRUE WHERE source_id = ? RETURNING id",
                id
            )
            .fetch_all(&mut *tx)
            .await?
            .len() as i64;

            sqlx::query!(
                "UPDATE sources SET deleted_at = datetime('now'), enabled = FALSE WHERE id = ?",
                id
            )
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;

            let wasm_path = self.settings.read().await.wasm_storage_path.clone();
            let storage = wasm_path
                .to_str()
                .ok_or_else(|| {
                    ServiceError::Internal("wasm_storage_path is not valid UTF-8".into())
                })?
                .to_owned();
            kani_core::file_storage::delete_wasm_file(&storage, &row.name)
                .await
                .map_err(ServiceError::Core)?;

            self.sources.remove_and_shutdown(id, "source-delete").await;
            let profile_dir = kani_core::v8_process::profile_dir_for(&row.name);
            let _ = tokio::fs::remove_dir_all(&profile_dir).await;
            self.cache.invalidate_source(id);
            self.audit(
                Some(user_id),
                "source.delete",
                Some(&row.name),
                Some(serde_json::json!({ "manga_orphaned": orphaned })),
            )
            .await;
        }

        Ok(())
    }

    pub async fn list_active_source_ids(&self) -> Result<Vec<i64>> {
        Ok(self.sources.active_ids())
    }

    pub async fn toggle_source_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        sqlx::query!("UPDATE sources SET enabled = ? WHERE id = ?", enabled, id)
            .execute(&self.db)
            .await?;

        if enabled {
            let source = sqlx::query!(
                "SELECT name, base_url, unrestricted_http FROM sources WHERE id = ? AND deleted_at IS NULL",
                id
            )
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Source {id} not found")))?;

            let wasm_storage_path = self.settings.read().await.wasm_storage_path.clone();

            let yaml_path = wasm_storage_path.join(format!("{}.yaml", source.name));
            if yaml_path.exists() {
                let text = tokio::fs::read_to_string(&yaml_path)
                    .await
                    .map_err(|e| ServiceError::Core(kani_core::Error::Io(e)))?;
                let ext = kani_yaml::parse_and_validate(&text, &yaml_path).map_err(|errs| {
                    ServiceError::Internal(
                        errs.iter()
                            .map(|e| e.to_string())
                            .collect::<Vec<_>>()
                            .join("; "),
                    )
                })?;
                let prefs = self.load_pref_map(id).await?;
                let browser_enabled = self.browser_enabled_flag(id).await;
                let backend = loader::build_yaml_source(
                    std::sync::Arc::new(ext),
                    self.smart_client.clone(),
                    std::sync::Arc::clone(&self.ext_cache),
                    format!("{}:", source.name),
                    prefs,
                    browser_enabled,
                );
                self.sources.hot_swap(id, backend).await;
                return Ok(());
            }

            let wasm_path = wasm_storage_path.join(format!("{}.wasm", source.name));

            let bytes = tokio::fs::read(&wasm_path)
                .await
                .map_err(|e| ServiceError::Core(kani_core::Error::Io(e)))?;

            let component = self
                .wasm_runtime
                .compile_component(&bytes)
                .map_err(ServiceError::Core)?;

            let instance_pre = self
                .wasm_runtime
                .instantiate_pre(&component)
                .map_err(ServiceError::Core)?;

            let prefs = self.load_pref_map(id).await?;

            let (pure_registry, hook_registry, max_hook_requests) = {
                let mut inst =
                    kani_core::sources::SourceInstance::new(self.smart_client.clone(), None, false);
                if inst
                    .load(
                        self.wasm_runtime.engine(),
                        &component,
                        self.wasm_runtime.linker(),
                    )
                    .await
                    .is_ok()
                {
                    let meta = inst.get_metadata().await.ok().and_then(|raw| {
                        serde_json::from_str::<kani_shared::ExtensionMetadata>(&raw).ok()
                    });
                    let max_hk = meta
                        .as_ref()
                        .and_then(|m| m.rate_limit.as_ref())
                        .map(|rl| rl.max_hook_requests)
                        .unwrap_or(3);
                    let pure_reg = meta.as_ref().and_then(compile_pure_registry);
                    let hook_reg = meta.as_ref().and_then(compile_hook_registry);
                    (pure_reg, hook_reg, max_hk)
                } else {
                    (None, None, 3u32)
                }
            };

            let ns = format!("{}:", source.name);
            let browser_enabled = self.browser_enabled_flag(id).await;
            let backend = loader::build_wasm_source(
                self.wasm_runtime.engine().clone(),
                instance_pre,
                self.smart_client.clone(),
                Some(source.base_url),
                source.unrestricted_http,
                browser_enabled,
                prefs,
                std::sync::Arc::clone(&self.ext_cache),
                ns,
                pure_registry,
                hook_registry,
                max_hook_requests,
            );

            self.sources.hot_swap(id, backend).await;
        } else {
            if let Ok(base_url) = self.get_source_base_url(id).await
                && let Some(domain) = base_url
                    .parse::<url::Url>()
                    .ok()
                    .and_then(|u| u.host_str().map(|h| h.to_owned()))
            {
                self.smart_client.deregister_rate_limit(&domain);
            }
            self.sources
                .remove_and_shutdown(id, "source-disabled")
                .await;
            self.cache.invalidate_source(id);
        }

        Ok(())
    }

    pub async fn toggle_source_favourite(&self, id: i64, favourited: bool) -> Result<()> {
        sqlx::query!(
            "UPDATE sources SET favourited = ? WHERE id = ?",
            favourited,
            id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Returns the base URL of a source, or an empty string if the source has none.
    pub async fn get_source_base_url(&self, id: i64) -> Result<String> {
        Ok(sqlx::query_scalar!(
            "SELECT base_url FROM sources WHERE id = ? AND deleted_at IS NULL",
            id
        )
        .fetch_optional(&self.db_read)
        .await?
        .unwrap_or_default())
    }

    pub async fn get_metadata(&self, id: i64) -> Result<String> {
        self.require_source_active(id).await?;
        let backend = self
            .sources
            .get_backend(id)
            .ok_or_else(|| ServiceError::NotFound(format!("Source {id} not found")))?;

        let t0 = std::time::Instant::now();
        let result = backend.get_metadata().await;
        let elapsed = t0.elapsed().as_millis() as u64;
        match result {
            Ok(r) => {
                self.record_source_success(id, elapsed).await;
                Ok(r)
            }
            Err(e) => {
                self.record_source_error(id).await;
                Err(ServiceError::Core(e))
            }
        }
    }

    /// Fills in the filter defaults an extension declares but the caller left
    /// unset.
    ///
    /// A `default:` is what the source's own UI submits. Sending nothing lets
    /// the site apply its own default instead, so an identical query can come
    /// back in a different order and with a different set.
    async fn with_filter_defaults(
        &self,
        id: i64,
        mut active: Vec<kani_shared::types::ActiveFilter>,
    ) -> Vec<kani_shared::types::ActiveFilter> {
        let defaults = match self.cache.get_filter_defaults(id) {
            Some(cached) => cached,
            None => {
                let Some(backend) = self.sources.get_backend(id) else {
                    return active;
                };
                // Deliberately not `self.get_filter_list`: that records source
                // health, and a local config read is not evidence a source works.
                let Ok((list, _)) = backend.get_filter_list_with_options().await else {
                    return active;
                };
                let defaults: Vec<kani_shared::types::ActiveFilter> = list
                    .filters
                    .into_iter()
                    .filter_map(|def| {
                        def.default_value
                            .map(|state| kani_shared::types::ActiveFilter {
                                filter_name: def.id,
                                state: state.into(),
                            })
                    })
                    .collect();
                self.cache.insert_filter_defaults(id, defaults.clone());
                defaults
            }
        };

        for default in defaults {
            if !active.iter().any(|f| f.filter_name == default.filter_name) {
                active.push(default);
            }
        }
        active
    }

    pub async fn get_popular_manga(
        &self,
        id: i64,
        page: i32,
        page_size: i32,
        filters: Option<String>,
    ) -> Result<String> {
        self.require_source_active(id).await?;
        let filters_key = filters.clone().unwrap_or_default();
        let active_filters: Vec<kani_shared::types::ActiveFilter> = filters
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let active_filters = self.with_filter_defaults(id, active_filters).await;
        let sources = self.sources.clone();
        let svc = self.clone();

        self.cache
            .get_or_fetch_popular_manga(id, page, page_size, filters_key, async move {
                let started = std::time::Instant::now();
                let outcome = async {
                    let backend = sources
                        .get_backend(id)
                        .ok_or_else(|| ServiceError::NotFound(format!("Source {id} not found")))?;
                    let result = backend
                        .get_popular_manga(page, page_size, &active_filters)
                        .await?;
                    serde_json::to_string(&result)
                        .map_err(|e| ServiceError::Core(kani_core::Error::Json(e)))
                }
                .await;
                svc.record_source_call(id, started, &outcome).await;
                outcome
            })
            .await
            .map_err(unwrap_cache_err)
    }

    pub async fn search_manga(
        &self,
        id: i64,
        query: &str,
        page: i32,
        page_size: i32,
        filters: Option<String>,
    ) -> Result<String> {
        self.require_source_active(id).await?;
        let filters_key = filters.clone().unwrap_or_default();
        let active_filters: Vec<kani_shared::types::ActiveFilter> = filters
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let active_filters = self.with_filter_defaults(id, active_filters).await;
        let sources = self.sources.clone();
        let svc = self.clone();
        let q = query.to_string();
        self.cache
            .get_or_fetch_search_results(id, query, page, page_size, filters_key, async move {
                let started = std::time::Instant::now();
                let outcome = async {
                    let backend = sources
                        .get_backend(id)
                        .ok_or_else(|| ServiceError::NotFound(format!("Source {id} not found")))?;
                    let result = backend
                        .search_manga(&q, page, page_size, &active_filters)
                        .await?;
                    serde_json::to_string(&result)
                        .map_err(|e| ServiceError::Core(kani_core::Error::Json(e)))
                }
                .await;
                svc.record_source_call(id, started, &outcome).await;
                outcome
            })
            .await
            .map_err(unwrap_cache_err)
    }

    pub async fn get_filter_list(&self, id: i64) -> Result<kani_core::WitFilterList> {
        self.require_source_active(id).await?;
        let backend = self
            .sources
            .get_backend(id)
            .ok_or_else(|| ServiceError::NotFound(format!("Source {id} not found")))?;
        let t0 = std::time::Instant::now();
        let (mut filter_list, fetched_defs_json) =
            match backend.get_filter_list_with_options().await {
                Ok(pair) => {
                    self.record_source_success(id, t0.elapsed().as_millis() as u64)
                        .await;
                    pair
                }
                Err(e) => {
                    self.record_source_error(id).await;
                    return Err(ServiceError::Core(e));
                }
            };
        if let Ok(defs) = kani_shared::serde_json::from_str::<Vec<kani_shared::FilterFetchDef>>(
            &fetched_defs_json,
        ) {
            filter_list = self.inject_fetched_options(id, filter_list, &defs).await;
        }
        Ok(filter_list)
    }

    async fn inject_fetched_options(
        &self,
        source_id: i64,
        mut filter_list: kani_core::WitFilterList,
        defs: &[kani_shared::FilterFetchDef],
    ) -> kani_core::WitFilterList {
        let (base_url, unrestricted_http) = sqlx::query!(
            "SELECT base_url, unrestricted_http AS \"unrestricted_http: bool\" \
             FROM sources WHERE id = ?",
            source_id
        )
        .fetch_optional(&self.db_read)
        .await
        .ok()
        .flatten()
        .map(|r| (r.base_url, r.unrestricted_http))
        .unwrap_or_default();

        for def in defs {
            let options = resolve_option_set(
                &*self.ext_cache,
                &self.smart_client,
                source_id,
                &base_url,
                unrestricted_http,
                def,
            )
            .await;
            let Some(options) = options else { continue };

            if let Some(filter) = filter_list
                .filters
                .iter_mut()
                .find(|f| f.id == def.filter_id)
            {
                filter.options = options
                    .into_iter()
                    .map(
                        |(name, value)| kani_core::wasm::kani::extension::types::FilterOption {
                            filter_name: def.filter_id.clone(),
                            name,
                            value,
                        },
                    )
                    .collect();
            }
        }
        filter_list
    }

    pub async fn get_source_url(&self, id: i64, manga_id: &str) -> Result<String> {
        self.require_source_active(id).await?;
        let backend = self
            .sources
            .get_backend(id)
            .ok_or_else(|| ServiceError::NotFound(format!("Source {id} not found")))?;
        backend
            .get_source_url(manga_id)
            .await
            .map_err(ServiceError::Core)
    }

    pub async fn get_manga_details(&self, id: i64, manga_id: &str) -> Result<String> {
        self.require_source_active(id).await?;
        let sources = self.sources.clone();
        let manga_id_d = manga_id.to_string();

        let svc = self.clone();

        self.cache
            .get_or_fetch_manga_details(id, &manga_id_d.clone(), async move {
                let started = std::time::Instant::now();
                let outcome = async {
                    let backend = sources
                        .get_backend(id)
                        .ok_or_else(|| ServiceError::NotFound(format!("Source {id} not found")))?;
                    let result = backend.get_manga_details(&manga_id_d).await?;
                    serde_json::to_string(&convert_to_shared_manga_info(result))
                        .map_err(|e| ServiceError::Core(kani_core::Error::Json(e)))
                }
                .await;
                svc.record_source_call(id, started, &outcome).await;
                outcome
            })
            .await
            .map_err(unwrap_cache_err)
    }

    pub async fn get_pages(&self, id: i64, manga_id: &str, chapter_id: &str) -> Result<String> {
        self.require_source_active(id).await?;
        let sources = self.sources.clone();
        let svc = self.clone();
        let manga_id_d = manga_id.to_string();
        let chapter_id_d = chapter_id.to_string();

        self.cache
            .get_or_fetch_pages(id, &manga_id_d.clone(), &chapter_id_d.clone(), async move {
                let started = std::time::Instant::now();
                let outcome = async {
                    let backend = sources
                        .get_backend(id)
                        .ok_or_else(|| ServiceError::NotFound(format!("Source {id} not found")))?;
                    let result = backend.get_pages(&manga_id_d, &chapter_id_d).await?;
                    serde_json::to_string(&result)
                        .map_err(|e| ServiceError::Core(kani_core::Error::Json(e)))
                }
                .await;
                svc.record_source_call(id, started, &outcome).await;
                outcome
            })
            .await
            .map_err(unwrap_cache_err)
    }

    pub async fn get_chapter_list_paged(
        &self,
        id: i64,
        manga_id: &str,
        page: i32,
        page_size: i32,
        sort: Option<String>,
    ) -> Result<String> {
        self.require_source_active(id).await?;
        let sources = self.sources.clone();
        let svc = self.clone();
        let manga_id_d = manga_id.to_string();
        let sort_key = sort.clone().unwrap_or_default();

        self.cache
            .get_or_fetch_chapter_list(
                id,
                &manga_id_d.clone(),
                page,
                page_size,
                &sort_key,
                async move {
                    let started = std::time::Instant::now();
                    let outcome = async {
                        let backend = sources.get_backend(id).ok_or_else(|| {
                            ServiceError::NotFound(format!("Source {id} not found"))
                        })?;
                        let result = backend
                            .get_chapter_list(&manga_id_d, page, Some(page_size), sort)
                            .await?;
                        serde_json::to_string(&result)
                            .map_err(|e| ServiceError::Core(kani_core::Error::Json(e)))
                    }
                    .await;
                    svc.record_source_call(id, started, &outcome).await;
                    outcome
                },
            )
            .await
            .map_err(unwrap_cache_err)
    }

    pub async fn get_chapter_sort_list(
        &self,
        id: i64,
    ) -> Result<Vec<kani_shared::types::SortOption>> {
        self.require_source_active(id).await?;
        let backend = self
            .sources
            .get_backend(id)
            .ok_or_else(|| ServiceError::NotFound(format!("Source {id} not found")))?;
        let wit_opts = backend.get_chapter_sort_list().await?;
        Ok(wit_opts
            .into_iter()
            .map(|o| kani_shared::types::SortOption {
                id: o.id,
                name: o.name,
            })
            .collect())
    }

    pub(super) async fn require_source_active(&self, id: i64) -> Result<()> {
        if self.sources.contains_key(id) {
            return Ok(());
        }
        let row = sqlx::query!(
            "SELECT enabled FROM sources WHERE id = ? AND deleted_at IS NULL",
            id
        )
        .fetch_optional(&self.db_read)
        .await?;
        match row {
            None => Err(ServiceError::NotFound(format!("Source {id} not found"))),
            Some(r) if !r.enabled => Err(ServiceError::SourceDisabled(id)),
            Some(_) => Err(ServiceError::NotFound(format!("Source {id} not found"))),
        }
    }

    pub async fn global_search(
        &self,
        query: &str,
        scope: SearchScope,
        page: i32,
        page_size: i32,
    ) -> Result<Vec<GlobalSearchResult>> {
        let favourited_only = matches!(scope, SearchScope::FavouritedOnly) as i64;

        let ids_to_search: IndexMap<i64, String> = sqlx::query!(
            "SELECT id, name FROM sources WHERE enabled = 1 AND deleted_at IS NULL AND (favourited = 1 OR ? = 0)",
            favourited_only
        )
        .fetch_all(&self.db_read)
        .await?
        .into_iter()
        .filter(|r| match &scope {
            SearchScope::Sources(ids) => ids.contains(&r.id),
            _ => true,
        })
        .map(|r| (r.id, r.name))
        .collect();

        let per_source_timeout =
            u64::try_from(self.settings.read().await.global_search_timeout_secs)
                .map(std::time::Duration::from_secs)
                .unwrap_or(DEFAULT_GLOBAL_SEARCH_TIMEOUT)
                .max(std::time::Duration::from_secs(1));

        let tasks: Vec<_> = ids_to_search
            .iter()
            .map(|(&source_id, source_name)| {
                let state = self.clone();
                let q = query.to_string();
                let source_name = source_name.clone();

                tokio::spawn(async move {
                    // One unresponsive source must not hold the whole aggregate hostage for the
                    // client's full timeout. Every other source has already answered; this one
                    // reports as failed and the results ship.
                    let result = match tokio::time::timeout(
                        per_source_timeout,
                        state.search_manga(source_id, &q, page, page_size, None),
                    )
                    .await
                    {
                        Ok(r) => r,
                        Err(_) => {
                            state.record_source_error(source_id).await;
                            Err(ServiceError::Internal(format!(
                                "Source {source_id} did not answer within {}s",
                                per_source_timeout.as_secs()
                            )))
                        }
                    };
                    (source_id, source_name, result)
                })
            })
            .collect();

        let outcomes = futures::future::join_all(tasks).await;

        let mut per_source_results: Vec<GlobalSearchResult> = Vec::new();

        for outcome in outcomes {
            match outcome {
                Ok((source_id, source_name, Ok(json))) => {
                    match serde_json::from_str::<MangaList>(&json) {
                        Ok(manga_list) => {
                            per_source_results.push(GlobalSearchResult {
                                source_id,
                                source_name,
                                has_next_page: manga_list.has_next_page,
                                manga: manga_list.manga,
                                error: None,
                            });
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse results for source {source_id}: {e}");
                            return Err(ServiceError::Core(kani_core::Error::Json(e)));
                        }
                    }
                }
                Ok((source_id, _, Err(e))) => {
                    tracing::warn!("Search failed for source {source_id}: {e}");
                    per_source_results.push(GlobalSearchResult {
                        source_id,
                        source_name: ids_to_search.get(&source_id).cloned().unwrap_or_default(),
                        has_next_page: false,
                        manga: vec![],
                        error: Some(e.to_string()),
                    });
                }
                Err(join_err) => {
                    tracing::error!("Task panicked: {join_err}");
                }
            }
        }

        Ok(per_source_results)
    }

    pub async fn get_source_health(&self) -> Result<Vec<SourceHealthRow>> {
        let rows = sqlx::query_as::<_, SourceHealthRow>(
            r#"SELECT
                s.id AS source_id,
                s.name AS source_name,
                sh.last_success_at,
                sh.last_error_at,
                COALESCE(sh.consecutive_error_count, 0) AS consecutive_error_count,
                sh.avg_response_ms
            FROM sources s
            LEFT JOIN source_health sh ON sh.source_id = s.id
            WHERE s.deleted_at IS NULL
            ORDER BY s.name"#,
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows)
    }

    /// Records health for a call whose result we already have.
    ///
    /// All source operations must report through this path to keep health complete.
    pub(crate) async fn record_source_call<T, E>(
        &self,
        source_id: i64,
        started: std::time::Instant,
        result: &std::result::Result<T, E>,
    ) {
        match result {
            Ok(_) => {
                self.record_source_success(source_id, started.elapsed().as_millis() as u64)
                    .await
            }
            Err(_) => self.record_source_error(source_id).await,
        }
    }

    pub(crate) async fn record_source_success(&self, source_id: i64, elapsed_ms: u64) {
        let ms = elapsed_ms as f64;
        let _ = sqlx::query(
            r#"INSERT INTO source_health (source_id, last_success_at, consecutive_error_count, avg_response_ms)
               VALUES (?, datetime('now'), 0, ?)
               ON CONFLICT(source_id) DO UPDATE SET
                 last_success_at = datetime('now'),
                 consecutive_error_count = 0,
                 avg_response_ms = CASE
                   WHEN avg_response_ms IS NULL THEN excluded.avg_response_ms
                   ELSE (avg_response_ms * 0.8 + excluded.avg_response_ms * 0.2)
                 END"#,
        )
        .bind(source_id)
        .bind(ms)
        .execute(&self.db)
        .await;
    }

    pub(crate) async fn record_source_error(&self, source_id: i64) {
        let _ = sqlx::query(
            r#"INSERT INTO source_health (source_id, last_error_at, consecutive_error_count)
               VALUES (?, datetime('now'), 1)
               ON CONFLICT(source_id) DO UPDATE SET
                 last_error_at = datetime('now'),
                 consecutive_error_count = consecutive_error_count + 1"#,
        )
        .bind(source_id)
        .execute(&self.db)
        .await;
    }

    pub(crate) async fn scan_and_register_sources(
        db: &SqlitePool,
        wasm_storage_path: &std::path::Path,
        smart_client: kani_core::http::SmartClient,
        wasm_runtime: &WasmRuntime,
        preference_schemas: &DashMap<i64, Vec<kani_core::PreferenceSpec>>,
        ext_cache: &dyn kani_core::cache::CacheBackend,
    ) -> Result<()> {
        tracing::info!(
            "Scanning and registering sources in {:?}",
            wasm_storage_path
        );

        let mut entries = tokio::fs::read_dir(wasm_storage_path)
            .await
            .map_err(|e| ServiceError::Core(kani_core::Error::Io(e)))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| ServiceError::Core(kani_core::Error::Io(e)))?
        {
            let path = entry.path();
            let ext_str = path.extension().and_then(|s| s.to_str());
            if ext_str == Some("yaml") {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_owned();

                let text = match tokio::fs::read_to_string(&path).await {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("Failed to read YAML source {:?}: {}", path, e);
                        continue;
                    }
                };

                let (ext, load_error) = match kani_yaml::parse_and_validate(&text, &path) {
                    Ok(ext) => {
                        let err = crate::install_gating::check_required_capabilities(
                            &ext.requires_capabilities,
                            kani_core::http::SolverCapability::Capture,
                        )
                        .err()
                        .or_else(|| {
                            crate::install_gating::check_min_kani_version(
                                ext.min_kani_version.as_deref(),
                                env!("CARGO_PKG_VERSION"),
                            )
                            .err()
                        });
                        (Some(ext), err)
                    }
                    Err(errs) => {
                        let msg = errs
                            .iter()
                            .map(|e| e.to_string())
                            .collect::<Vec<_>>()
                            .join("; ");
                        tracing::warn!("YAML source {:?} failed validation: {}", path, msg);
                        (None, Some(msg))
                    }
                };

                let canonical_id = ext.as_ref().map(|e| e.id.clone()).unwrap_or(stem);
                let enabled = ext.is_some() && load_error.is_none();

                if let Some(ref ext) = ext
                    && let Some(ref rl) = ext.metadata.rate_limit
                    && let Some(domain) = ext
                        .base_url
                        .parse::<url::Url>()
                        .ok()
                        .and_then(|u| u.host_str().map(|h| h.to_owned()))
                {
                    smart_client.register_rate_limit(
                        &domain,
                        &kani_shared::extension::RateLimitConfig {
                            requests_per_second: rl.requests_per_second as f32,
                            burst: rl.burst,
                            max_concurrent: rl.max_concurrent,
                            max_hook_requests: rl.max_hook_requests,
                        },
                    );
                }

                let version = ext.as_ref().map(|e| e.version.as_str()).unwrap_or("0.0.0");
                let base_url = ext.as_ref().map(|e| e.base_url.as_str()).unwrap_or("");
                let unrestricted = ext.as_ref().map(|e| e.unrestricted_http).unwrap_or(false);
                let mihon_id: Option<i64> = ext.as_ref().and_then(|e| e.mihon_source_id);
                let enabled_i = enabled as i64;

                let existing = sqlx::query("SELECT id, version FROM sources WHERE name = ?")
                    .bind(&canonical_id)
                    .fetch_optional(db)
                    .await?;

                if let Some(row) = existing {
                    let id: i64 = row.try_get("id")?;
                    let stored_version: String = row.try_get("version")?;
                    if ext.is_some() && stored_version != version {
                        crate::cache::invalidate_extension_cache(ext_cache, &canonical_id, id)
                            .await;
                    }
                    sqlx::query(
                        "UPDATE sources SET version = ?, base_url = ?, unrestricted_http = ?, \
                         mihon_source_id = ?, load_error = ?, \
                         enabled = CASE WHEN ? IS NULL THEN enabled ELSE 0 END, \
                         deleted_at = NULL WHERE id = ?",
                    )
                    .bind(version)
                    .bind(base_url)
                    .bind(unrestricted)
                    .bind(mihon_id)
                    .bind(load_error.as_deref())
                    .bind(load_error.as_deref())
                    .bind(id)
                    .execute(db)
                    .await?;
                } else {
                    sqlx::query(
                        "INSERT INTO sources (name, version, base_url, enabled, unrestricted_http, \
                         mihon_source_id, load_error) VALUES (?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(&canonical_id)
                    .bind(version)
                    .bind(base_url)
                    .bind(enabled_i)
                    .bind(unrestricted)
                    .bind(mihon_id)
                    .bind(load_error.as_deref())
                    .execute(db)
                    .await?;
                    if enabled {
                        tracing::info!("Registered YAML source: {}", canonical_id);
                    } else {
                        tracing::warn!("Registered YAML source '{}' with load error", canonical_id);
                    }
                }
                continue;
            } else if ext_str == Some("wasm") {
                let filename = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| ServiceError::Internal("Invalid filename".to_string()))?
                    .to_owned();

                let bytes = tokio::fs::read(&path)
                    .await
                    .map_err(|e| ServiceError::Core(kani_core::Error::Io(e)))?;

                let component = wasm_runtime
                    .compile_component(&bytes)
                    .map_err(ServiceError::Core)?;

                let (raw_meta, schema) = {
                    let mut inst =
                        kani_core::sources::SourceInstance::new(smart_client.clone(), None, false);
                    inst.load(wasm_runtime.engine(), &component, wasm_runtime.linker())
                        .await
                        .map_err(ServiceError::Core)?;

                    let meta = inst.get_metadata().await.map_err(ServiceError::Core)?;
                    let schema = inst.get_preferences().await.ok();
                    (meta, schema)
                };

                let metadata =
                    match serde_json::from_str::<kani_shared::ExtensionMetadata>(&raw_meta) {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::error!("Failed to convert metadata for {}: {}", filename, e);
                            continue;
                        }
                    };

                let canonical_id = metadata.id.clone();
                let initially_enabled = if metadata.unrestricted_http {
                    0i64
                } else {
                    1i64
                };

                if let Some(ref rl) = metadata.rate_limit
                    && let Some(domain) = metadata
                        .base_url
                        .parse::<url::Url>()
                        .ok()
                        .and_then(|u| u.host_str().map(|h| h.to_owned()))
                {
                    smart_client.register_rate_limit(&domain, rl);
                }

                let rename_file = |current_filename: &str| {
                    let src = wasm_storage_path.join(format!("{current_filename}.wasm"));
                    let dst = wasm_storage_path.join(format!("{canonical_id}.wasm"));
                    (src, dst, current_filename != canonical_id.as_str())
                };

                // Already registered under the canonical id — sync metadata, re-activate if
                // soft-deleted, and ensure the file is named correctly.
                let by_id = sqlx::query!(
                    "SELECT id, version, base_url, unrestricted_http, mihon_source_id, deleted_at FROM sources WHERE name = ?",
                    canonical_id
                )
                .fetch_optional(db)
                .await?;

                if let Some(existing) = by_id {
                    if existing.version != metadata.version {
                        tracing::warn!(
                            "Source '{}' version changed: DB has '{}', loaded '{}'",
                            canonical_id,
                            existing.version,
                            metadata.version
                        );
                    }
                    let version_changed = existing.version != metadata.version;
                    let base_url_changed = existing.base_url != metadata.base_url;
                    let http_changed = existing.unrestricted_http != metadata.unrestricted_http;
                    let mihon_changed = existing.mihon_source_id != metadata.mihon_source_id;
                    let was_deleted = existing.deleted_at.is_some();
                    if version_changed
                        || base_url_changed
                        || http_changed
                        || mihon_changed
                        || was_deleted
                    {
                        sqlx::query!(
                            "UPDATE sources SET version = ?, base_url = ?, unrestricted_http = ?, mihon_source_id = ?, deleted_at = NULL WHERE id = ?",
                            metadata.version,
                            metadata.base_url,
                            metadata.unrestricted_http,
                            metadata.mihon_source_id,
                            existing.id
                        )
                        .execute(db)
                        .await?;
                        if was_deleted {
                            sqlx::query!(
                                "UPDATE manga SET is_orphaned = FALSE WHERE source_id = ?",
                                existing.id
                            )
                            .execute(db)
                            .await?;
                            tracing::info!(
                                "Re-activated previously deleted source '{}'",
                                canonical_id
                            );
                        } else {
                            tracing::debug!("Synced metadata for source '{}'", canonical_id);
                        }
                    }
                    let (src, dst, needs_rename) = rename_file(&filename);
                    if needs_rename
                        && src.exists()
                        && !dst.exists()
                        && let Err(e) = tokio::fs::rename(&src, &dst).await
                    {
                        tracing::warn!(
                            "Failed to rename {} → {}: {}",
                            src.display(),
                            dst.display(),
                            e
                        );
                    }
                    continue;
                }

                let legacy = sqlx::query!(
                    "SELECT id FROM sources WHERE name = ? OR name = ? LIMIT 1",
                    filename,
                    metadata.name
                )
                .fetch_optional(db)
                .await?;

                if let Some(row) = legacy {
                    sqlx::query!(
                        "UPDATE sources SET name = ?, version = ?, base_url = ?, unrestricted_http = ?, mihon_source_id = ? WHERE id = ?",
                        canonical_id,
                        metadata.version,
                        metadata.base_url,
                        metadata.unrestricted_http,
                        metadata.mihon_source_id,
                        row.id
                    )
                    .execute(db)
                    .await?;

                    let (src, dst, needs_rename) = rename_file(&filename);
                    if needs_rename
                        && src.exists()
                        && !dst.exists()
                        && let Err(e) = tokio::fs::rename(&src, &dst).await
                    {
                        tracing::warn!(
                            "Failed to rename {} → {}: {}",
                            src.display(),
                            dst.display(),
                            e
                        );
                    }

                    tracing::info!("Migrated source '{}' → '{}'", filename, canonical_id);
                    continue;
                }

                let result = sqlx::query!(
                    "INSERT INTO sources (name, version, base_url, enabled, unrestricted_http, mihon_source_id) VALUES (?, ?, ?, ?, ?, ?)",
                    canonical_id,
                    metadata.version,
                    metadata.base_url,
                    initially_enabled,
                    metadata.unrestricted_http,
                    metadata.mihon_source_id,
                )
                .execute(db)
                .await?;

                if let Some(schema) = schema {
                    preference_schemas.insert(result.last_insert_rowid(), schema);
                }

                let (src, dst, needs_rename) = rename_file(&filename);
                if needs_rename
                    && src.exists()
                    && !dst.exists()
                    && let Err(e) = tokio::fs::rename(&src, &dst).await
                {
                    tracing::warn!(
                        "Failed to rename {} → {}: {}",
                        src.display(),
                        dst.display(),
                        e
                    );
                }

                tracing::info!("Registered new source: {}", canonical_id);
            }
        }
        Ok(())
    }

    pub async fn reload_source(&self, id: i64) -> Result<()> {
        let source = self.get_source(id).await?;
        let storage_path = self.settings.read().await.wasm_storage_path.clone();

        let yaml_path = storage_path.join(format!("{}.yaml", source.name));
        if yaml_path.exists() {
            return self.reload_yaml_source(id, &source.name, &yaml_path).await;
        }

        let wasm_path = storage_path.join(format!("{}.wasm", source.name));

        let bytes = tokio::fs::read(&wasm_path)
            .await
            .map_err(|e| ServiceError::Internal(format!("Failed to read WASM: {e}")))?;

        let runtime_clone = self.wasm_runtime.clone();
        let component =
            tokio::task::spawn_blocking(move || runtime_clone.compile_component(&bytes))
                .await
                .map_err(|e| {
                    ServiceError::Internal(format!("WASM compile task panicked: {e}"))
                })??;

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
            let raw_meta = inst.get_metadata().await.map_err(ServiceError::Core)?;
            let meta: kani_shared::ExtensionMetadata = serde_json::from_str(&raw_meta)
                .map_err(|e| ServiceError::Internal(format!("Invalid extension metadata: {e}")))?;
            let schema = inst.get_preferences().await.ok();
            (meta, schema)
        };

        sqlx::query!(
            "UPDATE sources SET version = ?, base_url = ?, unrestricted_http = ? WHERE id = ?",
            metadata.version,
            metadata.base_url,
            metadata.unrestricted_http,
            id
        )
        .execute(&self.db)
        .await?;

        let pure_registry = compile_pure_registry(&metadata);
        let hook_registry = compile_hook_registry(&metadata);
        let max_hook_requests = metadata
            .rate_limit
            .as_ref()
            .map(|rl| rl.max_hook_requests)
            .unwrap_or(3);
        let backend = loader::build_wasm_source(
            self.wasm_runtime.engine().clone(),
            self.wasm_runtime
                .instantiate_pre(&component)
                .map_err(ServiceError::Core)?,
            self.smart_client.clone(),
            Some(metadata.base_url.clone()),
            metadata.unrestricted_http,
            self.browser_enabled_flag(id).await,
            self.load_pref_map(id).await.unwrap_or_default(),
            std::sync::Arc::clone(&self.ext_cache),
            format!("{}:", metadata.id),
            pure_registry,
            hook_registry,
            max_hook_requests,
        );

        self.sources.insert(id, backend);

        if let Some(schema) = raw_schema {
            self.cache.insert_preference_schema(id, schema);
        }

        self.cache.invalidate_source(id);

        tracing::info!("Reloaded extension {} ({})", id, source.name);
        Ok(())
    }

    async fn reload_yaml_source(
        &self,
        id: i64,
        source_name: &str,
        yaml_path: &std::path::Path,
    ) -> Result<()> {
        let text = tokio::fs::read_to_string(yaml_path)
            .await
            .map_err(|e| ServiceError::Internal(format!("Failed to read YAML: {e}")))?;

        let validated = tokio::task::spawn_blocking(move || {
            stacker::grow(16 * 1024 * 1024, || {
                kani_yaml::parse_and_validate(&text, std::path::Path::new("extension.yaml"))
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

        sqlx::query!(
            "UPDATE sources SET version = ?, base_url = ?, unrestricted_http = ? WHERE id = ?",
            validated.version,
            validated.base_url,
            validated.unrestricted_http,
            id
        )
        .execute(&self.db)
        .await?;

        let prefs = self.load_pref_map(id).await.unwrap_or_default();
        let browser_enabled = self.browser_enabled_flag(id).await;
        let ns = format!("{}:", validated.id);
        let backend = loader::build_yaml_source(
            std::sync::Arc::new(validated),
            self.smart_client.clone(),
            std::sync::Arc::clone(&self.ext_cache),
            ns,
            prefs,
            browser_enabled,
        );
        self.sources.hot_swap(id, backend).await;
        self.cache.invalidate_source(id);

        tracing::info!("Reloaded extension {} ({})", id, source_name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::resolve_option_set;
    use kani_core::cache::{CacheBackend, InMemoryCache};
    use kani_shared::FilterFetchDef;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn make_def(cache_key: Option<&str>) -> FilterFetchDef {
        FilterFetchDef {
            filter_id: "genre".to_string(),
            option_set_name: "genres".to_string(),
            route: "https://example.invalid/api/genres".to_string(),
            response_type: "json".to_string(),
            container: None,
            fields: BTreeMap::new(),
            nsfw_field: None,
            cache_key: cache_key.map(str::to_owned),
            cache_ttl: 600,
        }
    }

    #[tokio::test]
    async fn resolve_option_set_returns_cached_values_without_network_call() {
        let cache = Arc::new(InMemoryCache::new());
        let client = kani_core::http::SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true);

        let options: Vec<(String, String)> = vec![
            ("Action".to_string(), "action".to_string()),
            ("Comedy".to_string(), "comedy".to_string()),
        ];
        let bytes = kani_shared::serde_json::to_vec(&options).unwrap();
        cache
            .put(
                "fetched_opts:1",
                "genres-v1",
                bytes,
                std::time::Duration::from_secs(600),
            )
            .await;

        let def = make_def(Some("genres-v1"));
        let result =
            resolve_option_set(&*cache, &client, 1, "https://example.invalid", false, &def).await;

        let result = result.expect("cache hit must return Some");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("Action".to_string(), "action".to_string()));
        assert_eq!(result[1], ("Comedy".to_string(), "comedy".to_string()));
    }

    #[tokio::test]
    async fn resolve_option_set_returns_none_on_network_failure() {
        let cache = Arc::new(InMemoryCache::new());
        let client = kani_core::http::SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true);
        let def = make_def(None);
        let result =
            resolve_option_set(&*cache, &client, 1, "https://example.invalid", false, &def).await;
        assert!(
            result.is_none(),
            "unreachable URL must return None (no cache, fetch fails)"
        );
    }

    #[tokio::test]
    async fn a_failed_option_set_fetch_does_not_write_the_cache() {
        use kani_shared_test::origin::{Response, TestOrigin};
        let origin = TestOrigin::start().await;
        origin.set("/genres", Response::status(500));

        let cache = Arc::new(InMemoryCache::new());
        let client = kani_core::http::SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true);
        let mut def = make_def(Some("genres-v1"));
        def.route = origin.url("/genres");

        let result = resolve_option_set(&*cache, &client, 1, &origin.base(), true, &def).await;
        assert!(result.is_none(), "a 500 fetch returns None");
        assert!(
            cache.get("fetched_opts:1", "genres-v1").await.is_none(),
            "a failed fetch must leave the cache empty so the next call re-fetches"
        );
    }

    #[tokio::test]
    async fn a_declared_option_set_cache_ttl_is_honoured() {
        use kani_shared_test::origin::{Response, TestOrigin};
        let origin = TestOrigin::start().await;
        origin.set(
            "/genres",
            Response::json(r#"[{"name":"Action","value":"action"}]"#),
        );

        let cache = Arc::new(InMemoryCache::new());
        let client = kani_core::http::SmartClient::new(None)
            .unwrap()
            .with_allow_loopback_egress(true);
        let mut def = make_def(Some("genres-v1"));
        def.route = origin.url("/genres");
        def.cache_ttl = 1;

        let first = resolve_option_set(&*cache, &client, 1, &origin.base(), true, &def).await;
        assert_eq!(first.expect("first fetch succeeds").len(), 1);
        assert_eq!(origin.hits("/genres"), 1);

        let cached = resolve_option_set(&*cache, &client, 1, &origin.base(), true, &def).await;
        assert_eq!(cached.expect("cache hit").len(), 1);
        assert_eq!(
            origin.hits("/genres"),
            1,
            "within the declared TTL the cache is honoured, no re-fetch"
        );

        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        let refetched = resolve_option_set(&*cache, &client, 1, &origin.base(), true, &def).await;
        assert_eq!(refetched.expect("re-fetch succeeds").len(), 1);
        assert_eq!(
            origin.hits("/genres"),
            2,
            "once the declared TTL elapses the value is re-fetched"
        );
    }
}
