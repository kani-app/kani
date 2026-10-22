//! Unified source backend over compiled WASM components and interpreted YAML extensions.

pub mod loader;
pub mod registry;
pub mod signing;
pub mod wasm_source;
pub mod yaml_source;

pub use registry::SourceRegistry;
pub use wasm_source::WasmSource;
pub use yaml_source::YamlSource;

/// Installed source implementation with behaviorally equivalent WASM and YAML dispatch paths.
pub enum SourceBackend {
    Wasm(Box<WasmSource>),
    Yaml(Box<YamlSource>),
}

impl SourceBackend {
    pub fn backend_kind(&self) -> &'static str {
        match self {
            Self::Wasm(_) => "wasm",
            Self::Yaml(_) => "yaml",
        }
    }

    pub fn update_preferences(&self, prefs: std::collections::HashMap<String, String>) {
        match self {
            Self::Wasm(w) => w.update_preferences(prefs),
            Self::Yaml(y) => y.update_preferences(prefs),
        }
    }

    /// Kills this source's browser subprocess if it has been idle for at least
    /// `idle_for`. Returns `true` when a process was reaped.
    pub(crate) async fn reap_idle_v8(&self, idle_for: std::time::Duration) -> bool {
        match self {
            Self::Wasm(w) => w.reap_idle_v8(idle_for).await,
            Self::Yaml(y) => y.reap_idle_v8(idle_for).await,
        }
    }

    pub(crate) async fn shutdown_v8(&self, reason: &str) -> bool {
        match self {
            Self::Wasm(w) => w.shutdown_v8(reason).await,
            Self::Yaml(y) => y.shutdown_v8(reason).await,
        }
    }

    pub(crate) async fn retire_v8(&self, reason: &str) -> bool {
        match self {
            Self::Wasm(w) => w.retire_v8(reason).await,
            Self::Yaml(y) => y.retire_v8(reason).await,
        }
    }

    /// Flips the operator gate for this source's browser capability. Takes effect
    /// on the next browser call (WASM) or the next browser endpoint eval (YAML).
    pub fn set_browser_enabled(&self, enabled: bool) {
        match self {
            Self::Wasm(w) => w.set_browser_enabled(enabled),
            Self::Yaml(y) => y.set_browser_enabled(enabled),
        }
    }

    pub fn extension_id(&self) -> &str {
        match self {
            Self::Wasm(w) => w.extension_id(),
            Self::Yaml(y) => y.config.id.as_str(),
        }
    }

    pub async fn get_metadata(&self) -> kani_core::error::Result<String> {
        let start = std::time::Instant::now();
        let result = async {
            match self {
                Self::Wasm(w) => w.lease_instance().await?.get_metadata().await,
                Self::Yaml(y) => y.get_metadata().await,
            }
        }
        .await;
        record_call(self.extension_id(), "get_metadata", start, &result);
        result
    }

    pub async fn get_popular_manga(
        &self,
        page: i32,
        page_size: i32,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> kani_core::error::Result<kani_core::wasm::kani::extension::types::MangaList> {
        let start = std::time::Instant::now();
        let result = async {
            match self {
                Self::Wasm(w) => {
                    w.lease_instance()
                        .await?
                        .get_popular_manga(page, page_size, filters)
                        .await
                }
                Self::Yaml(y) => y.get_popular_manga(page, page_size, filters).await,
            }
        }
        .await;
        record_call(self.extension_id(), "get_popular_manga", start, &result);
        result
    }

    pub async fn search_manga(
        &self,
        query: &str,
        page: i32,
        page_size: i32,
        filters: &[kani_shared::types::ActiveFilter],
    ) -> kani_core::error::Result<kani_core::wasm::kani::extension::types::MangaList> {
        let start = std::time::Instant::now();
        let result = async {
            match self {
                Self::Wasm(w) => {
                    w.lease_instance()
                        .await?
                        .search_manga(query, page, page_size, filters)
                        .await
                }
                Self::Yaml(y) => y.search_manga(query, page, page_size, filters).await,
            }
        }
        .await;
        record_call(self.extension_id(), "search_manga", start, &result);
        result
    }

    pub async fn get_filter_list_with_options(
        &self,
    ) -> kani_core::error::Result<(kani_core::WitFilterList, String)> {
        let start = std::time::Instant::now();
        let result = async {
            match self {
                Self::Wasm(w) => {
                    let mut instance = w.lease_instance().await?;
                    let filter_list = instance.get_filter_list().await?;
                    let fetched = instance
                        .get_fetched_option_sets()
                        .await
                        .unwrap_or_else(|_| "[]".to_string());
                    Ok((filter_list, fetched))
                }
                Self::Yaml(y) => {
                    let filter_list = y.get_filter_list().await?;
                    let fetched = y
                        .get_fetched_option_sets()
                        .await
                        .unwrap_or_else(|_| "[]".to_string());
                    Ok((filter_list, fetched))
                }
            }
        }
        .await;
        record_call(self.extension_id(), "get_filter_list", start, &result);
        result
    }

    pub async fn get_source_url(&self, manga_id: &str) -> kani_core::error::Result<String> {
        let start = std::time::Instant::now();
        let result = async {
            match self {
                Self::Wasm(w) => w.lease_instance().await?.get_url(manga_id).await,
                Self::Yaml(y) => y.get_source_url(manga_id).await,
            }
        }
        .await;
        record_call(self.extension_id(), "get_source_url", start, &result);
        result
    }

    pub async fn get_manga_details(
        &self,
        manga_id: &str,
    ) -> kani_core::error::Result<kani_core::wasm::kani::extension::types::MangaInfo> {
        let start = std::time::Instant::now();
        let result = async {
            match self {
                Self::Wasm(w) => w.lease_instance().await?.get_manga_details(manga_id).await,
                Self::Yaml(y) => y.get_manga_details(manga_id).await,
            }
        }
        .await;
        record_call(self.extension_id(), "get_manga_details", start, &result);
        result
    }

    pub async fn get_pages(
        &self,
        manga_id: &str,
        chapter_id: &str,
    ) -> kani_core::error::Result<kani_core::wasm::kani::extension::types::Chapter> {
        let start = std::time::Instant::now();
        let result = async {
            match self {
                Self::Wasm(w) => {
                    w.lease_instance()
                        .await?
                        .get_pages(manga_id, chapter_id)
                        .await
                }
                Self::Yaml(y) => y.get_pages(manga_id, chapter_id).await,
            }
        }
        .await;
        record_call(self.extension_id(), "get_pages", start, &result);
        result
    }

    pub async fn get_chapter_list(
        &self,
        manga_id: &str,
        page: i32,
        page_size: Option<i32>,
        sort: Option<String>,
    ) -> kani_core::error::Result<kani_core::wasm::kani::extension::types::ChapterList> {
        let start = std::time::Instant::now();
        let result = async {
            match self {
                Self::Wasm(w) => {
                    w.lease_instance()
                        .await?
                        .get_chapter_list(manga_id, page, page_size, sort)
                        .await
                }
                Self::Yaml(y) => y.get_chapter_list(manga_id, page, page_size, sort).await,
            }
        }
        .await;
        record_call(self.extension_id(), "get_chapter_list", start, &result);
        result
    }

    pub async fn get_chapter_sort_list(
        &self,
    ) -> kani_core::error::Result<Vec<kani_core::wasm::kani::extension::types::SortOption>> {
        let start = std::time::Instant::now();
        let result = async {
            match self {
                Self::Wasm(w) => w.lease_instance().await?.get_chapter_sort_list().await,
                Self::Yaml(y) => y.get_chapter_sort_list().await,
            }
        }
        .await;
        record_call(self.extension_id(), "get_chapter_sort_list", start, &result);
        result
    }

    pub async fn get_preferences(
        &self,
    ) -> kani_core::error::Result<Vec<kani_core::wasm::kani::extension::types::PreferenceSpec>>
    {
        let start = std::time::Instant::now();
        let result = async {
            match self {
                Self::Wasm(w) => w.lease_instance().await?.get_preferences().await,
                Self::Yaml(y) => y.get_preferences().await,
            }
        }
        .await;
        record_call(self.extension_id(), "get_preferences", start, &result);
        result
    }
}

fn record_call<T>(
    extension: &str,
    method: &'static str,
    start: std::time::Instant,
    result: &kani_core::error::Result<T>,
) {
    let ext = extension.to_string();
    metrics::counter!("kani_wasm_calls_total", "extension" => ext.clone(), "method" => method)
        .increment(1);
    metrics::histogram!("kani_wasm_call_duration_seconds", "extension" => ext.clone(), "method" => method)
        .record(start.elapsed().as_secs_f64());
    if result.is_err() {
        metrics::counter!("kani_wasm_call_errors_total", "extension" => ext, "method" => method)
            .increment(1);
    }
}

#[cfg(any(test, feature = "test-util"))]
impl SourceBackend {
    pub fn is_yaml(&self) -> bool {
        matches!(self, Self::Yaml(_))
    }
}

#[async_trait::async_trait]
impl kani_core::downloader::PageListFetcher for SourceBackend {
    async fn fetch_page_list(
        &self,
        manga_id: &str,
        chapter_id: &str,
    ) -> kani_core::error::Result<(kani_core::wasm::kani::extension::types::Chapter, String)> {
        match self {
            Self::Wasm(w) => w.fetch_page_list_impl(manga_id, chapter_id).await,
            Self::Yaml(y) => y.fetch_page_list(manga_id, chapter_id).await,
        }
    }
}
