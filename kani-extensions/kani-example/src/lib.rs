#[cfg(not(target_family = "wasm"))]
compile_error!(
    "kani-extensions/* are WASM-only -- build with `cargo run -p kani-cli -- build <name>`. \
     If a tool triggered this, it is using --workspace (or defaulting to it); scope it to \
     default-members instead -- clippy/nextest omit the flag, cargo-dist needs precise-builds."
);

use kani_shared::bindings::exports::kani::extension::manga_provider::Guest;
use kani_shared::{
    ExtensionMetadata, ExtensionResult, MangaExtension, MangaStatus, bindings, ext_version,
    to_shared_filters, types::ActiveFilter, wit_types,
};
use wit_types::{Chapter, ChapterList, MangaInfo, MangaList, PreferenceSpec};

kani_shared::guest_alloc!();

pub struct Example {
    _base_url: String,
}

impl Default for Example {
    fn default() -> Self {
        Self::new()
    }
}

impl Example {
    pub fn new() -> Self {
        Self {
            _base_url: "https://example.com".to_string(),
        }
    }

    pub fn metadata() -> ExtensionMetadata {
        ExtensionMetadata {
            id: "example".to_string(),
            name: "Example".to_string(),
            version: ext_version!("0.1.0"),
            base_url: "https://example.com".to_string(),
            language: "multi".to_string(),
            nsfw: false,
            unrestricted_http: false,
            mihon_source_id: None,
            rate_limit: None,
            ..Default::default()
        }
    }
}

impl Guest for Example {
    fn get_metadata() -> Result<String, wit_types::ExtensionError> {
        Ok(kani_shared::serde_json::to_string(&Example::metadata())
            .expect("ExtensionMetadata serializes to JSON"))
    }

    fn get_popular_manga(
        page: i32,
        page_size: i32,
        filters: Vec<wit_types::ActiveFilter>,
    ) -> Result<MangaList, wit_types::ExtensionError> {
        let shared = to_shared_filters(filters);
        get_extension()
            .get_popular_manga(page, page_size, &shared)
            .map_err(|e| e.into_wit())
    }

    fn search_manga(
        query: String,
        page: i32,
        page_size: i32,
        filters: Vec<wit_types::ActiveFilter>,
    ) -> Result<MangaList, wit_types::ExtensionError> {
        let shared = to_shared_filters(filters);
        get_extension()
            .search_manga(&query, page, page_size, &shared)
            .map_err(|e| e.into_wit())
    }

    fn get_filter_list() -> Result<wit_types::FilterList, wit_types::ExtensionError> {
        get_extension().get_filter_list().map_err(|e| e.into_wit())
    }

    fn get_fetched_option_sets() -> Result<String, wit_types::ExtensionError> {
        get_extension()
            .get_fetched_option_sets()
            .map_err(|e| e.into_wit())
    }

    fn get_manga_details(manga_id: String) -> Result<MangaInfo, wit_types::ExtensionError> {
        get_extension()
            .get_manga_details(&manga_id)
            .map_err(|e| e.into_wit())
    }

    fn get_chapter_list(
        manga_id: String,
        page: i32,
        page_size: Option<i32>,
        sort: Option<String>,
    ) -> Result<ChapterList, wit_types::ExtensionError> {
        get_extension()
            .get_chapter_list(&manga_id, page, page_size, sort)
            .map_err(|e| e.into_wit())
    }
    async fn get_chapter_list_stream(
        manga_id: String,
        sort: Option<String>,
    ) -> kani_shared::StreamReader<Result<wit_types::ChapterInfo, wit_types::ExtensionError>> {
        kani_shared::bridge_chapter_list_stream(get_extension(), manga_id, sort)
    }

    fn get_chapter_sort_list() -> Result<Vec<wit_types::SortOption>, wit_types::ExtensionError> {
        get_extension()
            .get_chapter_sort_list()
            .map_err(|e| e.into_wit())
    }

    fn get_pages(
        manga_id: String,
        chapter_id: String,
    ) -> Result<Chapter, wit_types::ExtensionError> {
        get_extension()
            .get_pages(&manga_id, &chapter_id)
            .map_err(|e| e.into_wit())
    }

    fn get_preferences() -> Result<Vec<PreferenceSpec>, wit_types::ExtensionError> {
        get_extension().get_preferences().map_err(|e| e.into_wit())
    }
    fn get_url(manga_id: String) -> Result<String, wit_types::ExtensionError> {
        get_extension().get_url(&manga_id).map_err(|e| e.into_wit())
    }
}

impl MangaExtension for Example {
    fn name(&self) -> &str {
        "Example"
    }

    fn get_popular_manga(
        &self,
        _page: i32,
        _page_size: i32,
        _filters: &[ActiveFilter],
    ) -> ExtensionResult<MangaList> {
        Ok(MangaList {
            manga: vec![],
            has_next_page: false,
            total_pages: None,
        })
    }

    fn search_manga(
        &self,
        _query: &str,
        _page: i32,
        _page_size: i32,
        _filters: &[ActiveFilter],
    ) -> ExtensionResult<MangaList> {
        Ok(MangaList {
            manga: vec![],
            has_next_page: false,
            total_pages: None,
        })
    }

    fn get_manga_details(&self, manga_id: &str) -> ExtensionResult<MangaInfo> {
        Ok(MangaInfo {
            id: manga_id.to_string(),
            title: "Example".to_string(),
            description: Some("Example".to_string()),
            status: MangaStatus::Ongoing,
            authors: vec![],
            artists: vec![],
            tags: vec![],
            cover_url: Some("https://example.com/cover.jpg".to_string()),
        })
    }

    fn get_chapter_list(
        &self,
        _manga_id: &str,
        _page: i32,
        _page_size: Option<i32>,
        _sort: Option<String>,
    ) -> ExtensionResult<ChapterList> {
        Ok(ChapterList {
            chapters: vec![],
            has_next_page: false,
            total_pages: None,
        })
    }

    fn get_pages(&self, _manga_id: &str, _chapter_id: &str) -> ExtensionResult<Chapter> {
        Ok(Chapter { pages: vec![] })
    }

    fn get_filter_list(&self) -> ExtensionResult<wit_types::FilterList> {
        Ok(wit_types::FilterList { filters: vec![] })
    }

    fn get_preferences(&self) -> ExtensionResult<Vec<PreferenceSpec>> {
        Ok(vec![])
    }

    fn get_chapter_sort_list(&self) -> ExtensionResult<Vec<wit_types::SortOption>> {
        Ok(vec![])
    }
}

use std::sync::OnceLock;

static EXTENSION: OnceLock<Example> = OnceLock::new();

fn get_extension() -> &'static Example {
    EXTENSION.get_or_init(Example::new)
}

bindings::export!(Example);
