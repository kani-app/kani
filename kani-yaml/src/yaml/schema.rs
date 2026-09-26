//! Serde input model defining the declarative extension YAML format.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
/// Top-level declarative extension document before semantic validation.
/// Defaults and accepted wire shapes in this module are part of the authoring format.
pub struct YamlExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    pub base_url: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub nsfw: bool,
    #[serde(default)]
    pub unrestricted_http: bool,
    #[serde(default)]
    pub endpoints: Endpoints,
    #[serde(default)]
    pub filters: Vec<FilterEntry>,
    #[serde(default)]
    pub preferences: Vec<PreferenceEntry>,
    #[serde(default)]
    pub option_sets: BTreeMap<String, OptionSetDef>,
    /// Optional URL template for manga canonical URL. Use `$manga_id$` as placeholder.
    pub get_url: Option<String>,
    /// Optional Mihon/Tachiyomi source ID for cross-app import matching.
    #[serde(default)]
    pub mihon_source_id: Option<i64>,
    /// Declares composite ID encode/decode for manga and/or chapter IDs.
    #[serde(default)]
    pub id_encoding: Option<IdEncodingBlock>,
    /// Declares named cache namespaces this extension wants the host to manage.
    #[serde(default)]
    pub cache: BTreeMap<String, CacheEntry>,
    /// Extended metadata (icon, languages, description, rate limit, sections).
    #[serde(default)]
    pub metadata: Option<MetadataBlock>,
    /// Schema version this YAML was authored against (defaults to 1, the current version).
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Minimum host (kani-app) semver this extension requires, if any.
    #[serde(default)]
    pub min_kani_version: Option<String>,
    /// Host capability strings this extension requires to be installed.
    #[serde(default)]
    pub requires_capabilities: Vec<String>,
    /// Declares the sort options this extension exposes for chapter lists.
    #[serde(default)]
    pub chapter_sort: Option<ChapterSortBlock>,
    /// Multi-source factory: one YAML template → N WASM outputs.
    #[serde(default)]
    pub factory: Option<FactoryBlock>,
    /// Named JavaScript scripts (name → source) executed in the headless browser context.
    #[serde(default)]
    pub browser_scripts: BTreeMap<String, String>,
    /// Pure Rhai scripts callable from the DSL via `.user.<name>(args...)`.
    #[serde(default)]
    pub scripts: ScriptsBlock,
    /// Rhai script body to run before every HTTP request for this source.
    #[serde(default)]
    pub pre_request: Option<String>,
    /// Rhai script bodies keyed by HTTP status pattern ("401", "5xx", "default").
    /// Runs after the response is received; can Retry, Fail, or Proceed.
    #[serde(default)]
    pub on_status: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ScriptsBlock {
    /// Pure Rhai functions (name → source). Callable as `.user.<name>(args...)` in DSL expressions.
    #[serde(default)]
    pub pure: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
/// Template and source matrix used to generate several extensions from one declaration.
pub struct FactoryBlock {
    /// Optional external template file path; if absent the containing YAML is the template.
    pub template: Option<String>,
    pub sources: Vec<FactorySource>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FactorySource {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub mihon_source_id: Option<i64>,
    /// Dot-path keyed overrides applied on top of the template (e.g. "endpoints.search.route").
    #[serde(default)]
    pub overrides: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ChapterSortBlock {
    pub default: Option<String>,
    #[serde(default)]
    pub options: Vec<ChapterSortOptionYaml>,
}

#[derive(Debug, Deserialize)]
pub struct ChapterSortOptionYaml {
    pub id: String,
    pub label: String,
}

/// Latest extension schema version accepted and emitted by this build.
pub(crate) const CURRENT_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

#[derive(Debug, Deserialize, Default)]
pub struct MetadataBlock {
    /// Base64-encoded icon image (PNG/WebP/SVG), ≤64KB decoded.
    pub icon: Option<String>,
    #[serde(default)]
    pub rate_limit: Option<RateLimitCfg>,
    #[serde(default)]
    pub languages: Vec<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub sections: Vec<SectionEntry>,
}

#[derive(Debug, Deserialize, Clone)]
/// Host-enforced upstream request and response-hook budget.
pub struct RateLimitCfg {
    /// Sustained requests per second.
    #[serde(default = "default_rps")]
    pub rps: f64,
    #[serde(default = "default_burst")]
    pub burst: u32,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    #[serde(default = "default_max_hook_requests")]
    pub max_hook_requests: u32,
}

fn default_rps() -> f64 {
    2.0
}

fn default_burst() -> u32 {
    8
}

fn default_max_concurrent() -> u32 {
    4
}

fn default_max_hook_requests() -> u32 {
    3
}

#[derive(Debug, Deserialize, Clone)]
pub struct SectionEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub nsfw: bool,
}

fn default_language() -> String {
    "en".to_string()
}

#[derive(Debug, Deserialize, Default)]
/// Declared provider endpoints keyed by the operation the host invokes.
pub struct Endpoints {
    pub popular: Option<PopularEndpoint>,
    pub search: Option<EndpointBody>,
    pub manga_details: Option<EndpointBody>,
    pub chapter_list: Option<EndpointBody>,
    pub pages: Option<EndpointBody>,
}

/// Popular can either delegate to another endpoint or define its own extraction.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum PopularEndpoint {
    // Delegated must come first so serde tries it before Full (which is permissive).
    Delegated {
        delegate_to: String,
        #[serde(default)]
        empty_without_filters: bool,
    },
    Full(Box<EndpointBody>),
}

#[derive(Debug, Deserialize, Default)]
/// Request, extraction, chaining, pagination, and hook declaration for one provider operation.
pub struct EndpointBody {
    pub route: Option<String>,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Query parameters. Values may contain `$var$` placeholders.
    #[serde(default)]
    pub queries: BTreeMap<String, String>,
    #[serde(default)]
    pub filter_mapping: BTreeMap<String, FilterMappingEntry>,
    #[serde(default)]
    pub filter_format: Option<FilterFormatCfg>,
    #[serde(rename = "type", default)]
    pub response_type: ResponseType,
    pub container: Option<String>,
    /// Document-level variable bindings (DSL expressions), evaluated before iteration.
    #[serde(default)]
    pub bindings: BTreeMap<String, String>,
    /// Per-element field extractions (DSL expressions).
    #[serde(default)]
    pub fields: BTreeMap<String, FieldDef>,
    /// Document-level output scalars (DSL expressions), evaluated once.
    #[serde(default)]
    pub scalars: BTreeMap<String, FieldDef>,
    /// Static bool or DSL expression; overrides automatic has_next_page logic.
    pub has_next_page: Option<HasNextPage>,
    /// Optional u32 or DSL expression; populates total_pages in MangaList/ChapterList.
    pub total_pages: Option<TotalPages>,
    pub pagination: Option<PaginationCfg>,
    /// Document-level chained fetches: evaluated once, result bound as `$merge_as`.
    #[serde(default)]
    pub then: Vec<ThenStep>,
    /// Per-element chained fetches: evaluated for each row, result stored as `merge_as` field.
    #[serde(default)]
    pub for_each: Vec<ForEachStep>,
    /// When set, the endpoint is fetched via a headless browser rather than direct HTTP.
    #[serde(default)]
    pub via: Option<EndpointVia>,
    /// URL of the page to load in the headless browser. Required when `via: browser_payload`.
    #[serde(default)]
    pub page_url: Option<String>,
    /// Name of a script declared in the top-level `browser_scripts` map.
    #[serde(default)]
    pub script: Option<String>,
    /// Timeout for the browser page load, in milliseconds. Default: 30000.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u32,
    /// Whether browser-backed endpoints periodically scroll to trigger lazy loading.
    #[serde(default)]
    pub auto_scroll: Option<bool>,
    /// Per-endpoint Rhai script body for pre_request (overrides source-level).
    #[serde(default)]
    pub pre_request: Option<String>,
    /// Per-endpoint Rhai script bodies for on_status (overrides source-level).
    #[serde(default)]
    pub on_status: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// Alternate transport used to obtain an endpoint's extraction input.
pub enum EndpointVia {
    #[default]
    BrowserPayload,
}

fn default_timeout_ms() -> u32 {
    30_000
}

/// What to do when a chained sub-fetch fails.
#[derive(Debug)]
pub enum OnFailure {
    /// Propagate the error (default).
    Fail,
    /// Suppress the error and use `Value::Null`.
    Skip,
    /// Suppress the error and evaluate the given DSL expression as a fallback.
    Use(String),
}

impl<'de> serde::Deserialize<'de> for OnFailure {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "skip" => Ok(OnFailure::Skip),
            "fail" => Ok(OnFailure::Fail),
            _ => Ok(OnFailure::Use(s)),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ThenStep {
    /// Name of a declared endpoint whose blueprint to use for extraction.
    pub endpoint: String,
    /// DSL expression evaluating to the URL to fetch.
    pub url_expr: String,
    /// Binding name (without `$`) for the fetched result; accessible in fields.
    pub merge_as: String,
    pub on_failure: Option<OnFailure>,
}

#[derive(Debug, Deserialize)]
pub struct ForEachStep {
    /// Name of a declared endpoint whose blueprint to use for extraction.
    pub endpoint: String,
    /// DSL expression evaluating to the URL to fetch (evaluated per-element).
    pub url_expr: String,
    /// Field name in which the fetched result is stored per row.
    pub merge_as: String,
    pub on_failure: Option<OnFailure>,
    /// DSL expression for deduplication key; rows with duplicate keys are dropped.
    pub deduplicate_by: Option<String>,
}

fn default_method() -> String {
    "GET".to_string()
}

/// `has_next_page` can be a literal `false`/`true` or a DSL expression string.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum HasNextPage {
    Static(bool),
    Expr(String),
}

/// `total_pages` can be a literal u32 or a DSL expression string.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TotalPages {
    Static(u32),
    Expr(String),
}

#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResponseType {
    #[default]
    Html,
    Json,
}

/// A field definition is either a bare DSL expression string, `{expr, optional}`,
/// or a composite-id map (subfield name -> DSL expression) consumed together with
/// a top-level `id_encoding` block.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum FieldDef {
    Expr(String),
    Full {
        expr: String,
        #[serde(default)]
        optional: bool,
    },
    Composite(BTreeMap<String, String>),
}

impl FieldDef {
    pub(crate) fn expr_str(&self) -> &str {
        match self {
            FieldDef::Expr(e) => e,
            FieldDef::Full { expr, .. } => expr,
            FieldDef::Composite(_) => "",
        }
    }

    pub fn optional(&self) -> bool {
        match self {
            FieldDef::Expr(_) | FieldDef::Composite(_) => false,
            FieldDef::Full { optional, .. } => *optional,
        }
    }

    pub(crate) fn as_composite(&self) -> Option<&BTreeMap<String, String>> {
        match self {
            FieldDef::Composite(map) => Some(map),
            _ => None,
        }
    }
}

/// Filter mapping entry: simple query-param name, a sort-pair split, or a
/// tuple split (value split at `:` into two separate query params).
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum FilterMappingEntry {
    Simple(String),
    SortPair {
        #[allow(dead_code)]
        kind: SortPairKind,
        key_template: String,
        #[serde(default)]
        direction_param: Option<String>,
    },
    TupleSplit {
        #[allow(dead_code)]
        kind: TupleSplitKind,
        from_param: String,
        to_param: String,
    },
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SortPairKind {
    SortPair,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TupleSplitKind {
    TupleSplit,
}

#[derive(Debug, Deserialize, Clone)]
/// Fixed-size source pagination declaration lowered into a blueprint pagination config.
pub struct PaginationCfg {
    pub native_page_size: usize,
    pub offset_param: String,
    pub offset_type: YamlOffsetType,
    #[serde(default = "default_page_start")]
    pub page_start: u32,
}

fn default_page_start() -> u32 {
    1
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum YamlOffsetType {
    Item,
    Page,
}

#[derive(Debug, Deserialize, Clone)]
/// One author-facing filter control and its wire-mapping metadata.
pub struct FilterEntry {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: FilterKind,
    #[serde(default)]
    pub options: Vec<FilterOption>,
    pub default: Option<FilterDefault>,
    pub semantic: Option<FilterSemantic>,
    /// Optional i18n key for the display name (display string for end users).
    pub name_i18n: Option<String>,
    /// References a top-level `option_sets` entry instead of inline `options`.
    pub options_ref: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    Checkbox,
    Select,
    Sort,
    TextInput,
    Multiselect,
    IntRange,
    DateRange,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FilterOption {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub nsfw: bool,
}

/// Default value for a filter — bool for checkbox, name+value for select/sort,
/// or bare string for text_input.
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum FilterDefault {
    Bool(bool),
    Option { name: String, value: String },
    Text(String),
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FilterSemantic {
    Author,
    Artist,
    Tag,
}

#[derive(Debug, Deserialize, Clone)]
/// One host-rendered and host-persisted extension preference.
pub struct PreferenceEntry {
    pub key: String,
    pub label: String,
    pub kind: PreferenceKind,
    #[serde(default)]
    pub options: Vec<PrefOption>,
    #[serde(default)]
    pub default: String,
    pub description: Option<String>,
    #[serde(default)]
    pub secret: bool,
    /// References a top-level `option_sets` entry instead of inline `options`.
    pub options_ref: Option<String>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceKind {
    Toggle,
    Select,
    Text,
    MultiValueList,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PrefOption {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FilterFormatCfg {
    #[serde(default)]
    pub multiselect: ArrayFormat,
    #[serde(default = "default_omit_empty")]
    pub omit_empty: bool,
    #[serde(default)]
    pub bool_format: BoolFormat,
    #[serde(default = "default_array_separator")]
    pub array_separator: String,
}

fn default_omit_empty() -> bool {
    true
}

fn default_array_separator() -> String {
    ",".to_string()
}

#[derive(Debug, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArrayFormat {
    #[default]
    Default,
    Bracket,
    CommaSeparated,
    Repeated,
}

#[derive(Debug, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoolFormat {
    #[default]
    TrueFalse,
    OneZero,
    YesNo,
}

/// Either a static inline list of options, or a definition for fetching them
/// lazily from the host at filter-panel render time.
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum OptionSetDef {
    Static(Vec<OptionSetItem>),
    Fetched {
        options_fetched_by: FetchedOptionsDef,
    },
}

#[derive(Debug, Deserialize, Clone)]
pub struct OptionSetItem {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub nsfw: bool,
}

#[derive(Debug, Deserialize, Clone)]
/// Host-fetched option-set extraction and caching declaration.
pub struct FetchedOptionsDef {
    pub route: String,
    #[serde(rename = "type", default)]
    pub response_type: ResponseType,
    pub container: Option<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    pub nsfw_field: Option<String>,
    pub cache: Option<InlineCacheEntry>,
}

#[derive(Debug, Deserialize, Clone)]
/// Inline fetched-option cache configuration.
pub struct InlineCacheEntry {
    /// Cache lifetime in seconds.
    #[serde(default = "default_cache_ttl")]
    pub ttl: u32,
    pub key: String,
}

fn default_cache_ttl() -> u32 {
    3600
}

#[derive(Debug, Deserialize, Clone, Default)]
/// Composite identifier encoding declarations for manga and chapter identifiers.
pub struct IdEncodingBlock {
    pub manga: Option<IdEncodingEntry>,
    pub chapter: Option<IdEncodingEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IdEncodingEntry {
    pub fields: Vec<String>,
    #[serde(default = "default_id_delimiter")]
    pub delimiter: String,
    #[serde(default)]
    pub encoding: YamlIdEncoding,
}

fn default_id_delimiter() -> String {
    "|".to_string()
}

#[derive(Debug, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum YamlIdEncoding {
    #[default]
    Base64Url,
    Base64,
    Passthrough,
    Hex,
}

impl YamlIdEncoding {
    pub(crate) fn to_ast(self) -> kani_shared::ast::IdEncoding {
        match self {
            YamlIdEncoding::Base64Url => kani_shared::ast::IdEncoding::Base64Url,
            YamlIdEncoding::Base64 => kani_shared::ast::IdEncoding::Base64,
            YamlIdEncoding::Passthrough => kani_shared::ast::IdEncoding::Passthrough,
            YamlIdEncoding::Hex => kani_shared::ast::IdEncoding::Hex,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
/// A cache namespace hook scripts may use.
pub struct CacheEntry {
    /// Default and maximum entry lifetime in seconds.
    #[serde(default = "default_cache_block_ttl")]
    pub ttl: u32,
    pub max_entries: Option<u32>,
}

fn default_cache_block_ttl() -> u32 {
    3600
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::EndpointBody;

    #[test]
    fn browser_auto_scroll_is_optional_and_can_be_disabled() {
        let omitted: EndpointBody = serde_yaml::from_str("route: /browse").unwrap();
        assert_eq!(omitted.auto_scroll, None);
        let disabled: EndpointBody =
            serde_yaml::from_str("route: /browse\nauto_scroll: false").unwrap();
        assert_eq!(disabled.auto_scroll, Some(false));
    }
}
