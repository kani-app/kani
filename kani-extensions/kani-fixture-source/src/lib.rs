#[cfg(not(target_family = "wasm"))]
compile_error!(
    "kani-extensions/* are WASM-only -- build with `cargo run -p kani-cli -- build <name>`. \
     If a tool triggered this, it is using --workspace (or defaulting to it); scope it to \
     default-members instead -- clippy/nextest omit the flag, cargo-dist needs precise-builds."
);

use kani_shared::ast::{BlueprintBuilder, Expr};
use kani_shared::bindings::exports::kani::extension::manga_provider::Guest;
use kani_shared::host_abi::{HttpRequest, extract, prefs};
use kani_shared::wit_types::ChapterInfo;
use kani_shared::{
    ExtensionMetadata, ExtensionResult, MangaExtension, MangaStatus, bindings, ext_version,
    to_shared_filters, types::ActiveFilter, wit_types,
};
use wit_types::{Chapter, ChapterList, MangaInfo, MangaList, MangaListItem, Page, PreferenceSpec};

kani_shared::guest_alloc!();

const BASE_URL_PREF: &str = "base_url";
const DEFAULT_BASE_URL: &str = "https://fixture.invalid";

pub struct Fixture;

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Fixture {
    pub fn new() -> Self {
        Self
    }

    /// The origin to fetch from, injected by the host as a preference. This is
    /// what lets a test aim the source at a `TestOrigin` on an ephemeral port.
    fn base_url(&self) -> String {
        prefs::get_str_or(BASE_URL_PREF, DEFAULT_BASE_URL)
            .trim_end_matches('/')
            .to_string()
    }

    pub fn metadata() -> ExtensionMetadata {
        ExtensionMetadata {
            id: "fixture".to_string(),
            name: "Fixture Source".to_string(),
            version: ext_version!("0.1.0"),
            base_url: DEFAULT_BASE_URL.to_string(),
            language: "en".to_string(),
            nsfw: false,
            unrestricted_http: true,
            mihon_source_id: None,
            rate_limit: Some(kani_shared::extension::RateLimitConfig {
                requests_per_second: 5.0,
                burst: 1,
                max_concurrent: 1,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn fetch_list(&self, req: HttpRequest) -> ExtensionResult<MangaList> {
        let bp = BlueprintBuilder::new(".item")
            .request(req)
            .field("id", Expr::self_ref().attr("data-id"))
            .field("title", Expr::self_ref().first(".title").text())
            .build();

        let rows = extract::html(None, &bp)?;
        let manga = (0..rows.rows_len())
            .filter_map(|i| {
                let row = rows.rows_get(i).ok()?;
                Some(MangaListItem {
                    id: row.require_str("/id").ok()?,
                    title: row.require_str("/title").ok()?,
                    cover_url: None,
                })
            })
            .collect();

        Ok(MangaList {
            manga,
            has_next_page: false,
            total_pages: None,
        })
    }
}

impl MangaExtension for Fixture {
    fn name(&self) -> &str {
        "Fixture Source"
    }

    fn get_popular_manga(
        &self,
        _page: i32,
        _page_size: i32,
        _filters: &[ActiveFilter],
    ) -> ExtensionResult<MangaList> {
        self.fetch_list(HttpRequest::get(format!("{}/popular", self.base_url())))
    }

    fn search_manga(
        &self,
        query: &str,
        _page: i32,
        _page_size: i32,
        filters: &[ActiveFilter],
    ) -> ExtensionResult<MangaList> {
        if let Some(n) = query.strip_prefix("__fanout__:")
            && let Ok(n) = n.parse::<i32>()
        {
            for i in 0..n {
                let bp = BlueprintBuilder::new(".item")
                    .request(HttpRequest::get(format!("{}/fanout/{i}", self.base_url())))
                    .field("id", Expr::self_ref().attr("data-id"))
                    .build();
                // Propagates the budget error the host raises once the cap is hit.
                extract::html(None, &bp)?;
            }
            return Ok(MangaList {
                manga: vec![],
                has_next_page: false,
                total_pages: None,
            });
        }

        let mut req = HttpRequest::get(format!("{}/search", self.base_url())).query("q", query);
        // Map a selected `genre` onto `g`, mirroring the interpreted source's
        // filter_mapping. A guest that renders the panel but drops the selection
        // is the silent-wrong failure this exists to catch.
        for f in filters {
            if f.filter_name == "genre"
                && let kani_shared::types::FilterState::Selection { value, .. } = &f.state
            {
                req = req.query("g", value);
            }
        }
        self.fetch_list(req)
    }

    /// Conformance affordance: a manga id of the form `__composite__:<encoded>`
    /// carries a base64url `series|volume` pair the guest must decode to build
    /// the detail request. Proves a composite id survives the round trip out to
    /// the host and back into a live call.
    fn get_manga_details(&self, manga_id: &str) -> ExtensionResult<MangaInfo> {
        if let Some(encoded) = manga_id.strip_prefix("__composite__:") {
            let parts = kani_shared::encoding::decode_composite(
                encoded,
                "|",
                &kani_shared::ast::IdEncoding::Base64Url,
                &["series", "volume"],
            )
            .map_err(kani_shared::extension::ExtensionError::parse)?;
            let get = |k: &str| {
                parts
                    .iter()
                    .find(|(name, _)| name == k)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            let bp = BlueprintBuilder::new(".manga")
                .request(HttpRequest::get(format!(
                    "{}/series/{}/vol/{}",
                    self.base_url(),
                    get("series"),
                    get("volume")
                )))
                .field("id", Expr::self_ref().attr("data-id"))
                .field("title", Expr::self_ref().first("h1").text())
                .build();
            let handle = extract::html(None, &bp)?;
            let row = handle.rows_get(0)?;
            return Ok(MangaInfo {
                id: manga_id.to_string(),
                title: row.require_str("/title")?,
                description: None,
                status: MangaStatus::Unknown,
                authors: vec![],
                artists: vec![],
                tags: vec![],
                cover_url: None,
            });
        }

        let bp = BlueprintBuilder::new(".manga")
            .request(HttpRequest::get(format!(
                "{}/manga/{}",
                self.base_url(),
                manga_id
            )))
            .field("id", Expr::self_ref().attr("data-id"))
            .field("title", Expr::self_ref().first("h1").text())
            .build();

        let rows = extract::html(None, &bp)?;
        let row = rows
            .rows_get(0)
            .map_err(|_| kani_shared::ExtensionError::parse("no details row".into()))?;

        Ok(MangaInfo {
            id: manga_id.to_string(),
            title: row.require_str("/title")?,
            description: None,
            status: MangaStatus::Unknown,
            authors: vec![],
            artists: vec![],
            tags: vec![],
            cover_url: None,
        })
    }

    fn get_chapter_list(
        &self,
        manga_id: &str,
        _page: i32,
        _page_size: Option<i32>,
        _sort: Option<String>,
    ) -> ExtensionResult<ChapterList> {
        let bp = BlueprintBuilder::new(".ch")
            .request(HttpRequest::get(format!(
                "{}/manga/{}/chapters",
                self.base_url(),
                manga_id
            )))
            .field("id", Expr::self_ref().attr("data-id"))
            .field("title", Expr::self_ref().first(".title").text())
            .build();

        let rows = extract::html(None, &bp)?;
        let chapters = (0..rows.rows_len())
            .filter_map(|i| {
                let row = rows.rows_get(i).ok()?;
                Some(ChapterInfo {
                    id: row.require_str("/id").ok()?,
                    number: 0.0,
                    title: row.get_str("/title"),
                    volume: None,
                    scanlator: None,
                    date_uploaded: None,
                    language: "en".to_string(),
                    page_count: None,
                })
            })
            .collect();

        Ok(ChapterList {
            chapters,
            has_next_page: false,
            total_pages: None,
        })
    }

    fn get_pages(&self, manga_id: &str, chapter_id: &str) -> ExtensionResult<Chapter> {
        let bp = BlueprintBuilder::new(".page")
            .request(HttpRequest::get(format!(
                "{}/manga/{}/chapter/{}",
                self.base_url(),
                manga_id,
                chapter_id
            )))
            .field("url", Expr::self_ref().attr("data-url"))
            .build();

        let rows = extract::html(None, &bp)?;
        Ok(Chapter {
            pages: (0..rows.rows_len())
                .filter_map(|i| {
                    let row = rows.rows_get(i).ok()?;
                    Some(Page {
                        index: i as i32,
                        url: row.require_str("/url").ok()?,
                        transform: None,
                    })
                })
                .collect(),
        })
    }

    fn get_filter_list(&self) -> ExtensionResult<wit_types::FilterList> {
        // The select fixture must exercise both filter rendering and wire encoding.
        Ok(wit_types::FilterList {
            filters: vec![wit_types::FilterDef {
                id: "genre".to_string(),
                name: "Genre".to_string(),
                tag: wit_types::FilterTypeTag::Select,
                options: ["action", "romance"]
                    .iter()
                    .map(|v| wit_types::FilterOption {
                        filter_name: "genre".to_string(),
                        name: v.to_string(),
                        value: v.to_string(),
                    })
                    .collect(),
                default_value: None,
                semantic: None,
            }],
        })
    }

    fn get_preferences(&self) -> ExtensionResult<Vec<PreferenceSpec>> {
        Ok(vec![])
    }

    fn get_chapter_sort_list(&self) -> ExtensionResult<Vec<wit_types::SortOption>> {
        Ok(vec![])
    }
}

impl Guest for Fixture {
    fn get_metadata() -> Result<String, wit_types::ExtensionError> {
        Ok(kani_shared::serde_json::to_string(&Fixture::metadata())
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

use std::sync::OnceLock;

static EXTENSION: OnceLock<Fixture> = OnceLock::new();

fn get_extension() -> &'static Fixture {
    EXTENSION.get_or_init(Fixture::new)
}

bindings::export!(Fixture);
