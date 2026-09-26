//! Validated intermediate representation shared by interpretation and Rust code generation.

use std::collections::BTreeMap;

use crate::yaml::schema::{
    FilterEntry, FilterFormatCfg, FilterMappingEntry, IdEncodingBlock, OptionSetDef, PaginationCfg,
    PreferenceEntry, ResponseType, YamlIdEncoding,
};
use kani_shared::ast::{Expr, OnFailurePolicy};

/// A fully-validated extension with all DSL strings compiled into `Expr` trees.
#[derive(Default)]
pub struct ValidatedExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    pub base_url: String,
    pub language: String,
    pub nsfw: bool,
    pub unrestricted_http: bool,
    pub popular: Option<ValidatedPopular>,
    pub search: Option<ValidatedEndpoint>,
    pub manga_details: Option<ValidatedEndpoint>,
    pub chapter_list: Option<ValidatedEndpoint>,
    pub pages: Option<ValidatedEndpoint>,
    pub filters: Vec<FilterEntry>,
    pub preferences: Vec<PreferenceEntry>,
    pub option_sets: BTreeMap<String, OptionSetDef>,
    /// Optional URL template, e.g. `"https://example.com/manga/$manga_id$"`.
    pub get_url: Option<String>,
    /// Optional Mihon/Tachiyomi source ID for cross-app import matching.
    pub mihon_source_id: Option<i64>,
    /// Composite ID encode/decode declared for manga and/or chapter IDs.
    pub id_encoding: Option<IdEncodingBlock>,
    pub cache: Vec<ValidatedCacheEntry>,
    pub metadata: ValidatedMetadata,
    /// Schema version this YAML was authored against.
    pub schema_version: u32,
    /// Minimum host (kani-app) semver this extension requires, if any.
    pub min_kani_version: Option<String>,
    pub requires_capabilities: Vec<String>,
    pub chapter_sort: Option<ValidatedChapterSort>,
    /// Named browser scripts (name → JS source), ready for codegen to write as `src/scripts/<name>.js`.
    pub browser_scripts: std::collections::BTreeMap<String, String>,
    /// Named pure Rhai scripts (name → source), ready for codegen to write as `src/scripts/<name>.rhai`.
    pub pure_scripts: std::collections::BTreeMap<String, String>,
    pub pre_request: Option<String>,
    /// Source-level on_status hook bodies keyed by status pattern.
    pub on_status: std::collections::BTreeMap<String, String>,
    /// Per-endpoint pre_request hooks: endpoint name → hook body.
    pub endpoint_pre_request: std::collections::BTreeMap<String, String>,
    /// Per-endpoint on_status hooks: endpoint name → (status pattern → hook body).
    pub endpoint_on_status:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

impl ValidatedExtension {
    /// The declared cache namespaces, as the hook runtime enforces them.
    pub fn cache_limits(
        &self,
    ) -> std::collections::BTreeMap<String, kani_shared::CacheNamespaceLimits> {
        self.cache
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    kani_shared::CacheNamespaceLimits {
                        ttl_seconds: c.ttl,
                        max_entries: c.max_entries,
                    },
                )
            })
            .collect()
    }

    /// Look up a named endpoint by its YAML key.
    pub fn endpoint_by_name(&self, name: &str) -> Option<&ValidatedEndpoint> {
        match name {
            "popular" => self.popular.as_ref().and_then(|p| match p {
                ValidatedPopular::Full(ep) => Some(ep.as_ref()),
                ValidatedPopular::Delegated { .. } => None,
            }),
            "search" => self.search.as_ref(),
            "manga_details" => self.manga_details.as_ref(),
            "chapter_list" => self.chapter_list.as_ref(),
            "pages" => self.pages.as_ref(),
            _ => None,
        }
    }
}

/// Validated chapter-sort declaration with a checked default identifier.
pub struct ValidatedChapterSort {
    pub default: Option<String>,
    pub options: Vec<ValidatedChapterSortOption>,
}

pub struct ValidatedChapterSortOption {
    pub id: String,
    pub label: String,
}

/// Validated extended metadata, ready for codegen to populate into the
/// generated `metadata()` literal.
#[derive(Default)]
pub struct ValidatedMetadata {
    pub icon: Option<String>,
    pub rate_limit: Option<ValidatedRateLimit>,
    pub languages: Vec<String>,
    pub description: Option<String>,
    pub sections: Vec<ValidatedSection>,
}

/// Validated request and hook budget ready for extension metadata emission.
pub struct ValidatedRateLimit {
    pub requests_per_second: f64,
    pub burst: u32,
    pub max_concurrent: u32,
    pub max_hook_requests: u32,
}

pub struct ValidatedSection {
    pub id: String,
    pub name: String,
    pub nsfw: bool,
}

/// A validated entry from the top-level `cache` block: a namespace hook scripts may use.
pub struct ValidatedCacheEntry {
    pub name: String,
    pub ttl: u32,
    pub max_entries: Option<u32>,
}

/// Popular-list operation implemented directly or delegated to another endpoint.
pub enum ValidatedPopular {
    Delegated {
        delegate_to: String,
        empty_without_filters: bool,
    },
    Full(Box<ValidatedEndpoint>),
}

/// Endpoint whose placeholders, expressions, chaining, and output fields have passed validation.
pub struct ValidatedEndpoint {
    pub route: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub queries: Vec<QueryEntry>,
    pub filter_mapping: Vec<(String, FilterMappingEntry)>,
    pub filter_format: Option<FilterFormatCfg>,
    pub response_type: ResponseType,
    pub container: String,
    pub bindings: Vec<ValidatedBinding>,
    pub fields: Vec<ValidatedField>,
    pub scalars: Vec<ValidatedField>,
    pub has_next_page: ValidatedHnp,
    pub total_pages: ValidatedTotalPages,
    pub pagination: Option<PaginationCfg>,
    pub composite_id_decodes: Vec<CompositeIdDecode>,
    /// Document-level chained fetches (→ blueprint bindings).
    pub then_steps: Vec<ValidatedThenStep>,
    /// Per-element chained fetches (→ blueprint fields).
    pub for_each_steps: Vec<ValidatedForEachStep>,
    /// When Some, the endpoint is fetched via headless browser (`capture_page_payload`).
    pub via: Option<crate::yaml::schema::EndpointVia>,
    /// Absolute URL of the page to load. Present iff `via` is Some.
    pub page_url: Option<String>,
    /// Name of a declared browser script. Present iff `via` is Some.
    pub script_name: Option<String>,
    /// Browser page-load timeout in milliseconds.
    pub timeout_ms: u32,
    /// Whether browser capture periodically scrolls the page.
    pub auto_scroll: bool,
}

pub struct ValidatedThenStep {
    pub url_expr: Expr,
    /// Binding name (without `$`) for the fetched result.
    pub merge_as: String,
    /// Name of the endpoint whose blueprint to use for sub-extraction.
    pub endpoint_name: String,
    pub on_failure: OnFailurePolicy,
}

pub struct ValidatedForEachStep {
    pub url_expr: Expr,
    /// Field name in which the fetched result is stored per row.
    pub merge_as: String,
    /// Name of the endpoint whose blueprint to use for sub-extraction.
    pub endpoint_name: String,
    pub on_failure: OnFailurePolicy,
    pub deduplicate_by: Option<Expr>,
}

/// Describes a composite-id decode codegen must emit as a `let` binding
/// prologue, derived from a `$role.field$` placeholder in a route or query.
pub struct CompositeIdDecode {
    /// `"manga"` or `"chapter"`.
    pub role: String,
    /// The Rust function argument holding the encoded composite ID (e.g. `manga_id`).
    pub fn_arg: String,
    /// All fields declared in `id_encoding.<role>.fields`, in encode/decode order.
    pub fields: Vec<String>,
    pub delimiter: String,
    pub encoding: YamlIdEncoding,
    /// Subset of `fields` actually referenced by this endpoint's route/queries.
    pub referenced_fields: Vec<String>,
}

/// Validated source of the `has_next_page` response value.
pub enum ValidatedHnp {
    Static(bool),
    Scalar(Expr),
    /// Default: true (caller checks if a `has_next_page` scalar was added separately).
    Default,
}

/// Validated source of the optional `total_pages` response value.
pub enum ValidatedTotalPages {
    Static(u32),
    Scalar(Expr),
    None,
}

pub struct QueryEntry {
    pub key: String,
    pub value: QueryValue,
}

pub enum QueryValue {
    /// Literal string appended verbatim.
    Static(String),
    /// Single `$var$` placeholder → Rust function-arg identifier.
    Arg(String),
}

pub struct ValidatedBinding {
    /// Variable name without the `$` prefix.
    pub name: String,
    pub expr: Expr,
}

/// Validated output field and whether a missing value is accepted.
pub struct ValidatedField {
    pub name: String,
    pub source: FieldSource,
    pub optional: bool,
}

/// Origin of a validated output field.
pub enum FieldSource {
    /// Expression to include as a blueprint field; evaluated against the document.
    Blueprint(Expr),
    /// Value taken directly from the named Rust function argument (e.g. `manga_id`).
    FnArg(String),
}
