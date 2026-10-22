//! Test extension for kani-core WASM ABI integration tests.
//!
//! Each exported manga-provider function exercises a specific set of host imports,
//! encoding the extracted values in the returned types so host-side tests can assert
//! exact values without needing to mock anything.
//!
//! Dispatch table:
//!
//!   get_popular_manga(page=1)  → html::* (parse, select, list_len, list_get, attr, text,
//!                                          first, inner_html, outer_html, children, drops)
//!   get_popular_manga(page=2)  → json::* (parse, get_str/i64/f64/bool, array_len/get,
//!                                          object_keys/get, to_string, drop)
//!   get_popular_manga(page=3)  → utility::* (date_parse, date_parse_rfc3339, resolve_url,
//!                                             build_url, url_encode, url_decode,
//!                                             get_query_param, encode_form, log)
//!   search_manga(query="prefs")        → prefs::get_value + host_abi prefs helpers
//!   search_manga(query="extract-html") → extraction::extract_html with a Blueprint
//!   search_manga(query="extract-json") → extraction::extract_json with a Blueprint
//!   get_manga_details("error-paths")   → invalid-handle error return verification
//!   get_chapter_list("error-network")  → ExtensionError::Network kind round-trip
//!   get_chapter_list("error-parse")    → ExtensionError::Parse kind round-trip
//!   get_chapter_list("error-not-found")→ ExtensionError::NotFound kind round-trip
//!   get_chapter_list("error-rate-limited") → ExtensionError::RateLimited kind round-trip
//!   get_chapter_list("error-auth")     → ExtensionError::Auth kind round-trip
//!   get_chapter_list("error-timeout")  → ExtensionError::Timeout kind round-trip
//!   get_chapter_list("error-internal") → ExtensionError::Internal kind round-trip
//!   get_chapter_list("paginated-stream")     → 2-page ChapterList (2 chapters/page), used
//!                                               to prove the default get_chapter_list_stream
//!                                               bridge delivers all chapters in order
//!   get_chapter_list_stream("native-stream") → native stream<chapter-info> override that
//!                                               calls extraction::extract_html (an async
//!                                               host import) between two stream writes,
//!                                               proving a guest can make a reentrant host
//!                                               call from inside its spawn_local writer
//!                                               while the host is concurrently draining

#[cfg(not(target_family = "wasm"))]
compile_error!(
    "kani-extensions/* are WASM-only -- build with `cargo run -p kani-cli -- build <name>`. \
     If a tool triggered this, it is using --workspace (or defaulting to it); scope it to \
     default-members instead -- clippy/nextest omit the flag, cargo-dist needs precise-builds."
);

use kani_shared::bindings::exports::kani::extension::manga_provider::Guest;
use kani_shared::bindings::kani::extension::{json, prefs as prefs_raw};
use kani_shared::html;
use kani_shared::utility;
use kani_shared::{
    ExtensionError, ExtensionMetadata, ExtensionResult, MangaExtension, MangaStatus, bindings,
    ext_version,
    host_abi::{extract, prefs},
    to_shared_filters,
    types::ActiveFilter,
    wit_types,
};
use wit_types::{
    Chapter, ChapterList, ExtensionError as WitError, MangaInfo, MangaList, MangaListItem,
    PreferenceSpec,
};

kani_shared::guest_alloc!();

pub struct TestAbi;

impl Default for TestAbi {
    fn default() -> Self {
        Self
    }
}

impl TestAbi {
    pub fn new() -> Self {
        Self
    }

    pub fn metadata() -> ExtensionMetadata {
        ExtensionMetadata {
            id: "test-abi".to_string(),
            name: "TestAbi".to_string(),
            version: ext_version!("0.1.0"),
            base_url: "https://example.com".to_string(),
            language: "en".to_string(),
            nsfw: false,
            unrestricted_http: false,
            mihon_source_id: None,
            rate_limit: None,
            ..Default::default()
        }
    }
}

fn item(id: &str, title: &str, cover: Option<&str>) -> MangaListItem {
    MangaListItem {
        id: id.to_string(),
        title: title.to_string(),
        cover_url: cover.map(|s| s.to_string()),
    }
}

fn list(items: Vec<MangaListItem>) -> MangaList {
    MangaList {
        manga: items,
        has_next_page: false,
        total_pages: None,
    }
}

const HTML_DOC: &str = r#"<html><body>
  <div class="card" data-id="42">
    <p class="title">Hello World</p>
    <span class="extra">foo</span>
  </div>
</body></html>"#;

fn test_html_imports() -> ExtensionResult<MangaList> {
    let doc = html::parse(HTML_DOC).map_err(ExtensionError::unknown)?;

    let list_h = html::select(doc, ".card").map_err(ExtensionError::unknown)?;
    let len = html::list_len(list_h).map_err(ExtensionError::unknown)?;
    if len != 1 {
        return Err(ExtensionError::unknown(format!(
            "expected list_len=1, got {}",
            len
        )));
    }
    let elem = html::list_get(list_h, 0).map_err(ExtensionError::unknown)?;

    let id_val = html::attr(elem, "", "data-id")
        .map_err(ExtensionError::unknown)?
        .unwrap_or_default();

    let title_val = html::text(elem, ".title")
        .map_err(ExtensionError::unknown)?
        .unwrap_or_default();

    let first_opt = html::first(doc, ".card").map_err(ExtensionError::unknown)?;
    let first_h = first_opt.ok_or_else(|| ExtensionError::unknown("first returned None".into()))?;

    let _inner = html::inner_html(elem).map_err(ExtensionError::unknown)?;
    let outer = html::outer_html(elem).map_err(ExtensionError::unknown)?;

    let child_list = html::children(elem).map_err(ExtensionError::unknown)?;
    let child_len = html::list_len(child_list).map_err(ExtensionError::unknown)?;
    if child_len < 2 {
        return Err(ExtensionError::unknown(format!(
            "expected >=2 children, got {}",
            child_len
        )));
    }

    html::drop_list(list_h);
    html::drop_list(child_list);
    html::drop_doc(elem);
    html::drop_doc(first_h);
    html::drop_doc(doc);

    let cover = if outer.is_some() {
        "html-ok"
    } else {
        "no-outer-html"
    };
    Ok(list(vec![item(&id_val, &title_val, Some(cover))]))
}

const JSON_DOC: &[u8] = br#"{
  "name": "Alice",
  "age": 30,
  "active": true,
  "score": 9.5,
  "tags": ["alpha", "beta"],
  "nested": {"key": "value"}
}"#;

fn test_json_imports() -> ExtensionResult<MangaList> {
    let h = json::parse(JSON_DOC).map_err(ExtensionError::parse)?;

    let name = json::get_str(h, "/name")
        .map_err(ExtensionError::unknown)?
        .unwrap_or_default();

    let age = json::get_i64(h, "/age")
        .map_err(ExtensionError::unknown)?
        .unwrap_or(0);

    let active = json::get_bool(h, "/active")
        .map_err(ExtensionError::unknown)?
        .unwrap_or(false);

    let score = json::get_f64(h, "/score")
        .map_err(ExtensionError::unknown)?
        .unwrap_or(0.0);

    let tag_count = json::array_len(h, "/tags")
        .map_err(ExtensionError::unknown)?
        .unwrap_or(0);

    let tag0_h = json::array_get(h, "/tags", 0).map_err(ExtensionError::unknown)?;
    let tag0 = json::get_str(tag0_h, "")
        .map_err(ExtensionError::unknown)?
        .unwrap_or_default();
    json::drop_json(tag0_h);

    let keys = json::object_keys(h, "").map_err(ExtensionError::unknown)?;

    let nested_opt = json::object_get(h, "", "nested").map_err(ExtensionError::unknown)?;
    if let Some(nested_h) = nested_opt {
        let _val = json::get_str(nested_h, "/key").map_err(ExtensionError::unknown)?;
        json::drop_json(nested_h);
    }

    let json_str = json::to_string(h).map_err(ExtensionError::unknown)?;

    json::drop_json(h);

    let cover = if active
        && score > 9.0
        && tag_count == 2
        && tag0 == "alpha"
        && keys.len() >= 5
        && json_str.contains("Alice")
    {
        "json-ok"
    } else {
        "json-fail"
    };

    Ok(list(vec![item(&name, &age.to_string(), Some(cover))]))
}

fn test_utility_imports() -> ExtensionResult<MangaList> {
    let ts1 = utility::date_parse("2024-01-15", "[year]-[month]-[day]")
        .map_err(ExtensionError::unknown)?;

    let ts2 =
        utility::date_parse_rfc3339("2024-01-15T00:00:00Z").map_err(ExtensionError::unknown)?;

    if ts1 != ts2 {
        return Err(ExtensionError::unknown(format!(
            "date mismatch: date_parse={} vs rfc3339={}",
            ts1, ts2
        )));
    }

    let _resolved =
        utility::resolve_url("https://example.com/a/b", "/c").map_err(ExtensionError::unknown)?;

    let built = utility::build_url(
        "https://example.com",
        &[
            ("pg".to_string(), "5".to_string()),
            ("sort".to_string(), "asc".to_string()),
        ],
    )
    .map_err(ExtensionError::unknown)?;

    let encoded = utility::url_encode("hello world");
    let decoded = utility::url_decode(&encoded).map_err(ExtensionError::unknown)?;

    let pg_val = utility::get_query_param(&built, "pg");

    let _form = utility::encode_form(&[
        ("a".to_string(), "b c".to_string()),
        ("d".to_string(), "e".to_string()),
    ]);

    utility::log(1, 0, "kani-test-abi utility test");

    Ok(list(vec![item("util-ok", &decoded, pg_val.as_deref())]))
}

fn test_prefs_imports() -> ExtensionResult<MangaList> {
    let raw_str = prefs_raw::get_value("test_str");
    let raw_missing = prefs_raw::get_value("missing_key");

    let str_val = prefs::get_str("test_str");
    let bool_val = prefs::get_bool("test_bool");
    let _i64_val = prefs::get_i64("test_i64");
    let _f64_val = prefs::get_f64("test_f64");
    let missing_empty = prefs::get_str("missing_key");

    let all_ok = raw_str.is_some() && raw_missing.is_none() && missing_empty.is_empty();

    let cover = if all_ok {
        Some("prefs-ok")
    } else {
        Some("prefs-fail")
    };

    Ok(list(vec![item(&str_val, &bool_val.to_string(), cover)]))
}

const EXTRACT_HTML: &str = r#"<html><body>
  <ul>
    <li class="item" data-id="1"><span class="name">Alpha</span></li>
    <li class="item" data-id="2"><span class="name">Beta</span></li>
  </ul>
</body></html>"#;

fn test_extract_html() -> ExtensionResult<MangaList> {
    use kani_shared::ast::{BlueprintBuilder, Expr};
    let doc = html::parse(EXTRACT_HTML).map_err(ExtensionError::unknown)?;

    let bp = BlueprintBuilder::new(".item")
        .field("id", Expr::self_ref().attr("data-id"))
        .field("name", Expr::self_ref().first(".name").text())
        .build();

    let result = extract::html(Some(doc), &bp)?;

    html::drop_doc(doc);

    if result.rows_len() == 0 {
        return Err(ExtensionError::unknown(
            "extract_html returned no rows".into(),
        ));
    }

    let mut items = Vec::new();
    for row in result.rows_iter() {
        let id = row.get_str("/id").unwrap_or_default();
        let name = row.get_str("/name").unwrap_or_default();
        items.push(item(&id, &name, None));
    }

    Ok(list(items))
}

const EXTRACT_JSON: &[u8] = br#"{
  "items": [
    {"manga_id": "j1", "manga_title": "JsonAlpha"},
    {"manga_id": "j2", "manga_title": "JsonBeta"}
  ]
}"#;

fn test_extract_json() -> ExtensionResult<MangaList> {
    use kani_shared::ast::{BlueprintBuilder, Expr};
    let h = json::parse(EXTRACT_JSON).map_err(ExtensionError::parse)?;

    let bp = BlueprintBuilder::new("/items")
        .field("id", Expr::self_ref().ptr("/manga_id").str_val())
        .field("title", Expr::self_ref().ptr("/manga_title").str_val())
        .build();

    let result = extract::json(Some(h), &bp)?;

    json::drop_json(h);

    if result.rows_len() == 0 {
        return Err(ExtensionError::unknown(
            "extract_json returned no rows".into(),
        ));
    }

    let mut items = Vec::new();
    for row in result.rows_iter() {
        let id = row.get_str("/id").unwrap_or_default();
        let title = row.get_str("/title").unwrap_or_default();
        items.push(item(&id, &title, None));
    }

    Ok(list(items))
}

fn test_native_chapter_list_stream()
-> kani_shared::StreamReader<Result<wit_types::ChapterInfo, wit_types::ExtensionError>> {
    let (mut tx, rx) = bindings::wit_stream::new();
    kani_shared::spawn_local(async move {
        let first = wit_types::ChapterInfo {
            id: "native-1".into(),
            number: 1.0,
            title: None,
            volume: None,
            scanlator: None,
            date_uploaded: None,
            language: "en".into(),
            page_count: Some(19),
        };
        if !tx.write_all(vec![Ok(first)]).await.is_empty() {
            return;
        }

        if let Ok(doc) = html::parse(EXTRACT_HTML) {
            use kani_shared::ast::{BlueprintBuilder, Expr};
            let bp = BlueprintBuilder::new(".item")
                .field("id", Expr::self_ref().attr("data-id"))
                .build();
            let _ = extract::html(Some(doc), &bp);
            html::drop_doc(doc);
        }

        let second = wit_types::ChapterInfo {
            id: "native-2".into(),
            number: 2.0,
            title: None,
            volume: None,
            scanlator: None,
            date_uploaded: None,
            language: "en".into(),
            page_count: None,
        };
        let _ = tx.write_all(vec![Ok(second)]).await;
    });
    rx
}

fn test_error_paths() -> ExtensionResult<MangaInfo> {
    let invalid_list: i32 = 9999;
    let invalid_doc: i32 = 9998;
    let invalid_json: i32 = 9997;

    let list_get_err = html::list_get(invalid_list, 0).is_err();
    let attr_err = html::attr(invalid_doc, "", "data-id").is_err();
    let text_err = html::text(invalid_doc, "").is_err();
    let inner_html_err = html::inner_html(invalid_doc).is_err();
    let outer_html_err = html::outer_html(invalid_doc).is_err();
    let first_err = html::first(invalid_doc, "div").is_err();
    let children_err = html::children(invalid_doc).is_err();
    let select_err = html::select(invalid_doc, "div").is_err();

    let json_get_str_err = json::get_str(invalid_json, "/x").is_err();
    let json_get_i64_err = json::get_i64(invalid_json, "/x").is_err();
    let json_get_f64_err = json::get_f64(invalid_json, "/x").is_err();
    let json_get_bool_err = json::get_bool(invalid_json, "/x").is_err();
    let json_arr_len_err = json::array_len(invalid_json, "/x").is_err();
    let json_arr_get_err = json::array_get(invalid_json, "/x", 0).is_err();
    let json_obj_keys_err = json::object_keys(invalid_json, "").is_err();
    let json_obj_get_err = json::object_get(invalid_json, "", "k").is_err();
    let json_to_str_err = json::to_string(invalid_json).is_err();

    let all_ok = list_get_err
        && attr_err
        && text_err
        && inner_html_err
        && outer_html_err
        && first_err
        && children_err
        && select_err
        && json_get_str_err
        && json_get_i64_err
        && json_get_f64_err
        && json_get_bool_err
        && json_arr_len_err
        && json_arr_get_err
        && json_obj_keys_err
        && json_obj_get_err
        && json_to_str_err;

    Ok(MangaInfo {
        id: "error-paths".to_string(),
        title: "ErrorPaths".to_string(),
        cover_url: None,
        description: Some(
            if all_ok {
                "error-paths-ok"
            } else {
                "error-paths-fail"
            }
            .to_string(),
        ),
        authors: vec![],
        artists: vec![],
        status: MangaStatus::Unknown,
        tags: vec![],
    })
}

fn test_paginated_chapter_list(page: i32) -> ChapterList {
    fn chapter(id: &str, number: f64) -> wit_types::ChapterInfo {
        wit_types::ChapterInfo {
            id: id.to_string(),
            number,
            title: None,
            volume: None,
            scanlator: None,
            date_uploaded: None,
            language: "en".to_string(),
            page_count: Some(number as u32 + 10),
        }
    }
    match page {
        1 => ChapterList {
            chapters: vec![chapter("p1-1", 1.0), chapter("p1-2", 2.0)],
            has_next_page: true,
            total_pages: Some(2),
        },
        2 => ChapterList {
            chapters: vec![chapter("p2-1", 3.0), chapter("p2-2", 4.0)],
            has_next_page: false,
            total_pages: Some(2),
        },
        _ => ChapterList {
            chapters: vec![],
            has_next_page: false,
            total_pages: Some(2),
        },
    }
}

fn test_error_kind(manga_id: &str) -> ExtensionResult<ChapterList> {
    match manga_id {
        "error-network" => Err(ExtensionError::network("connection refused".into())
            .with_url("https://example.com/api?q=1")),
        "error-parse" => Err(ExtensionError::parse("unexpected token".into())),
        "error-not-found" => Err(ExtensionError::not_found("manga not found".into())),
        "error-rate-limited" => Err(ExtensionError::rate_limited_with_retry(60)),
        "error-auth" => Err(ExtensionError::auth("token expired".into())),
        "error-timeout" => Err(ExtensionError::timeout("request timed out".into())),
        "error-internal" => Err(ExtensionError::internal("unexpected state".into())),
        _ => Ok(ChapterList {
            chapters: vec![],
            has_next_page: false,
            total_pages: None,
        }),
    }
}

impl Guest for TestAbi {
    fn get_metadata() -> Result<String, WitError> {
        Ok(kani_shared::serde_json::to_string(&TestAbi::metadata())
            .expect("ExtensionMetadata serializes to JSON"))
    }

    fn get_popular_manga(
        page: i32,
        page_size: i32,
        filters: Vec<wit_types::ActiveFilter>,
    ) -> Result<MangaList, WitError> {
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
    ) -> Result<MangaList, WitError> {
        let shared = to_shared_filters(filters);
        get_extension()
            .search_manga(&query, page, page_size, &shared)
            .map_err(|e| e.into_wit())
    }

    fn get_filter_list() -> Result<wit_types::FilterList, WitError> {
        get_extension().get_filter_list().map_err(|e| e.into_wit())
    }

    fn get_fetched_option_sets() -> Result<String, WitError> {
        get_extension()
            .get_fetched_option_sets()
            .map_err(|e| e.into_wit())
    }

    fn get_manga_details(manga_id: String) -> Result<MangaInfo, WitError> {
        get_extension()
            .get_manga_details(&manga_id)
            .map_err(|e| e.into_wit())
    }

    fn get_chapter_list(
        manga_id: String,
        page: i32,
        page_size: Option<i32>,
        sort: Option<String>,
    ) -> Result<ChapterList, WitError> {
        get_extension()
            .get_chapter_list(&manga_id, page, page_size, sort)
            .map_err(|e| e.into_wit())
    }
    async fn get_chapter_list_stream(
        manga_id: String,
        sort: Option<String>,
    ) -> kani_shared::StreamReader<Result<wit_types::ChapterInfo, wit_types::ExtensionError>> {
        if manga_id == "native-stream" {
            return test_native_chapter_list_stream();
        }
        kani_shared::bridge_chapter_list_stream(get_extension(), manga_id, sort)
    }

    fn get_chapter_sort_list() -> Result<Vec<wit_types::SortOption>, WitError> {
        get_extension()
            .get_chapter_sort_list()
            .map_err(|e| e.into_wit())
    }

    fn get_pages(manga_id: String, chapter_id: String) -> Result<Chapter, WitError> {
        get_extension()
            .get_pages(&manga_id, &chapter_id)
            .map_err(|e| e.into_wit())
    }

    fn get_preferences() -> Result<Vec<PreferenceSpec>, WitError> {
        get_extension().get_preferences().map_err(|e| e.into_wit())
    }

    fn get_url(manga_id: String) -> Result<String, WitError> {
        get_extension().get_url(&manga_id).map_err(|e| e.into_wit())
    }
}

impl MangaExtension for TestAbi {
    fn name(&self) -> &str {
        "TestAbi"
    }

    fn get_popular_manga(
        &self,
        page: i32,
        _page_size: i32,
        _filters: &[ActiveFilter],
    ) -> ExtensionResult<MangaList> {
        match page {
            1 => test_html_imports(),
            2 => test_json_imports(),
            3 => test_utility_imports(),
            _ => Ok(MangaList {
                manga: vec![],
                has_next_page: false,
                total_pages: None,
            }),
        }
    }

    fn search_manga(
        &self,
        query: &str,
        _page: i32,
        _page_size: i32,
        _filters: &[ActiveFilter],
    ) -> ExtensionResult<MangaList> {
        match query {
            "prefs" => test_prefs_imports(),
            "extract-html" => test_extract_html(),
            "extract-json" => test_extract_json(),
            _ => Ok(MangaList {
                manga: vec![],
                has_next_page: false,
                total_pages: None,
            }),
        }
    }

    fn get_manga_details(&self, manga_id: &str) -> ExtensionResult<MangaInfo> {
        match manga_id {
            "error-paths" => test_error_paths(),
            _ => Err(ExtensionError::not_found(manga_id.to_string())),
        }
    }

    fn get_chapter_list(
        &self,
        manga_id: &str,
        page: i32,
        _page_size: Option<i32>,
        _sort: Option<String>,
    ) -> ExtensionResult<ChapterList> {
        if manga_id == "paginated-stream" {
            return Ok(test_paginated_chapter_list(page));
        }
        test_error_kind(manga_id)
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

static EXTENSION: OnceLock<TestAbi> = OnceLock::new();

fn get_extension() -> &'static TestAbi {
    EXTENSION.get_or_init(TestAbi::new)
}

bindings::export!(TestAbi);
