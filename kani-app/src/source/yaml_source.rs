use std::collections::HashMap;
use std::sync::Arc;

use kani_core::error::{Error, Result};
use kani_core::wasm::AllowedHost;
use kani_core::wasm::kani::extension::types::{
    Chapter, ChapterList, FilterDef, FilterList, FilterOption, FilterSemantic, FilterState,
    FilterTypeTag, MangaInfo, MangaList, OptionState, PrefKind, PreferenceSpec, SortOption,
};

/// Ceiling on browser page loads spent on one logical page. A page size far
/// larger than the site's own costs one navigation per capture.
const MAX_BROWSER_CAPTURES_PER_PAGE: usize = 6;

/// The span of the site's own pages one logical page is cut from.
///
/// A site's fixed page size rarely divides the size asked for, so a logical page
/// starts mid-capture. Captures are fetched whole and then trimmed, which keeps
/// consecutive pages gapless without overfilling either of them.
#[derive(Clone, Copy)]
struct PageWindow {
    native: usize,
    first_native: usize,
    captures: usize,
    skip: usize,
    take: usize,
}

impl PageWindow {
    fn new(logical_page: usize, requested: Option<usize>, native_page_size: usize) -> Self {
        let native = native_page_size.max(1);
        let requested = requested.unwrap_or(native).max(1);
        let start = (logical_page.max(1) - 1) * requested;
        let first_native = start / native;
        let captures = ((start + requested - 1) / native - first_native + 1)
            .min(MAX_BROWSER_CAPTURES_PER_PAGE);
        let skip = start - first_native * native;
        Self {
            native,
            first_native,
            captures,
            skip,
            // A window wider than the capture ceiling is served short rather than
            // misaligned: the rows still start where the page begins.
            take: requested.min(captures * native - skip),
        }
    }

    /// The site's own page index for one capture of this window, 0-based.
    fn native_index(&self, capture: usize) -> usize {
        self.first_native + capture
    }

    /// Cuts the fetched captures down to this page. Rows the window pulled in
    /// beyond it belong to the next page, so they are reported as one rather
    /// than shown here, and the site's page count is restated in these pages.
    fn trim(&self, value: &mut serde_json::Value) {
        let Some(rows) = value.get_mut("rows").and_then(|r| r.as_array_mut()) else {
            return;
        };
        let skip = self.skip.min(rows.len());
        rows.drain(..skip);
        let surplus = rows.len() > self.take;
        rows.truncate(self.take);

        let Some(scalars) = value.get_mut("scalars").and_then(|s| s.as_object_mut()) else {
            return;
        };
        if surplus {
            scalars.insert("has_next_page".to_string(), serde_json::Value::Bool(true));
        }
        if let Some(native_total) = scalars
            .get("total_pages")
            .and_then(serde_json::Value::as_i64)
            .filter(|n| *n > 0)
        {
            let items = native_total as usize * self.native;
            scalars.insert(
                "total_pages".to_string(),
                serde_json::Value::from(items.div_ceil(self.take)),
            );
        }
    }
}

/// Folds one capture's extraction into the running result: rows accumulate,
/// and later captures win for the paging scalars they carry.
fn merge_capture(merged: &mut Option<serde_json::Value>, value: serde_json::Value) {
    let Some(acc) = merged.as_mut() else {
        *merged = Some(value);
        return;
    };
    if let Some(rows) = value.get("rows").and_then(|r| r.as_array()).cloned()
        && let Some(acc_rows) = acc.get_mut("rows").and_then(|r| r.as_array_mut())
    {
        acc_rows.extend(rows);
    }
    if let (Some(acc_obj), Some(obj)) = (acc.as_object_mut(), value.as_object()) {
        for (key, v) in obj {
            if key != "rows" {
                acc_obj.insert(key.clone(), v.clone());
            }
        }
    }
}

pub struct YamlSource {
    pub config: Arc<kani_yaml::ValidatedExtension>,
    http: kani_core::http::SmartClient,
    cache: Arc<dyn kani_core::cache::CacheBackend>,
    cache_namespace: String,
    v8_process: kani_core::v8_process::V8ProcessHandle,
    prefs: Arc<std::sync::RwLock<HashMap<String, String>>>,
    semaphore: Arc<tokio::sync::Semaphore>,
    hook_registry: Option<Arc<kani_core::scripting::HookRegistry>>,
    pure_fn_registry: Option<Arc<kani_core::scripting::PureFunctionRegistry>>,
    browser_scripts: Arc<kani_core::scripting::BrowserScriptRegistry>,
    browser_enabled: Arc<std::sync::atomic::AtomicBool>,
    max_hook_requests: u32,
    eval_limits: kani_core::evaluator::EvalLimits,
}

impl YamlSource {
    pub fn new(
        config: Arc<kani_yaml::ValidatedExtension>,
        http: kani_core::http::SmartClient,
        cache: Arc<dyn kani_core::cache::CacheBackend>,
        cache_namespace: String,
        preferences: HashMap<String, String>,
        browser_enabled: bool,
    ) -> Self {
        let max_concurrent = config
            .metadata
            .rate_limit
            .as_ref()
            .map(|rl| rl.max_concurrent as usize)
            .unwrap_or(4);
        let max_hook_requests = config
            .metadata
            .rate_limit
            .as_ref()
            .map(|rl| rl.max_hook_requests)
            .unwrap_or(3);

        let hook_scripts = kani_core::scripting::HookScripts {
            shared: config.pure_scripts.clone(),
            pre_request: config.pre_request.clone(),
            on_status: config.on_status.clone(),
            endpoint_pre_request: config.endpoint_pre_request.clone(),
            endpoint_on_status: config.endpoint_on_status.clone(),
        };
        let hook_registry = if hook_scripts.pre_request.is_some()
            || !hook_scripts.on_status.is_empty()
            || !hook_scripts.endpoint_pre_request.is_empty()
            || !hook_scripts.endpoint_on_status.is_empty()
        {
            kani_core::scripting::HookRegistry::compile(&hook_scripts)
                .ok()
                .map(Arc::new)
        } else {
            None
        };

        let pure_fn_registry = if !config.pure_scripts.is_empty() {
            kani_core::scripting::PureFunctionRegistry::compile(&config.pure_scripts)
                .ok()
                .map(Arc::new)
        } else {
            None
        };

        let browser_scripts = Arc::new(kani_core::scripting::BrowserScriptRegistry::from_map(
            &config.browser_scripts,
        ));

        Self {
            config,
            http,
            cache,
            cache_namespace,
            v8_process: kani_core::v8_process::new_handle(),
            prefs: Arc::new(std::sync::RwLock::new(preferences)),
            semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrent)),
            hook_registry,
            pure_fn_registry,
            browser_scripts,
            browser_enabled: Arc::new(std::sync::atomic::AtomicBool::new(browser_enabled)),
            max_hook_requests,
            eval_limits: kani_core::evaluator::EvalLimits::default(),
        }
    }

    /// Override the evaluator limits (test seam — trip a cap without a giant fixture).
    pub fn with_eval_limits(mut self, limits: kani_core::evaluator::EvalLimits) -> Self {
        self.eval_limits = limits;
        self
    }

    pub fn update_preferences(&self, prefs: HashMap<String, String>) {
        if let Ok(mut lock) = self.prefs.write() {
            *lock = prefs;
        }
    }

    /// `idle_for` governs the local V8 worker only; solver sessions expire on the
    /// solver's own TTL, so the host has nothing to configure for them.
    pub(crate) async fn reap_idle_v8(&self, idle_for: std::time::Duration) -> bool {
        let (local, solver) = tokio::join!(
            kani_core::v8_process::reap_if_idle(&self.v8_process, idle_for),
            self.http
                .reap_solver_sessions(&self.config.id, kani_core::http::solver_session_ttl())
        );
        local || solver > 0
    }

    pub(crate) async fn shutdown_v8(&self, reason: &str) -> bool {
        let (local, solver) = tokio::join!(
            kani_core::v8_process::shutdown(&self.v8_process, reason),
            self.http.destroy_solver_sessions(&self.config.id)
        );
        local || solver > 0
    }

    pub(crate) async fn retire_v8(&self, reason: &str) -> bool {
        let (local, solver) = tokio::join!(
            kani_core::v8_process::retire(&self.v8_process, reason),
            self.http.destroy_solver_sessions(&self.config.id)
        );
        local || solver > 0
    }

    pub fn set_browser_enabled(&self, enabled: bool) {
        self.browser_enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    fn make_host_state(&self) -> Result<kani_core::wasm::HostState> {
        let allowed_host = if self.config.unrestricted_http {
            AllowedHost::Unrestricted
        } else {
            AllowedHost::Restricted(self.config.base_url.clone())
        };
        let mut state = kani_core::wasm::HostState::new(
            self.http.clone(),
            allowed_host,
            Arc::clone(&self.cache),
            self.cache_namespace.clone(),
            Arc::clone(&self.v8_process),
        )?;
        state.preferences = self.prefs.read().unwrap_or_else(|e| e.into_inner()).clone();
        state.hook_registry = self.hook_registry.clone();
        state.pure_fn_registry = self.pure_fn_registry.clone();
        state.browser_scripts = Some(Arc::clone(&self.browser_scripts));
        state.max_hook_requests = self.max_hook_requests;
        state.set_eval_limits(self.eval_limits);
        Ok(state)
    }

    async fn acquire(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Internal("Source concurrency semaphore closed".into()))
    }

    fn build_args(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn make_request(
        ep: &kani_yaml::ValidatedEndpoint,
        ext: &kani_yaml::ValidatedExtension,
        args: &HashMap<String, String>,
        endpoint_name: &str,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> std::result::Result<kani_shared::ast::RequestDef, String> {
        let mut resolved = args.clone();
        kani_yaml::resolve_composite_ids(ep, &mut resolved);
        let url = kani_yaml::build_url_with_args(&ext.base_url, &ep.route, &resolved)?;
        let mut queries = kani_yaml::build_queries(&ep.queries, &resolved);
        queries.extend(kani_yaml::apply_filters(
            &ep.filter_mapping,
            ep.filter_format.as_ref(),
            filters,
        ));
        Ok(kani_shared::ast::RequestDef {
            url,
            method: ep.method.clone(),
            headers: ep.headers.clone(),
            queries,
            endpoint_id: Some(endpoint_name.to_string()),
        })
    }

    async fn eval_endpoint_once(
        &self,
        ep: &kani_yaml::ValidatedEndpoint,
        endpoint_name: &str,
        args: &HashMap<String, String>,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> std::result::Result<serde_json::Value, String> {
        use kani_core::evaluator::{html_eval, json_eval};
        use kani_yaml::yaml::schema::ResponseType;

        let req = Self::make_request(ep, &self.config, args, endpoint_name, filters)?;
        let bp = kani_yaml::build_blueprint(ep, &self.config, endpoint_name, req);
        let mut state = self.make_host_state()?;

        if ep.pagination.is_some() {
            let page = args.get("page").and_then(|p| p.parse().ok()).unwrap_or(1);
            let page_size = args
                .get("page_size")
                .and_then(|p| p.parse().ok())
                .unwrap_or(20);
            return match ep.response_type {
                ResponseType::Html => {
                    html_eval::extract_html_paginated(&mut state, page, page_size, &bp).await
                }
                ResponseType::Json => {
                    json_eval::extract_json_paginated(&mut state, page, page_size, &bp).await
                }
            };
        }

        match ep.response_type {
            ResponseType::Html => html_eval::extract_html(&mut state, None, &bp).await,
            ResponseType::Json => json_eval::extract_json(&mut state, None, &bp).await,
        }
    }

    async fn eval_browser_payload_endpoint(
        &self,
        ep: &kani_yaml::ValidatedEndpoint,
        endpoint_name: &str,
        args: &HashMap<String, String>,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> Result<serde_json::Value> {
        use kani_core::evaluator::json_eval;

        fn invalid(message: String) -> Error {
            Error::Extension(kani_shared::extension::ExtensionError::parse(message))
        }

        if !self
            .browser_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err(invalid(
                "Browser capability is disabled for this source".to_string(),
            ));
        }

        let page_url_template = ep
            .page_url
            .as_deref()
            .ok_or_else(|| invalid("browser_payload endpoint missing page_url".to_string()))?;
        let script_name = ep
            .script_name
            .as_deref()
            .ok_or_else(|| invalid("browser_payload endpoint missing script".to_string()))?;
        let init_script = self
            .browser_scripts
            .get(script_name)
            .ok_or_else(|| invalid(format!("browser script '{script_name}' not declared")))?;

        let mut resolved = args.clone();
        kani_yaml::resolve_composite_ids(ep, &mut resolved);
        let page_url =
            kani_yaml::build_url_with_args("", page_url_template, &resolved).map_err(invalid)?;

        let mut params = kani_yaml::build_queries(&ep.queries, &resolved);
        params.extend(kani_yaml::apply_filters(
            &ep.filter_mapping,
            ep.filter_format.as_ref(),
            filters,
        ));

        let pagination = ep.pagination.as_ref();
        let logical_page = args
            .get("page")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1)
            .max(1);
        let window = pagination.map(|p| {
            PageWindow::new(
                logical_page,
                args.get("page_size").and_then(|v| v.parse::<usize>().ok()),
                p.native_page_size,
            )
        });
        let captures = window.map_or(1, |w| w.captures);

        let req =
            Self::make_request(ep, &self.config, args, endpoint_name, &[]).map_err(invalid)?;
        let bp = kani_yaml::build_blueprint(ep, &self.config, endpoint_name, req);

        let mut merged: Option<serde_json::Value> = None;
        for capture in 0..captures {
            let mut params = params.clone();
            if let (Some(p), Some(w)) = (pagination, window) {
                let native_index = w.native_index(capture);
                let offset = match p.offset_type {
                    kani_yaml::yaml::schema::YamlOffsetType::Page => {
                        (native_index + p.page_start as usize).to_string()
                    }
                    kani_yaml::yaml::schema::YamlOffsetType::Item => {
                        (native_index * p.native_page_size.max(1)).to_string()
                    }
                };
                params.push((p.offset_param.clone(), offset));
            }
            let page_url = append_query_params(&page_url, &params);

            let mut state = self.make_host_state()?;
            let profile_key = state.browser_profile_key.clone();

            // Enforce the source's AllowedHost policy on the browser target before any
            // V8 dispatch, mirroring the HTTP path — a restricted source must not be
            // able to point the browser at an arbitrary host.
            let host = page_url
                .parse::<url::Url>()
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .ok_or_else(|| {
                    invalid(format!("browser_payload page_url has no host: {page_url}"))
                })?;
            state.allowed_host.allows_host(&host).map_err(invalid)?;

            let payload = kani_core::v8_process::capture_page_payload_resilient(
                &self.v8_process,
                &self.http,
                &page_url,
                init_script,
                ep.timeout_ms,
                Some(&profile_key),
                ep.auto_scroll,
            )
            .await
            .map_err(|error| match error {
                kani_core::v8_process::CapturePagePayloadError::Action { code, message }
                    if code.starts_with("solver_") =>
                {
                    Error::BrowserCaptureUnavailable { code, message }
                }
                other => invalid(other.to_string()),
            })?;

            let value = json_eval::extract_json_str(&mut state, &payload, &bp)
                .await
                .map_err(invalid)?;
            merge_capture(&mut merged, value);
        }

        let mut merged =
            merged.ok_or_else(|| invalid("browser_payload produced no captures".to_string()))?;
        if let Some(w) = window {
            w.trim(&mut merged);
        }
        Ok(merged)
    }

    async fn eval_endpoint(
        &self,
        ep: &kani_yaml::ValidatedEndpoint,
        endpoint_name: &str,
        args: &HashMap<String, String>,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> Result<serde_json::Value> {
        use kani_yaml::yaml::schema::EndpointVia;

        if ep.via == Some(EndpointVia::BrowserPayload) {
            let mut v = self
                .eval_browser_payload_endpoint(ep, endpoint_name, args, filters)
                .await?;
            inject_fn_arg_fields(&mut v, ep, args);
            deduplicate_for_each_rows(&mut v, ep).await?;
            return Ok(v);
        }

        let mut retries_left = self.max_hook_requests;
        loop {
            match self
                .eval_endpoint_once(ep, endpoint_name, args, filters)
                .await
            {
                Ok(mut v) => {
                    inject_fn_arg_fields(&mut v, ep, args);
                    deduplicate_for_each_rows(&mut v, ep).await?;
                    return Ok(v);
                }
                Err(ref e) if e.starts_with("__refresh_auth__:") => {
                    if retries_left == 0 {
                        return Err(Error::Extension(
                            kani_shared::extension::ExtensionError::parse(
                                "RefreshAuth: max retries exceeded".to_string(),
                            ),
                        ));
                    }
                    retries_left -= 1;
                    let auth_endpoint_name = e.strip_prefix("__refresh_auth__:").unwrap_or("login");
                    if let Some(auth_ep) = self.config.endpoint_by_name(auth_endpoint_name) {
                        let auth_args = HashMap::new();
                        let _ = self
                            .eval_endpoint_once(auth_ep, auth_endpoint_name, &auth_args, &[])
                            .await;
                    }
                }
                Err(e) => {
                    return Err(Error::Extension(classify_eval_error(e)));
                }
            }
        }
    }

    pub async fn get_metadata(&self) -> Result<String> {
        let rl = self.config.metadata.rate_limit.as_ref().map(|r| {
            kani_shared::extension::RateLimitConfig {
                requests_per_second: r.requests_per_second as f32,
                burst: r.burst,
                max_concurrent: r.max_concurrent,
                max_hook_requests: r.max_hook_requests,
            }
        });
        let meta = kani_shared::extension::ExtensionMetadata {
            id: self.config.id.clone(),
            name: self.config.name.clone(),
            version: self.config.version.clone(),
            base_url: self.config.base_url.clone(),
            language: self.config.language.clone(),
            nsfw: self.config.nsfw,
            unrestricted_http: self.config.unrestricted_http,
            mihon_source_id: self.config.mihon_source_id,
            rate_limit: rl,
            icon: self.config.metadata.icon.clone(),
            languages: self.config.metadata.languages.clone(),
            description: self.config.metadata.description.clone(),
            schema_version: self.config.schema_version,
            min_kani_version: self.config.min_kani_version.clone(),
            requires_capabilities: self.config.requires_capabilities.clone(),
            sections: self
                .config
                .metadata
                .sections
                .iter()
                .map(|s| kani_shared::extension::Section {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    nsfw: s.nsfw,
                })
                .collect(),
            scripts: std::collections::BTreeMap::new(),
            pre_request: self.config.pre_request.clone(),
            on_status: self.config.on_status.clone(),
            endpoint_pre_request: self.config.endpoint_pre_request.clone(),
            endpoint_on_status: self.config.endpoint_on_status.clone(),
        };
        serde_json::to_string(&meta).map_err(Error::Json)
    }

    pub async fn get_popular_manga(
        &self,
        page: i32,
        page_size: i32,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> Result<MangaList> {
        use kani_yaml::yaml::model::ValidatedPopular;

        let _permit = self.acquire().await?;

        match &self.config.popular {
            Some(ValidatedPopular::Delegated {
                delegate_to: _,
                empty_without_filters,
            }) => {
                if *empty_without_filters && filters.is_empty() {
                    return Ok(MangaList {
                        manga: vec![],
                        has_next_page: false,
                        total_pages: None,
                    });
                }
                self.search_inner("", page, page_size, filters).await
            }
            Some(ValidatedPopular::Full(ep)) => {
                let args = Self::build_args(&[
                    ("page", &page.to_string()),
                    ("page_size", &page_size.to_string()),
                ]);
                let result = self.eval_endpoint(ep, "popular", &args, filters).await?;
                Ok(unpack_manga_list(&result, ep))
            }
            None => Err(Error::Extension(
                kani_shared::extension::ExtensionError::parse(
                    "popular endpoint not configured".to_string(),
                ),
            )),
        }
    }

    async fn search_inner(
        &self,
        query: &str,
        page: i32,
        page_size: i32,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> Result<MangaList> {
        let ep = self.config.search.as_ref().ok_or_else(|| {
            Error::Extension(kani_shared::extension::ExtensionError::parse(
                "search endpoint not configured".to_string(),
            ))
        })?;
        let page_str = page.to_string();
        let page_size_str = page_size.to_string();
        let args = Self::build_args(&[
            ("query", query),
            ("page", &page_str),
            ("page_size", &page_size_str),
        ]);
        let result = self.eval_endpoint(ep, "search", &args, filters).await?;
        Ok(unpack_manga_list(&result, ep))
    }

    pub async fn search_manga(
        &self,
        query: &str,
        page: i32,
        page_size: i32,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> Result<MangaList> {
        let _permit = self.acquire().await?;
        self.search_inner(query, page, page_size, filters).await
    }

    pub async fn get_manga_details(&self, manga_id: &str) -> Result<MangaInfo> {
        let _permit = self.acquire().await?;

        let ep = self.config.manga_details.as_ref().ok_or_else(|| {
            Error::Extension(kani_shared::extension::ExtensionError::parse(
                "manga_details endpoint not configured".to_string(),
            ))
        })?;

        let args = Self::build_args(&[("manga_id", manga_id)]);
        let result = self.eval_endpoint(ep, "manga_details", &args, &[]).await?;
        unpack_manga_info(&result)
    }

    pub async fn get_chapter_list(
        &self,
        manga_id: &str,
        page: i32,
        page_size: Option<i32>,
        sort: Option<String>,
    ) -> Result<ChapterList> {
        let _permit = self.acquire().await?;

        let ep = self.config.chapter_list.as_ref().ok_or_else(|| {
            Error::Extension(kani_shared::extension::ExtensionError::parse(
                "chapter_list endpoint not configured".to_string(),
            ))
        })?;

        let page_str = page.to_string();
        let page_size_str = page_size.unwrap_or(100).to_string();
        let sort_str = sort.as_deref().unwrap_or("");
        let args = Self::build_args(&[
            ("manga_id", manga_id),
            ("page", &page_str),
            ("page_size", &page_size_str),
            ("sort", sort_str),
        ]);
        let result = self.eval_endpoint(ep, "chapter_list", &args, &[]).await?;
        Ok(unpack_chapter_list(&result, ep))
    }

    pub async fn get_pages(&self, manga_id: &str, chapter_id: &str) -> Result<Chapter> {
        let _permit = self.acquire().await?;

        let ep = self.config.pages.as_ref().ok_or_else(|| {
            Error::Extension(kani_shared::extension::ExtensionError::parse(
                "pages endpoint not configured".to_string(),
            ))
        })?;

        let args = Self::build_args(&[("manga_id", manga_id), ("chapter_id", chapter_id)]);
        let result = self.eval_endpoint(ep, "pages", &args, &[]).await?;
        Ok(unpack_chapter(&result))
    }

    pub async fn get_chapter_sort_list(&self) -> Result<Vec<SortOption>> {
        let opts = self
            .config
            .chapter_sort
            .as_ref()
            .map(|cs| {
                cs.options
                    .iter()
                    .map(|o| SortOption {
                        id: o.id.clone(),
                        name: o.label.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(opts)
    }

    pub async fn get_source_url(&self, manga_id: &str) -> Result<String> {
        let template = self.config.get_url.as_deref().ok_or_else(|| {
            Error::Extension(kani_shared::extension::ExtensionError::parse(
                "get_url not configured".to_string(),
            ))
        })?;
        let mut args = Self::build_args(&[("manga_id", manga_id)]);
        kani_yaml::resolve_get_url_manga_id(&self.config, &mut args);
        kani_yaml::build_url_with_args(&self.config.base_url, template, &args)
            .map_err(|e| Error::Extension(kani_shared::extension::ExtensionError::parse(e)))
    }

    pub async fn get_filter_list(&self) -> Result<FilterList> {
        use kani_yaml::yaml::schema::{
            FilterDefault, FilterKind, FilterSemantic as YSemantic, OptionSetDef,
        };

        let filters = self
            .config
            .filters
            .iter()
            .map(|entry| {
                let tag = match entry.kind {
                    FilterKind::Select => FilterTypeTag::Select,
                    FilterKind::Checkbox => FilterTypeTag::Checkbox,
                    FilterKind::TextInput => FilterTypeTag::TextInput,
                    FilterKind::Sort => FilterTypeTag::Sort,
                    FilterKind::Multiselect => FilterTypeTag::Multiselect,
                    FilterKind::IntRange | FilterKind::DateRange => FilterTypeTag::TextInput,
                };

                let options: Vec<FilterOption> = if let Some(ref key) = entry.options_ref {
                    match self.config.option_sets.get(key) {
                        Some(OptionSetDef::Static(items)) => items
                            .iter()
                            .map(|item| FilterOption {
                                filter_name: entry.id.clone(),
                                name: item.name.clone(),
                                value: item.value.clone(),
                            })
                            .collect(),
                        _ => vec![],
                    }
                } else {
                    entry
                        .options
                        .iter()
                        .map(|o| FilterOption {
                            filter_name: entry.id.clone(),
                            name: o.name.clone(),
                            value: o.value.clone(),
                        })
                        .collect()
                };

                let default_value = entry.default.as_ref().map(|d| match d {
                    FilterDefault::Bool(b) => FilterState::Checkbox(*b),
                    FilterDefault::Option { name, value } => FilterState::Selection(OptionState {
                        name: name.clone(),
                        value: value.clone(),
                    }),
                    FilterDefault::Text(t) => FilterState::TextInput(t.clone()),
                });

                let semantic = entry.semantic.as_ref().map(|s| match s {
                    YSemantic::Author => FilterSemantic::Author,
                    YSemantic::Artist => FilterSemantic::Artist,
                    YSemantic::Tag => FilterSemantic::Tag,
                });

                FilterDef {
                    id: entry.id.clone(),
                    name: entry.name.clone(),
                    tag,
                    options,
                    default_value,
                    semantic,
                }
            })
            .collect();

        Ok(FilterList { filters })
    }

    pub async fn get_fetched_option_sets(&self) -> Result<String> {
        use kani_yaml::yaml::schema::{OptionSetDef, ResponseType};

        let entries: Vec<kani_shared::filter_fetch::FilterFetchDef> = self
            .config
            .filters
            .iter()
            .filter_map(|f| {
                let options_ref = f.options_ref.as_ref()?;
                let OptionSetDef::Fetched {
                    options_fetched_by: def,
                } = self.config.option_sets.get(options_ref)?
                else {
                    return None;
                };
                Some(kani_shared::filter_fetch::FilterFetchDef {
                    filter_id: f.id.clone(),
                    option_set_name: options_ref.clone(),
                    route: def.route.clone(),
                    response_type: match def.response_type {
                        ResponseType::Html => "html".to_string(),
                        ResponseType::Json => "json".to_string(),
                    },
                    container: def.container.clone(),
                    fields: def.fields.clone(),
                    nsfw_field: def.nsfw_field.clone(),
                    cache_key: def.cache.as_ref().map(|c| c.key.clone()),
                    cache_ttl: def.cache.as_ref().map(|c| c.ttl).unwrap_or(300),
                })
            })
            .collect();

        serde_json::to_string(&entries)
            .map_err(|e| Error::Internal(format!("fetched option sets: {e}")))
    }

    pub async fn get_preferences(&self) -> Result<Vec<PreferenceSpec>> {
        use kani_yaml::yaml::schema::PreferenceKind;

        let specs = self
            .config
            .preferences
            .iter()
            .map(|entry| {
                let kind = match entry.kind {
                    PreferenceKind::Toggle => PrefKind::Toggle,
                    PreferenceKind::Select => PrefKind::Select,
                    PreferenceKind::Text => PrefKind::Text,
                    PreferenceKind::MultiValueList => PrefKind::MultiValueList,
                };
                PreferenceSpec {
                    key: entry.key.clone(),
                    label: entry.label.clone(),
                    kind,
                    options: entry
                        .options
                        .iter()
                        .map(|o| (o.name.clone(), o.value.clone()))
                        .collect(),
                    default: entry.default.clone(),
                    description: entry.description.clone(),
                    secret: entry.secret,
                }
            })
            .collect();

        Ok(specs)
    }

    pub async fn fetch_page_list(
        &self,
        manga_id: &str,
        chapter_id: &str,
    ) -> Result<(Chapter, String)> {
        let chapter = self.get_pages(manga_id, chapter_id).await?;
        Ok((chapter, self.config.base_url.clone()))
    }
}

/// Preserves retry and rate-limit semantics encoded in evaluator HTTP-status errors.
/// Unmarked failures are classified as extraction errors.
fn classify_eval_error(e: String) -> kani_shared::extension::ExtensionError {
    kani_shared::extension::classify_status_error(&e)
        .unwrap_or_else(|| kani_shared::extension::ExtensionError::parse(e))
}

/// Lower an endpoint's `has_next_page` config to the shared unpack spec.
fn hnp_spec(ep: &kani_yaml::ValidatedEndpoint) -> kani_shared::unpack::HasNextPage {
    match &ep.has_next_page {
        kani_yaml::yaml::model::ValidatedHnp::Static(b) => {
            kani_shared::unpack::HasNextPage::Static(*b)
        }
        _ => kani_shared::unpack::HasNextPage::FromScalar,
    }
}

/// Lower an endpoint's `total_pages` config to the shared unpack spec.
fn total_pages_spec(ep: &kani_yaml::ValidatedEndpoint) -> kani_shared::unpack::TotalPages {
    match &ep.total_pages {
        kani_yaml::yaml::model::ValidatedTotalPages::Static(n) => {
            kani_shared::unpack::TotalPages::Static(*n)
        }
        kani_yaml::yaml::model::ValidatedTotalPages::None => kani_shared::unpack::TotalPages::None,
        kani_yaml::yaml::model::ValidatedTotalPages::Scalar(_) => {
            kani_shared::unpack::TotalPages::FromScalar
        }
    }
}

/// Applies each `for_each` step's `deduplicate_by` key to the extracted rows.
///
/// Runs after `inject_fn_arg_fields`, so a key naming a function-argument field
/// such as `$manga_id$` sees it.
async fn deduplicate_for_each_rows(
    result: &mut serde_json::Value,
    ep: &kani_yaml::ValidatedEndpoint,
) -> Result<()> {
    for step in &ep.for_each_steps {
        if let Some(key) = &step.deduplicate_by {
            kani_core::evaluator::json_eval::deduplicate_rows(result, key)
                .await
                .map_err(|e| {
                    Error::Extension(kani_shared::extension::ExtensionError::parse(format!(
                        "deduplicate_by: {e}"
                    )))
                })?;
        }
    }
    Ok(())
}

fn unpack_manga_list(result: &serde_json::Value, ep: &kani_yaml::ValidatedEndpoint) -> MangaList {
    kani_shared::unpack::unpack_manga_list(result, hnp_spec(ep), total_pages_spec(ep), &[]).into()
}

/// Graft function-argument fields (`id: "$manga_id$"`) onto each extracted row.
///
/// Codegen substitutes such a field from the method argument, but the
/// interpreted engine builds a blueprint from only the `Blueprint`-sourced
/// fields, so a `FnArg` field never appears in the extraction result. Without
/// this, `unpack_manga_info` sees a row missing `id` and fails — a divergence
/// from the compiled path for the standard `$manga_id$` id pattern.
fn inject_fn_arg_fields(
    result: &mut serde_json::Value,
    ep: &kani_yaml::ValidatedEndpoint,
    args: &HashMap<String, String>,
) {
    use kani_yaml::yaml::model::FieldSource;

    let fn_arg_fields: Vec<(String, String)> = ep
        .fields
        .iter()
        .filter_map(|f| match &f.source {
            FieldSource::FnArg(arg) => args.get(arg).map(|v| (f.name.clone(), v.clone())),
            _ => None,
        })
        .collect();
    if fn_arg_fields.is_empty() {
        return;
    }

    if let Some(rows) = result.get_mut("rows").and_then(|r| r.as_array_mut()) {
        for row in rows.iter_mut() {
            if let Some(obj) = row.as_object_mut() {
                for (name, val) in &fn_arg_fields {
                    obj.insert(name.clone(), serde_json::Value::String(val.clone()));
                }
            }
        }
    }
}

/// Append `params` to `url`'s query string, percent-encoding each pair.
fn append_query_params(url: &str, params: &[(String, String)]) -> String {
    if params.is_empty() {
        return url.to_string();
    }
    let (base, fragment) = match url.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (url, None),
    };
    let mut out = String::from(base);
    for (k, v) in params {
        out.push(if out.contains('?') { '&' } else { '?' });
        out.push_str(&urlencoding::encode(k));
        out.push('=');
        out.push_str(&urlencoding::encode(v));
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

fn unpack_manga_info(result: &serde_json::Value) -> Result<MangaInfo> {
    kani_shared::unpack::unpack_manga_info(result, &[])
        .map(Into::into)
        .map_err(Error::Extension)
}

fn unpack_chapter_list(
    result: &serde_json::Value,
    ep: &kani_yaml::ValidatedEndpoint,
) -> ChapterList {
    kani_shared::unpack::unpack_chapter_list(result, hnp_spec(ep), total_pages_spec(ep), &[]).into()
}

fn unpack_chapter(result: &serde_json::Value) -> Chapter {
    kani_shared::unpack::unpack_pages(result, &[]).into()
}

#[cfg(test)]
impl YamlSource {
    pub(crate) fn for_test() -> Self {
        let cache = Arc::new(kani_core::cache::InMemoryCache::new());
        Self {
            config: Arc::new(kani_yaml::ValidatedExtension::default()),
            http: kani_core::http::SmartClient::new(None).expect("SmartClient::new"),
            cache,
            cache_namespace: String::new(),
            v8_process: kani_core::v8_process::new_handle(),
            prefs: Arc::new(std::sync::RwLock::new(HashMap::new())),
            semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            hook_registry: None,
            pure_fn_registry: None,
            browser_scripts: Arc::new(kani_core::scripting::BrowserScriptRegistry::default()),
            browser_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            max_hook_requests: 3,
            eval_limits: kani_core::evaluator::EvalLimits::default(),
        }
    }
}

#[cfg(test)]
mod browser_pagination_tests {
    #![allow(clippy::unwrap_used)]
    use super::{PageWindow, merge_capture};

    fn window(page: usize, requested: usize) -> PageWindow {
        PageWindow::new(page, Some(requested), 28)
    }

    fn rows(from: usize, count: usize) -> serde_json::Value {
        serde_json::json!({
            "rows": (from..from + count).collect::<Vec<_>>(),
            "scalars": { "has_next_page": true, "total_pages": 10 },
        })
    }

    #[test]
    fn a_page_inside_one_capture_costs_one() {
        assert_eq!(window(1, 18).captures, 1);
        assert_eq!(window(1, 28).captures, 1);
    }

    #[test]
    fn a_page_straddling_two_captures_costs_two() {
        assert_eq!(window(1, 32).captures, 2);
        assert_eq!(window(2, 32).captures, 2);
    }

    #[test]
    fn an_extreme_page_size_is_capped_and_served_short() {
        let w = window(1, 10_000);
        assert_eq!(w.captures, 6);
        assert_eq!(w.take, 6 * 28);
    }

    #[test]
    fn consecutive_pages_tile_the_items_without_gap_or_overlap() {
        let mut seen: Vec<usize> = Vec::new();
        for page in 1..=3 {
            let w = window(page, 32);
            let mut merged = None;
            for capture in 0..w.captures {
                merge_capture(&mut merged, rows(w.native_index(capture) * 28, 28));
            }
            let mut merged = merged.unwrap();
            w.trim(&mut merged);
            let page_rows: Vec<usize> = serde_json::from_value(merged["rows"].clone()).unwrap();
            assert_eq!(
                page_rows.len(),
                32,
                "page {page} must hold what was asked for"
            );
            seen.extend(page_rows);
        }
        assert_eq!(seen, (0..96).collect::<Vec<_>>());
    }

    #[test]
    fn a_page_smaller_than_a_capture_still_advances_by_its_own_size() {
        let w = window(2, 12);
        let mut merged = None;
        for capture in 0..w.captures {
            merge_capture(&mut merged, rows(w.native_index(capture) * 28, 28));
        }
        let mut merged = merged.unwrap();
        w.trim(&mut merged);
        let page_rows: Vec<usize> = serde_json::from_value(merged["rows"].clone()).unwrap();
        assert_eq!(page_rows, (12..24).collect::<Vec<_>>());
    }

    #[test]
    fn rows_left_over_by_the_trim_are_reported_as_a_next_page() {
        let w = window(1, 32);
        let mut merged = None;
        merge_capture(&mut merged, rows(0, 28));
        merge_capture(
            &mut merged,
            serde_json::json!({
                "rows": (28..56).collect::<Vec<_>>(),
                "scalars": { "has_next_page": false, "total_pages": 2 },
            }),
        );
        let mut merged = merged.unwrap();
        w.trim(&mut merged);
        assert_eq!(merged["rows"].as_array().unwrap().len(), 32);
        assert_eq!(
            merged["scalars"]["has_next_page"],
            serde_json::json!(true),
            "the site is out of pages but 24 fetched rows have not been shown"
        );
    }

    #[test]
    fn the_sites_page_count_is_restated_in_the_pages_being_served() {
        let w = window(1, 14);
        let mut merged = Some(rows(0, 28));
        w.trim(merged.as_mut().unwrap());
        assert_eq!(
            merged.unwrap()["scalars"]["total_pages"],
            serde_json::json!(20),
            "10 site pages of 28 are 20 pages of 14"
        );
    }

    #[test]
    fn merging_accumulates_rows_and_takes_the_later_scalars() {
        let mut merged = None;
        merge_capture(
            &mut merged,
            serde_json::json!({ "rows": [1, 2], "has_next_page": true }),
        );
        merge_capture(
            &mut merged,
            serde_json::json!({ "rows": [3], "has_next_page": false }),
        );
        let merged = merged.unwrap();
        assert_eq!(merged["rows"], serde_json::json!([1, 2, 3]));
        assert_eq!(merged["has_next_page"], serde_json::json!(false));
    }
}

#[cfg(test)]
mod page_url_tests {
    use super::append_query_params;

    fn params(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn no_params_leaves_the_url_alone() {
        assert_eq!(
            append_query_params("https://x.test/browse", &[]),
            "https://x.test/browse"
        );
    }

    #[test]
    fn separator_follows_what_the_template_already_has() {
        assert_eq!(
            append_query_params("https://x.test/browse", &params(&[("page", "2")])),
            "https://x.test/browse?page=2"
        );
        assert_eq!(
            append_query_params("https://x.test/browse?q=a", &params(&[("page", "2")])),
            "https://x.test/browse?q=a&page=2"
        );
    }

    #[test]
    fn keys_and_values_are_encoded_and_repeat_keys_survive() {
        assert_eq!(
            append_query_params(
                "https://x.test/browse",
                &params(&[("genres_in[]", "23"), ("genres_in[]", "9"), ("q", "a b")])
            ),
            "https://x.test/browse?genres_in%5B%5D=23&genres_in%5B%5D=9&q=a%20b"
        );
    }

    #[test]
    fn params_land_before_a_fragment() {
        assert_eq!(
            append_query_params("https://x.test/browse#top", &params(&[("page", "2")])),
            "https://x.test/browse?page=2#top"
        );
    }
}
