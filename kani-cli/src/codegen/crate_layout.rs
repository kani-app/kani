//! Emit Cargo.toml and the lib.rs header / footer for a generated extension crate.

use crate::yaml::model::{ValidatedChapterSort, ValidatedExtension};

pub(crate) fn emit_cargo_toml(ext: &ValidatedExtension, embedded_bytes: bool) -> String {
    let id = &ext.id;
    let features: &str = if embedded_bytes {
        ", features = [\"meta\"]"
    } else {
        ", features = [\"builder\"]"
    };
    format!(
        r#"[package]
name = "kani-{id}"
version = "{version}"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
kani-shared = {{ path = "../../kani-shared"{features} }}

[package.metadata]
id = "{id}"
"#,
        version = ext.version,
    )
}

pub(crate) fn emit_lib_header(ext: &ValidatedExtension, embedded_bytes: bool) -> String {
    let struct_name = to_pascal_case(&ext.id);
    let base_url = &ext.base_url;
    let id = &ext.id;
    let name = &ext.name;
    let version = &ext.version;
    let language = &ext.language;
    let nsfw = ext.nsfw;
    let unrestricted_http = ext.unrestricted_http;
    let mihon_source_id = match ext.mihon_source_id {
        Some(n) => format!("Some({n}_i64)"),
        None => "None".to_string(),
    };

    let icon = match &ext.metadata.icon {
        Some(b64) => format!("Some(\"{}\".to_string())", escape_str(b64)),
        None => "None".to_string(),
    };
    let rate_limit = match &ext.metadata.rate_limit {
        Some(rl) => format!(
            "Some(kani_shared::RateLimitConfig {{ requests_per_second: {rps}_f32, burst: {burst}_u32, max_concurrent: {max_concurrent}_u32, max_hook_requests: {max_hook_requests}_u32 }})",
            rps = rl.requests_per_second,
            burst = rl.burst,
            max_concurrent = rl.max_concurrent,
            max_hook_requests = rl.max_hook_requests,
        ),
        None => "None".to_string(),
    };
    let languages = format!(
        "vec![{}]",
        ext.metadata
            .languages
            .iter()
            .map(|l| format!("\"{}\".to_string()", escape_str(l)))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let description = match &ext.metadata.description {
        Some(d) => format!("Some(\"{}\".to_string())", escape_str(d)),
        None => "None".to_string(),
    };
    let sections = format!(
        "vec![{}]",
        ext.metadata
            .sections
            .iter()
            .map(|s| format!(
                "kani_shared::Section {{ id: \"{}\".to_string(), name: \"{}\".to_string(), nsfw: {} }}",
                escape_str(&s.id),
                escape_str(&s.name),
                s.nsfw
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let schema_version = ext.schema_version;
    let min_kani_version = match &ext.min_kani_version {
        Some(v) => format!("Some(\"{}\".to_string())", escape_str(v)),
        None => "None".to_string(),
    };
    let requires_capabilities = format!(
        "vec![{}]",
        ext.requires_capabilities
            .iter()
            .map(|c| format!("\"{}\".to_string()", escape_str(c)))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let scripts = if ext.pure_scripts.is_empty() {
        "std::collections::BTreeMap::new()".to_string()
    } else {
        let entries = ext
            .pure_scripts
            .keys()
            .map(|name| {
                format!(
                    "(\"{name}\".to_string(), include_str!(\"scripts/{name}.rhai\").to_string())"
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("std::collections::BTreeMap::from([{entries}])")
    };

    let pre_request = match &ext.pre_request {
        Some(body) => format!("Some({}.to_string())", escape_rust_string_lit(body)),
        None => "None".to_string(),
    };

    let on_status = emit_btreemap_string_string(&ext.on_status);

    let endpoint_pre_request = emit_btreemap_string_string(&ext.endpoint_pre_request);

    let endpoint_on_status = if ext.endpoint_on_status.is_empty() {
        "std::collections::BTreeMap::new()".to_string()
    } else {
        let entries = ext
            .endpoint_on_status
            .iter()
            .map(|(ep, patterns)| {
                format!(
                    "(\"{ep}\".to_string(), {})",
                    emit_btreemap_string_string(patterns)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("std::collections::BTreeMap::from([{entries}])")
    };

    let cache = if ext.cache.is_empty() {
        "std::collections::BTreeMap::new()".to_string()
    } else {
        let entries = ext
            .cache
            .iter()
            .map(|c| {
                let max_entries = match c.max_entries {
                    Some(n) => format!("Some({n}_u32)"),
                    None => "None".to_string(),
                };
                format!(
                    "(\"{}\".to_string(), kani_shared::CacheNamespaceLimits {{ ttl_seconds: {}_u32, max_entries: {max_entries} }})",
                    escape_str(&c.name),
                    c.ttl
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("std::collections::BTreeMap::from([{entries}])")
    };

    let pref_import = if ext.preferences.is_empty() {
        ""
    } else {
        ", preference_list"
    };

    let extract_import = if embedded_bytes {
        "use kani_shared::host_abi::{HttpRequest, extract_raw};"
    } else {
        "use kani_shared::host_abi::{HttpRequest, extract};"
    };

    let ast_import = if embedded_bytes {
        String::new()
    } else {
        "use kani_shared::ast::{BlueprintBuilder, Expr, OffsetType};\n".into()
    };

    format!(
        r#"// @generated by kani-cli from {id}.yaml — edit freely, but regenerate with `kani-cli generate` to reset.

use std::sync::OnceLock;
use kani_shared::bindings::exports::kani::extension::manga_provider::Guest;
{extract_import}
use kani_shared::{{
    ExtensionMetadata, ExtensionResult, MangaExtension, bindings, wit_types,
    types::ActiveFilter, to_shared_filters, filter_list{pref_import}, FilterState,
}};
{ast_import}use wit_types::{{Chapter, ChapterList, ChapterInfo, MangaInfo, MangaList, PreferenceSpec}};

kani_shared::guest_alloc!();

pub struct {struct_name} {{
    base_url: String,
}}

impl Default for {struct_name} {{
    fn default() -> Self {{ Self::new() }}
}}

impl {struct_name} {{
    pub fn new() -> Self {{
        Self {{ base_url: "{base_url}".to_string() }}
    }}

    pub fn metadata() -> ExtensionMetadata {{
        ExtensionMetadata {{
            id:               "{id}".to_string(),
            name:             "{name}".to_string(),
            version:          "{version}".to_string(),
            base_url:         "{base_url}".to_string(),
            language:         "{language}".to_string(),
            nsfw:             {nsfw},
            unrestricted_http: {unrestricted_http},
            mihon_source_id:  {mihon_source_id},
            rate_limit:       {rate_limit},
            icon:             {icon},
            languages:        {languages},
            description:      {description},
            schema_version:   {schema_version}_u32,
            min_kani_version: {min_kani_version},
            requires_capabilities: {requires_capabilities},
            sections:         {sections},
            scripts:          {scripts},
            pre_request:      {pre_request},
            on_status:        {on_status},
            endpoint_pre_request: {endpoint_pre_request},
            endpoint_on_status: {endpoint_on_status},
            cache:            {cache},
            dsl_schema_version: Some(kani_shared::ast::DSL_SCHEMA_VERSION),
        }}
    }}
}}
"#
    )
}

pub(crate) fn emit_guest_impl(ext: &ValidatedExtension) -> String {
    let struct_name = to_pascal_case(&ext.id);

    let popular_impl: String = if ext.popular.is_some() {
        "    fn get_popular_manga(page: i32, page_size: i32, filters: Vec<wit_types::ActiveFilter>) -> Result<MangaList, wit_types::ExtensionError> {\n\
         let shared = to_shared_filters(filters);\n\
         get_extension().get_popular_manga(page, page_size, &shared).map_err(|e| e.into_wit())\n\
         }\n".to_string()
    } else {
        "    fn get_popular_manga(_page: i32, _page_size: i32, _filters: Vec<wit_types::ActiveFilter>) -> Result<MangaList, wit_types::ExtensionError> {\n\
         Ok(MangaList { manga: vec![], has_next_page: false, total_pages: None })\n\
         }\n".to_string()
    };

    let search_impl: String = if ext.search.is_some() {
        "    fn search_manga(query: String, page: i32, page_size: i32, filters: Vec<wit_types::ActiveFilter>) -> Result<MangaList, wit_types::ExtensionError> {\n\
         let shared = to_shared_filters(filters);\n\
         get_extension().search_manga(&query, page, page_size, &shared).map_err(|e| e.into_wit())\n\
         }\n".to_string()
    } else {
        "    fn search_manga(_query: String, _page: i32, _page_size: i32, _filters: Vec<wit_types::ActiveFilter>) -> Result<MangaList, wit_types::ExtensionError> {\n\
         Ok(MangaList { manga: vec![], has_next_page: false, total_pages: None })\n\
         }\n".to_string()
    };

    format!(
        r#"impl Guest for {struct_name} {{
    fn get_metadata() -> Result<String, wit_types::ExtensionError> {{
        Ok(kani_shared::serde_json::to_string(&{struct_name}::metadata())
            .expect("ExtensionMetadata serializes to JSON"))
    }}

{popular_impl}
{search_impl}
    fn get_filter_list() -> Result<wit_types::FilterList, wit_types::ExtensionError> {{
        get_extension().get_filter_list().map_err(|e| e.into_wit())
    }}

    fn get_fetched_option_sets() -> Result<String, wit_types::ExtensionError> {{
        get_extension().get_fetched_option_sets().map_err(|e| e.into_wit())
    }}

    fn get_manga_details(manga_id: String) -> Result<MangaInfo, wit_types::ExtensionError> {{
        get_extension().get_manga_details(&manga_id).map_err(|e| e.into_wit())
    }}

    fn get_chapter_list(manga_id: String, page: i32, page_size: Option<i32>, sort: Option<String>) -> Result<ChapterList, wit_types::ExtensionError> {{
        get_extension().get_chapter_list(&manga_id, page, page_size, sort).map_err(|e| e.into_wit())
    }}

    async fn get_chapter_list_stream(manga_id: String, sort: Option<String>) -> kani_shared::StreamReader<Result<ChapterInfo, wit_types::ExtensionError>> {{
        kani_shared::bridge_chapter_list_stream(get_extension(), manga_id, sort)
    }}

    fn get_chapter_sort_list() -> Result<Vec<wit_types::SortOption>, wit_types::ExtensionError> {{
        get_extension().get_chapter_sort_list().map_err(|e| e.into_wit())
    }}

    fn get_pages(manga_id: String, chapter_id: String) -> Result<Chapter, wit_types::ExtensionError> {{
        get_extension().get_pages(&manga_id, &chapter_id).map_err(|e| e.into_wit())
    }}

    fn get_preferences() -> Result<Vec<PreferenceSpec>, wit_types::ExtensionError> {{
        get_extension().get_preferences().map_err(|e| e.into_wit())
    }}

    fn get_url(manga_id: String) -> Result<String, wit_types::ExtensionError> {{
        get_extension().get_url(&manga_id).map_err(|e| e.into_wit())
    }}
}}

static EXTENSION: OnceLock<{struct_name}> = OnceLock::new();

fn get_extension() -> &'static {struct_name} {{
    EXTENSION.get_or_init({struct_name}::new)
}}

bindings::export!({struct_name});
"#
    )
}

/// Emits the `get_chapter_sort_list` (and optionally `default_chapter_sort`)
/// `MangaExtension` impl methods from a validated `chapter_sort` block.
pub(crate) fn emit_chapter_sort(cs: &ValidatedChapterSort) -> String {
    let entries: Vec<String> = cs
        .options
        .iter()
        .map(|o| {
            format!(
                "wit_types::SortOption {{ id: \"{id}\".to_string(), name: \"{label}\".to_string() }}",
                id = escape_str(&o.id),
                label = escape_str(&o.label),
            )
        })
        .collect();

    let sort_list = format!(
        "fn get_chapter_sort_list(&self) -> ExtensionResult<Vec<wit_types::SortOption>> {{\n    Ok(vec![{}])\n}}",
        entries.join(", ")
    );

    match &cs.default {
        Some(d) => format!(
            "{sort_list}\n\nfn default_chapter_sort(&self) -> Option<String> {{\n    Some(\"{d}\".to_string())\n}}",
            d = escape_str(d)
        ),
        None => sort_list,
    }
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_rust_string_lit(s: &str) -> String {
    format!("\"{}\"", escape_str(s))
}

fn emit_btreemap_string_string(map: &std::collections::BTreeMap<String, String>) -> String {
    if map.is_empty() {
        "std::collections::BTreeMap::new()".to_string()
    } else {
        let entries = map
            .iter()
            .map(|(k, v)| {
                format!(
                    "({}.to_string(), {}.to_string())",
                    escape_rust_string_lit(k),
                    escape_rust_string_lit(v)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("std::collections::BTreeMap::from([{entries}])")
    }
}

pub(crate) fn to_pascal_case(s: &str) -> String {
    s.split(['-', '_'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}
