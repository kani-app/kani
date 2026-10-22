//! WASM runtime and host ABI for extensions.

pub mod abi;
pub mod cache;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store};

use crate::error::Result;

/// Host-side bindings generated from the `kani-extension` WIT world.
pub mod bindings {
    wasmtime::component::bindgen!({
        path: "wit/kani.wit",
        world: "kani-extension",
        imports: {
            "kani:extension/http": async,
            "kani:extension/extraction": async,
            "kani:extension/cache": async,
            "kani:extension/scripting": async,
        },
        exports: {
            default: async
        },
        additional_derives: [serde::Serialize, serde::Deserialize]
    });
}

pub use bindings::KaniExtension;
pub use bindings::KaniExtensionPre;
pub use bindings::exports;
pub use bindings::kani;

/// Buffered HTTP response stored while adapting host requests to WIT values.
pub struct ResponseData {
    pub body: String,
    pub status: u16,
}

/// Parsed scraper document admitted into the host's synchronized handle store.
pub struct SafeHtml(pub scraper::Html);
// SAFETY: after parsing, the host performs only immutable queries against the scraper tree.
unsafe impl Send for SafeHtml {}
// SAFETY: after parsing, the host performs only immutable queries against the scraper tree.
unsafe impl Sync for SafeHtml {}

impl SafeHtml {
    pub(crate) fn parse_document(html: &str) -> Self {
        Self(scraper::Html::parse_document(html))
    }

    pub fn parse_fragment(html: &str) -> Self {
        Self(scraper::Html::parse_fragment(html))
    }
}

/// Shared, synchronized ownership of a parsed HTML document.
pub struct SendHtml(pub Arc<Mutex<SafeHtml>>);

impl SendHtml {
    pub(crate) fn parse_document(html: &str) -> Self {
        Self(Arc::new(Mutex::new(SafeHtml::parse_document(html))))
    }

    pub fn parse_fragment(html: &str) -> Self {
        Self(Arc::new(Mutex::new(SafeHtml::parse_fragment(html))))
    }
}

#[derive(Clone)]
/// A node handle that keeps its owning document alive.
pub struct StoredNode {
    pub doc: Arc<Mutex<SafeHtml>>,
    pub node_id: ego_tree::NodeId,
}

#[derive(Debug, Clone)]
/// Network authority granted to one extension instance.
pub enum AllowedHost {
    Restricted(String),
    Unrestricted,
    MetadataOnly,
}

impl AllowedHost {
    /// Enforces this policy for a resolved request host. Shared by the HTTP path
    /// (`HostState::check_allowed_url`) and the browser `page_url` check so both
    /// apply identical matching rules.
    pub fn allows_host(&self, host: &str) -> std::result::Result<(), String> {
        match self {
            AllowedHost::Restricted(allowed) => {
                if host != allowed {
                    Err(format!(
                        "Request blocked: extension may only contact '{}', got '{}'",
                        allowed, host
                    ))
                } else {
                    Ok(())
                }
            }
            AllowedHost::Unrestricted => Ok(()),
            AllowedHost::MetadataOnly => {
                Err("HTTP requests are not permitted on metadata-only instances.".into())
            }
        }
    }
}

use crate::http::SmartClient;

/// Maximum combined HTML, element-list, and JSON handles held by one guest store.
pub(crate) const MAX_HANDLES: usize = 10_000;

/// Per-instance host resources and capability state exposed through WIT imports.
/// Handle maps own guest-visible resources and are cleared between provider calls.
pub struct HostState {
    pub http_client: SmartClient,
    pub allowed_host: AllowedHost,
    pub next_doc_handle: i32,
    pub html_docs: HashMap<i32, StoredNode>,
    pub html_lists: HashMap<i32, Vec<StoredNode>>,
    pub json_docs: HashMap<i32, serde_json::Value>,
    pub selector_cache: std::sync::Mutex<HashMap<String, std::sync::Arc<scraper::Selector>>>,
    pub last_error: Option<i32>,
    pub preferences: std::collections::HashMap<String, String>,
    pub call_started_at: std::time::Instant,
    pub io_count: u32,
    pub last_io_at: Option<std::time::Instant>,
    /// Limits for each top-level operation, from the extension's `metadata.rate_limit`.
    pub operation_limits: crate::budget::OperationLimits,
    /// The budget shared by all work of the current top-level operation.
    pub operation_budget: std::sync::Arc<crate::budget::OperationBudget>,
    /// Cache backend shared across all calls for this source. The namespace prefix
    /// (`{extension_id}:{version}:{scope}:`) is resolved at construction time.
    pub ext_cache: std::sync::Arc<dyn crate::cache::CacheBackend>,
    pub ext_cache_namespace: String,
    /// Stable per-source key (the extension id) used to derive a dedicated
    /// Chromium `userDataDir`, so browser session state never leaks between
    /// sources. Derived from `ext_cache_namespace` at construction.
    pub browser_profile_key: String,
    /// Handle to the shared Node.js V8 subprocess. Lazy-spawned on first use.
    pub v8_process: crate::v8_process::V8ProcessHandle,
    /// Named `passPayload` init scripts, for the Rhai `capture_page_payload`
    /// hook binding. `None` when the source declares no browser scripts.
    pub browser_scripts: Option<std::sync::Arc<crate::scripting::BrowserScriptRegistry>>,
    /// Operator gate for this source's browser capability. When `false`, browser
    /// capture calls are rejected before any V8 dispatch. Defaults to `true`.
    pub browser_enabled: bool,
    /// Compiled Rhai pure-function registry for `.user.<name>()` DSL calls.
    /// `None` when the source has no `scripts.pure` block.
    pub pure_fn_registry: Option<std::sync::Arc<crate::scripting::PureFunctionRegistry>>,
    /// Compiled Rhai hook registry for `pre_request` / `on_status` hooks.
    /// `None` when the source has no hooks defined.
    pub hook_registry: Option<std::sync::Arc<crate::scripting::HookRegistry>>,
    pub max_hook_requests: u32,
    /// Per-eval resource budget (iterations, depth). Reset at the start of each blueprint eval.
    pub eval_budget: std::sync::Arc<crate::evaluator::shared::EvalBudget>,
}

impl StoredNode {
    /// Locks the document, wraps the node as an `ElementRef`, and calls `f`.
    /// Returns an error if the node is not an element or the lock is poisoned.
    pub(crate) fn with_element<T, F>(&self, f: F) -> std::result::Result<T, String>
    where
        F: FnOnce(scraper::ElementRef) -> std::result::Result<T, String>,
    {
        let guard = self.doc.lock().map_err(|_| "HTML document lock poisoned")?;
        let tree_node = guard
            .0
            .tree
            .get(self.node_id)
            .ok_or("HTML node no longer present in document tree")?;
        match scraper::ElementRef::wrap(tree_node) {
            Some(el) => f(el),
            None => Err("node is not an element".into()),
        }
    }

    /// Like [`Self::with_element`] but returns `Ok(None)` when the node is not an element,
    /// so callers that treat that as a non-error can propagate it cleanly.
    pub(crate) fn try_with_element<T, F>(&self, f: F) -> std::result::Result<Option<T>, String>
    where
        F: FnOnce(scraper::ElementRef) -> std::result::Result<Option<T>, String>,
    {
        let guard = self.doc.lock().map_err(|_| "HTML document lock poisoned")?;
        let tree_node = guard
            .0
            .tree
            .get(self.node_id)
            .ok_or("HTML node no longer present in document tree")?;
        Ok(match scraper::ElementRef::wrap(tree_node) {
            Some(el) => f(el)?,
            None => None,
        })
    }
}

impl HostState {
    pub fn new(
        http_client: SmartClient,
        allowed_host: AllowedHost,
        ext_cache: std::sync::Arc<dyn crate::cache::CacheBackend>,
        ext_cache_namespace: String,
        v8_process: crate::v8_process::V8ProcessHandle,
    ) -> Result<Self> {
        let allowed_host = match allowed_host {
            AllowedHost::Restricted(raw) => {
                let host = raw
                    .parse::<url::Url>()
                    .ok()
                    .and_then(|u| u.host_str().map(|h| h.to_string()))
                    .ok_or_else(|| {
                        crate::error::Error::Internal(format!(
                            "Cannot derive hostname from base_url '{}' for HTTP restriction",
                            raw
                        ))
                    })?;
                AllowedHost::Restricted(host)
            }
            other => other,
        };

        let browser_profile_key = ext_cache_namespace
            .split(':')
            .next()
            .unwrap_or_default()
            .to_string();

        Ok(Self {
            http_client,
            allowed_host,
            next_doc_handle: 1,
            html_docs: HashMap::new(),
            html_lists: HashMap::new(),
            json_docs: HashMap::new(),
            selector_cache: std::sync::Mutex::new(HashMap::new()),
            last_error: None,
            preferences: HashMap::new(),
            call_started_at: std::time::Instant::now(),
            io_count: 0,
            last_io_at: None,
            operation_limits: crate::budget::OperationLimits::default(),
            operation_budget: crate::budget::OperationBudget::new(
                crate::budget::OperationLimits::default(),
            ),
            ext_cache,
            ext_cache_namespace,
            browser_profile_key,
            v8_process,
            browser_scripts: None,
            browser_enabled: true,
            pure_fn_registry: None,
            hook_registry: None,
            max_hook_requests: 3,
            eval_budget: std::sync::Arc::new(crate::evaluator::shared::EvalBudget::new()),
        })
    }

    /// Replace the evaluator budget with one built from custom limits. Test seam
    /// for tripping a cap (list size, string length, iterations, depth) without a
    /// giant fixture; production keeps [`crate::evaluator::EvalLimits::default`].
    pub fn set_eval_limits(&mut self, limits: crate::evaluator::EvalLimits) {
        self.eval_budget =
            std::sync::Arc::new(crate::evaluator::shared::EvalBudget::with_limits(limits));
    }

    pub fn clear_all(&mut self) {
        self.html_docs.clear();
        self.html_lists.clear();
        self.json_docs.clear();
        self.next_doc_handle = 1;
    }

    pub(crate) fn charge_io(&mut self) -> std::result::Result<(), String> {
        self.io_count += 1;
        self.operation_budget.charge_request()
    }

    pub(crate) fn charge_response_bytes(&self, len: usize) -> std::result::Result<(), String> {
        self.operation_budget.charge_bytes(len)
    }

    /// Starts a new top-level operation: a fresh budget under this source's limits.
    pub fn begin_operation(&mut self) {
        self.call_started_at = std::time::Instant::now();
        self.io_count = 0;
        self.last_io_at = None;
        self.operation_budget = crate::budget::OperationBudget::new(self.operation_limits);
    }

    /// Returns an error string if the total live handle count is at or above
    /// [`MAX_HANDLES`].
    pub(crate) fn check_handle_capacity(&self) -> std::result::Result<(), String> {
        let total = self.html_docs.len() + self.html_lists.len() + self.json_docs.len();
        if total >= MAX_HANDLES {
            Err(format!(
                "handle limit reached ({MAX_HANDLES}): extension is leaking document handles"
            ))
        } else {
            Ok(())
        }
    }

    /// Enforces the http/https rule and the `AllowedHost` policy for a request URL.
    pub(crate) fn check_allowed_url(&self, url: &url::Url) -> std::result::Result<(), String> {
        crate::network::require_http_url(url.as_str())?;
        self.allowed_host.allows_host(url.host_str().unwrap_or(""))
    }

    pub(crate) fn get_json(&self, handle: i32) -> std::result::Result<&serde_json::Value, String> {
        self.json_docs
            .get(&handle)
            .ok_or_else(|| "Invalid JSON handle".to_string())
    }

    pub(crate) fn get_html_doc(&self, handle: i32) -> std::result::Result<&StoredNode, String> {
        self.html_docs
            .get(&handle)
            .ok_or_else(|| "Document not found".to_string())
    }

    /// Returns a reference to the compiled selector, parsing and caching it on
    /// the first call for a given `selector` string.
    pub(crate) fn get_or_parse_selector(&mut self, selector: &str) -> Result<&scraper::Selector> {
        let cache = self
            .selector_cache
            .get_mut()
            .map_err(|_| crate::error::Error::Internal("selector cache lock poisoned".into()))?;
        if !cache.contains_key(selector) {
            let parsed = scraper::Selector::parse(selector)
                .map_err(|e| crate::error::Error::Internal(format!("Invalid selector: {:?}", e)))?;
            cache.insert(selector.to_string(), std::sync::Arc::new(parsed));
        }
        Ok(&**cache.get(selector).expect("selector was just inserted"))
    }
}

impl Default for HostState {
    fn default() -> Self {
        HostState::new(
            SmartClient::new(None).expect("failed to build default SmartClient"),
            AllowedHost::MetadataOnly,
            Arc::new(crate::cache::InMemoryCache::new()),
            String::new(),
            crate::v8_process::new_handle(),
        )
        .expect("failed to construct default HostState")
    }
}

/// WASM runtime wrapper with async support enabled.
pub struct WasmRuntime {
    engine: Engine,
    linker: Linker<HostState>,
    module_cache: Option<std::sync::Mutex<cache::WasmModuleCache>>,
}

impl WasmRuntime {
    /// Enable the compiled-module cache, rooted at the instance's data
    /// directory. Returns the directory and error if it could not be created,
    /// so the caller can report it rather than silently running without a cache.
    pub fn attach_module_cache(
        &mut self,
        data_dir: &std::path::Path,
    ) -> std::result::Result<(), (std::path::PathBuf, std::io::Error)> {
        let cache = cache::WasmModuleCache::open(data_dir)?;
        self.module_cache = Some(std::sync::Mutex::new(cache));
        Ok(())
    }

    pub fn new(max_instances: u32) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.concurrency_support(true);
        config.epoch_interruption(true);

        let mut pool = wasmtime::PoolingAllocationConfig::default();
        pool.total_component_instances(max_instances);

        config.allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling(pool));

        Self::new_with_config(config)
    }

    pub fn new_on_demand() -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.concurrency_support(true);
        config.epoch_interruption(true);

        Self::new_with_config(config)
    }

    fn new_with_config(config: Config) -> Result<Self> {
        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);

        KaniExtension::add_to_linker::<_, HasSelf<HostState>>(&mut linker, |state| state)?;

        Ok(Self {
            engine,
            linker,
            module_cache: None,
        })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Compiles a WASM component from bytes, consulting the on-disk module cache first.
    pub fn compile_component(&self, bytes: &[u8]) -> Result<Component> {
        let sha256 = cache::WasmModuleCache::sha256_hex(bytes);

        if let Some(mc) = &self.module_cache {
            let cached = mc
                .lock()
                .map_err(|_| {
                    crate::error::Error::Internal("wasm module cache lock poisoned".into())
                })?
                .try_get(&self.engine, &sha256)?;
            if let Some(c) = cached {
                return Ok(c);
            }
        }

        let component = Component::new(&self.engine, bytes)?;

        if let Some(mc) = &self.module_cache
            && let Ok(mut guard) = mc.lock()
        {
            guard.insert(&sha256, component.clone());
        }

        Ok(component)
    }

    pub fn prune_module_cache(&self, live_hashes: &std::collections::HashSet<String>) {
        if let Some(mc) = &self.module_cache
            && let Ok(guard) = mc.lock()
        {
            guard.prune(live_hashes);
        }
    }

    pub fn create_store(&self) -> Store<HostState> {
        Store::new(&self.engine, HostState::default())
    }

    pub async fn instantiate(
        &self,
        store: &mut Store<HostState>,
        component: &Component,
    ) -> Result<KaniExtension> {
        let binding = KaniExtension::instantiate_async(store, component, &self.linker).await?;
        Ok(binding)
    }

    /// Pre-links a component's imports so instances can be created cheaply.
    pub fn instantiate_pre(&self, component: &Component) -> Result<KaniExtensionPre<HostState>> {
        let instance_pre = self.linker.instantiate_pre(component)?;
        let pre = KaniExtensionPre::new(instance_pre)?;
        Ok(pre)
    }

    pub fn linker(&self) -> &Linker<HostState> {
        &self.linker
    }
}

impl std::fmt::Debug for WasmRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmRuntime")
            .field("engine", &"<wasmtime::Engine>")
            .finish()
    }
}

pub mod filter_conversions {
    use crate::wasm::kani::extension::types as wit;
    use kani_shared::types::{ActiveFilter, FilterState};

    impl From<wit::FilterState> for FilterState {
        fn from(s: wit::FilterState) -> Self {
            match s {
                wit::FilterState::Checkbox(c) => FilterState::Checkbox(c),
                wit::FilterState::TextInput(t) => FilterState::TextInput(t),
                wit::FilterState::Selection(opt) => FilterState::Selection {
                    name: opt.name,
                    value: opt.value,
                },
                wit::FilterState::Multiselect(values) => FilterState::Multiselect(values),
            }
        }
    }

    impl From<FilterState> for wit::FilterState {
        fn from(s: FilterState) -> Self {
            match s {
                FilterState::Checkbox(c) => Self::Checkbox(c),
                FilterState::TextInput(t) => Self::TextInput(t),
                FilterState::Selection { name, value } => {
                    Self::Selection(wit::OptionState { name, value })
                }
                FilterState::Multiselect(values) => Self::Multiselect(values),
            }
        }
    }

    pub fn to_wit_active_filters(filters: &[ActiveFilter]) -> Vec<wit::ActiveFilter> {
        filters
            .iter()
            .map(|f| wit::ActiveFilter {
                filter_name: f.filter_name.clone(),
                state: f.state.clone().into(),
            })
            .collect()
    }
}

/// Convert the shared (wit-bindgen) result types the interpreted `YamlSource`
/// produces via `kani_shared::unpack::*` into the wasmtime component types
/// `SourceBackend` returns. The two are structurally identical but distinct Rust
/// types (wasmtime bindgen vs wit-bindgen); this is the one-way host-side hop.
pub mod result_conversions {
    use crate::wasm::kani::extension::types as wit;
    use kani_shared::wit_types as gw;

    impl From<kani_shared::types::MangaStatus> for wit::MangaStatus {
        fn from(s: kani_shared::types::MangaStatus) -> Self {
            use kani_shared::types::MangaStatus as S;
            match s {
                S::Ongoing => wit::MangaStatus::Ongoing,
                S::Completed => wit::MangaStatus::Completed,
                S::Hiatus => wit::MangaStatus::Hiatus,
                S::Cancelled => wit::MangaStatus::Cancelled,
                S::Unknown => wit::MangaStatus::Unknown,
            }
        }
    }

    impl From<gw::MangaListItem> for wit::MangaListItem {
        fn from(m: gw::MangaListItem) -> Self {
            wit::MangaListItem {
                id: m.id,
                title: m.title,
                cover_url: m.cover_url,
            }
        }
    }

    impl From<gw::MangaList> for wit::MangaList {
        fn from(m: gw::MangaList) -> Self {
            wit::MangaList {
                manga: m.manga.into_iter().map(Into::into).collect(),
                has_next_page: m.has_next_page,
                total_pages: m.total_pages,
            }
        }
    }

    impl From<gw::MangaInfo> for wit::MangaInfo {
        fn from(m: gw::MangaInfo) -> Self {
            wit::MangaInfo {
                id: m.id,
                title: m.title,
                cover_url: m.cover_url,
                description: m.description,
                authors: m.authors,
                artists: m.artists,
                status: m.status.into(),
                tags: m.tags,
            }
        }
    }

    impl From<gw::ChapterInfo> for wit::ChapterInfo {
        fn from(c: gw::ChapterInfo) -> Self {
            wit::ChapterInfo {
                id: c.id,
                number: c.number,
                title: c.title,
                volume: c.volume,
                scanlator: c.scanlator,
                date_uploaded: c.date_uploaded,
                language: c.language,
                page_count: c.page_count,
            }
        }
    }

    impl From<gw::ChapterList> for wit::ChapterList {
        fn from(c: gw::ChapterList) -> Self {
            wit::ChapterList {
                chapters: c.chapters.into_iter().map(Into::into).collect(),
                has_next_page: c.has_next_page,
                total_pages: c.total_pages,
            }
        }
    }

    impl From<gw::Page> for wit::Page {
        fn from(p: gw::Page) -> Self {
            wit::Page {
                index: p.index,
                url: p.url,
                transform: p.transform,
            }
        }
    }

    impl From<gw::Chapter> for wit::Chapter {
        fn from(c: gw::Chapter) -> Self {
            wit::Chapter {
                pages: c.pages.into_iter().map(Into::into).collect(),
            }
        }
    }
}

/// Reached from other crates only through the `$crate::` path inside
/// `sources.rs`'s macros, so it must stay `pub` despite no direct caller naming it.
/// Converts a guest's error for the host: an exhausted operation budget becomes
/// [`crate::error::Error::BudgetExceeded`] whichever path raised it, anything else stays an
/// extension error.
pub fn guest_error(e: kani::extension::types::ExtensionError) -> crate::error::Error {
    let ext = ext_error_from_wit(e);
    if crate::budget::is_budget_exceeded(&ext.message) {
        crate::error::Error::BudgetExceeded(ext.message)
    } else if let Some(target) = ext.message.strip_prefix("__refresh_auth__:") {
        crate::error::Error::Extension(kani_shared::extension::ExtensionError::auth(format!(
            "a hook asked to refresh auth through {target:?}, but refresh_auth is only supported \
             in interpreted YAML sources"
        )))
    } else {
        crate::error::Error::Extension(ext)
    }
}

pub fn ext_error_from_wit(
    e: kani::extension::types::ExtensionError,
) -> kani_shared::extension::ExtensionError {
    use kani::extension::types::ExtensionErrorKind as WitKind;
    use kani_shared::extension::ExtensionErrorKind;
    let kind = match e.kind {
        WitKind::Network => ExtensionErrorKind::Network,
        WitKind::Parse => ExtensionErrorKind::Parse,
        WitKind::NotFound => ExtensionErrorKind::NotFound,
        WitKind::RateLimited => ExtensionErrorKind::RateLimited,
        WitKind::Auth => ExtensionErrorKind::Auth,
        WitKind::ContentUnavailable => ExtensionErrorKind::ContentUnavailable,
        WitKind::Timeout => ExtensionErrorKind::Timeout,
        WitKind::InvalidInput => ExtensionErrorKind::InvalidInput,
        WitKind::Internal => ExtensionErrorKind::Internal,
        WitKind::Unknown => ExtensionErrorKind::Unknown,
        WitKind::SourceUpdating => ExtensionErrorKind::Updating,
    };
    kani_shared::extension::ExtensionError {
        kind,
        message: e.message,
        source_url: e.source_url.map(|u| redact_url(&u)),
        retry_after_secs: e.retry_after_secs,
    }
}

fn redact_url(url: &str) -> String {
    let url = strip_userinfo(url);
    strip_sensitive_params(&url)
}

fn strip_userinfo(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        let host_end = after_scheme.find('/').unwrap_or(after_scheme.len());
        let authority = &after_scheme[..host_end];
        if let Some(at_pos) = authority.rfind('@') {
            let host = &authority[at_pos + 1..];
            let rest = &after_scheme[host_end..];
            return format!("{}{}{}", &url[..scheme_end + 3], host, rest);
        }
    }
    url.to_string()
}

fn strip_sensitive_params(url: &str) -> String {
    const SENSITIVE: &[&str] = &["token", "api_key", "session", "password", "signature"];
    let (base, fragment) = match url.find('#') {
        Some(p) => (&url[..p], Some(&url[p..])),
        None => (url, None),
    };
    let Some(q_pos) = base.find('?') else {
        return url.to_string();
    };
    let path = &base[..q_pos];
    let query = &base[q_pos + 1..];
    let filtered: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            let key = pair.split('=').next().unwrap_or("");
            !SENSITIVE.iter().any(|s| key.eq_ignore_ascii_case(s))
        })
        .collect();
    let result = if filtered.is_empty() {
        path.to_string()
    } else {
        format!("{}?{}", path, filtered.join("&"))
    };
    match fragment {
        Some(f) => format!("{}{}", result, f),
        None => result,
    }
}
