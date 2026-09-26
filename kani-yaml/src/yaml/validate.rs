//! Semantic validation: DSL parsing, variable references, and required field checks.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use kani_shared::ast::Expr;

use super::model::{
    CompositeIdDecode, FieldSource, QueryEntry, QueryValue, ValidatedBinding, ValidatedCacheEntry,
    ValidatedChapterSort, ValidatedChapterSortOption, ValidatedEndpoint, ValidatedExtension,
    ValidatedField, ValidatedForEachStep, ValidatedHnp, ValidatedMetadata, ValidatedPopular,
    ValidatedRateLimit, ValidatedSection, ValidatedThenStep, ValidatedTotalPages,
};
use super::schema::{
    CURRENT_SCHEMA_VERSION, CacheEntry, ChapterSortBlock, EndpointBody, FilterEntry,
    FilterFormatCfg, FilterKind, FilterMappingEntry, ForEachStep, HasNextPage, IdEncodingBlock,
    MetadataBlock, OnFailure, OptionSetDef, PopularEndpoint, PreferenceEntry, SectionEntry,
    ThenStep, TotalPages, YamlExtension,
};
use crate::dsl::parse as parse_dsl_expression;
use crate::error::YamlError;
use kani_shared::ast::OnFailurePolicy;

const POPULAR_ARGS: &[&str] = &["page", "page_size", "filters"];
const SEARCH_ARGS: &[&str] = &["query", "page", "page_size", "filters"];
const DETAILS_ARGS: &[&str] = &["manga_id"];
const CHAPTER_LIST_ARGS: &[&str] = &["manga_id", "page", "page_size"];
const PAGES_ARGS: &[&str] = &["chapter_id", "manga_id"];

const MANGA_LIST_REQUIRED: &[&str] = &["id", "title"];
const DETAILS_REQUIRED: &[&str] = &["id", "title", "status"];
const CHAPTER_LIST_REQUIRED: &[&str] = &["id"];
const PAGES_REQUIRED: &[&str] = &["url", "index"];

pub fn validate(
    ext: &YamlExtension,
    _source: &str,
    path: &Path,
) -> Result<ValidatedExtension, Vec<YamlError>> {
    let mut errors: Vec<YamlError> = Vec::new();
    let filename = path.to_string_lossy().into_owned();

    if !kani_shared::types::is_valid_extension_id(&ext.id) {
        errors.push(YamlError::Validation(format!(
            "id: '{}' must match [a-z][a-z0-9-]* (lowercase letter first, then lowercase \
             letters, digits and hyphens)",
            ext.id
        )));
    }

    let id_encoding = ext.id_encoding.as_ref();
    if let Some(block) = id_encoding {
        errors.append(&mut validate_id_encoding(block));
    }

    let popular = ext.endpoints.popular.as_ref().and_then(|p| {
        match validate_popular(
            p,
            &filename,
            &ext.filters,
            id_encoding,
            &ext.browser_scripts,
        ) {
            Ok(vp) => Some(vp),
            Err(mut errs) => {
                errors.append(&mut errs);
                None
            }
        }
    });

    let search = ext.endpoints.search.as_ref().and_then(|e| {
        match validate_endpoint(
            e,
            "search",
            SEARCH_ARGS,
            MANGA_LIST_REQUIRED,
            &filename,
            id_encoding,
            &ext.browser_scripts,
        ) {
            Ok(ve) => Some(ve),
            Err(mut errs) => {
                errors.append(&mut errs);
                None
            }
        }
    });

    let manga_details =
        ext.endpoints.manga_details.as_ref().and_then(|e| {
            match validate_endpoint(
                e,
                "manga_details",
                DETAILS_ARGS,
                DETAILS_REQUIRED,
                &filename,
                id_encoding,
                &ext.browser_scripts,
            ) {
                Ok(ve) => Some(ve),
                Err(mut errs) => {
                    errors.append(&mut errs);
                    None
                }
            }
        });

    let chapter_list = ext.endpoints.chapter_list.as_ref().and_then(|e| {
        match validate_endpoint(
            e,
            "chapter_list",
            CHAPTER_LIST_ARGS,
            CHAPTER_LIST_REQUIRED,
            &filename,
            id_encoding,
            &ext.browser_scripts,
        ) {
            Ok(ve) => Some(ve),
            Err(mut errs) => {
                errors.append(&mut errs);
                None
            }
        }
    });

    let pages = ext.endpoints.pages.as_ref().and_then(|e| {
        match validate_endpoint(
            e,
            "pages",
            PAGES_ARGS,
            PAGES_REQUIRED,
            &filename,
            id_encoding,
            &ext.browser_scripts,
        ) {
            Ok(ve) => Some(ve),
            Err(mut errs) => {
                errors.append(&mut errs);
                None
            }
        }
    });

    errors.append(&mut validate_browser_scripts(&ext.browser_scripts));
    errors.append(&mut validate_pure_scripts(&ext.scripts.pure));
    errors.append(&mut validate_hook_scripts(ext));

    // Validate filter IDs: non-empty, no whitespace, no leading/trailing ':', at most one ':'.
    for filter in &ext.filters {
        let id = &filter.id;
        if id.is_empty() {
            errors.push(YamlError::Validation(format!(
                "filters: filter ID must not be empty (filter name: '{}')",
                filter.name
            )));
        } else if id.chars().any(|c| c.is_whitespace()) {
            errors.push(YamlError::Validation(format!(
                "filters: filter ID '{}' must not contain whitespace",
                id
            )));
        } else if id.starts_with(':') || id.ends_with(':') {
            errors.push(YamlError::Validation(format!(
                "filters: filter ID '{}' must not start or end with ':'",
                id
            )));
        } else if id.chars().filter(|&c| c == ':').count() > 1 {
            errors.push(YamlError::Validation(format!(
                "filters: filter ID '{}' must contain at most one ':' separator",
                id
            )));
        }
    }

    errors.append(&mut validate_filters_and_option_sets(
        &ext.filters,
        &ext.preferences,
        &ext.option_sets,
    ));

    let cache = match validate_cache(&ext.cache) {
        Ok(c) => c,
        Err(mut errs) => {
            errors.append(&mut errs);
            vec![]
        }
    };

    let metadata = match validate_metadata(ext.metadata.as_ref()) {
        Ok(m) => m,
        Err(mut errs) => {
            errors.append(&mut errs);
            ValidatedMetadata::default()
        }
    };

    errors.append(&mut validate_versioning(
        ext.schema_version,
        ext.min_kani_version.as_deref(),
    ));

    let chapter_sort =
        ext.chapter_sort
            .as_ref()
            .and_then(|block| match validate_chapter_sort(block) {
                Ok(cs) => Some(cs),
                Err(mut errs) => {
                    errors.append(&mut errs);
                    None
                }
            });

    // Cross-validate chaining step endpoint references.
    let known_endpoint_names: std::collections::HashSet<&str> = [
        ext.endpoints.popular.as_ref().map(|_| "popular"),
        ext.endpoints.search.as_ref().map(|_| "search"),
        ext.endpoints
            .manga_details
            .as_ref()
            .map(|_| "manga_details"),
        ext.endpoints.chapter_list.as_ref().map(|_| "chapter_list"),
        ext.endpoints.pages.as_ref().map(|_| "pages"),
    ]
    .into_iter()
    .flatten()
    .collect();

    let chained_endpoints: [(&str, Option<&ValidatedEndpoint>); 5] = [
        (
            "popular",
            popular.as_ref().and_then(|p| match p {
                ValidatedPopular::Full(ep) => Some(ep.as_ref()),
                _ => None,
            }),
        ),
        ("search", search.as_ref()),
        ("manga_details", manga_details.as_ref()),
        ("chapter_list", chapter_list.as_ref()),
        ("pages", pages.as_ref()),
    ];
    for (ep_name, ep_opt) in &chained_endpoints {
        let Some(ep) = ep_opt else { continue };
        for step in &ep.then_steps {
            if !known_endpoint_names.contains(step.endpoint_name.as_str()) {
                errors.push(YamlError::Validation(format!(
                    "endpoints.{ep_name}.then: endpoint '{}' is not defined",
                    step.endpoint_name
                )));
            }
        }
        for step in &ep.for_each_steps {
            if !known_endpoint_names.contains(step.endpoint_name.as_str()) {
                errors.push(YamlError::Validation(format!(
                    "endpoints.{ep_name}.for_each: endpoint '{}' is not defined",
                    step.endpoint_name
                )));
            }
        }
    }

    if errors.is_empty() {
        Ok(ValidatedExtension {
            id: ext.id.clone(),
            name: ext.name.clone(),
            version: ext.version.clone(),
            base_url: ext.base_url.clone(),
            language: ext.language.clone(),
            nsfw: ext.nsfw,
            unrestricted_http: ext.unrestricted_http,
            popular,
            search,
            manga_details,
            chapter_list,
            pages,
            filters: ext.filters.clone(),
            preferences: ext.preferences.clone(),
            option_sets: ext.option_sets.clone(),
            get_url: ext.get_url.clone(),
            mihon_source_id: ext.mihon_source_id,
            id_encoding: ext.id_encoding.clone(),
            cache,
            metadata,
            schema_version: ext.schema_version,
            min_kani_version: ext.min_kani_version.clone(),
            requires_capabilities: ext.requires_capabilities.clone(),
            chapter_sort,
            browser_scripts: ext.browser_scripts.clone(),
            pure_scripts: ext.scripts.pure.clone(),
            pre_request: ext.pre_request.clone(),
            on_status: ext.on_status.clone(),
            endpoint_pre_request: collect_endpoint_pre_requests(ext),
            endpoint_on_status: collect_endpoint_on_status(ext),
        })
    } else {
        Err(errors)
    }
}

/// Cross-validates `filters`/`preferences` against top-level `option_sets`:
/// every `options_ref` must resolve, and `int_range`/`date_range` filters
/// require `min`+`max`.
fn validate_filters_and_option_sets(
    filters: &[FilterEntry],
    preferences: &[PreferenceEntry],
    option_sets: &BTreeMap<String, OptionSetDef>,
) -> Vec<YamlError> {
    let mut errors = Vec::new();

    for filter in filters {
        if let Some(options_ref) = &filter.options_ref
            && !option_sets.contains_key(options_ref)
        {
            errors.push(YamlError::Validation(format!(
                "filters.{}: options_ref '{}' does not match any entry in option_sets",
                filter.id, options_ref
            )));
        }

        if matches!(filter.kind, FilterKind::IntRange | FilterKind::DateRange)
            && (filter.min.is_none() || filter.max.is_none())
        {
            errors.push(YamlError::Validation(format!(
                "filters.{}: {:?} filters require both 'min' and 'max'",
                filter.id, filter.kind
            )));
        }
    }

    for pref in preferences {
        if let Some(options_ref) = &pref.options_ref
            && !option_sets.contains_key(options_ref)
        {
            errors.push(YamlError::Validation(format!(
                "preferences.{}: options_ref '{}' does not match any entry in option_sets",
                pref.key, options_ref
            )));
        }
    }

    for (name, def) in option_sets {
        if name.is_empty() {
            errors.push(YamlError::Validation(
                "option_sets: option set name must not be empty".to_string(),
            ));
        }
        if let OptionSetDef::Fetched { options_fetched_by } = def {
            if options_fetched_by.route.is_empty() {
                errors.push(YamlError::Validation(format!(
                    "option_sets.{name}.options_fetched_by: 'route' must not be empty"
                )));
            }
            if let Some(cache) = &options_fetched_by.cache {
                const MAX_TTL_SECONDS: u32 = 30 * 24 * 60 * 60;
                if cache.key.is_empty() {
                    errors.push(YamlError::Validation(format!(
                        "option_sets.{name}.options_fetched_by.cache: 'key' must not be empty"
                    )));
                }
                if cache.ttl > MAX_TTL_SECONDS {
                    errors.push(YamlError::Validation(format!(
                        "option_sets.{name}.options_fetched_by.cache: ttl must not exceed 30 days ({MAX_TTL_SECONDS} seconds)"
                    )));
                }
            }
        }
    }

    errors
}

/// Validates a `filter_format` block: `array_separator` must be non-empty
/// when `multiselect: comma_separated` is used.
fn validate_filter_format(fmt: &FilterFormatCfg, endpoint: &str) -> Vec<YamlError> {
    let mut errors = Vec::new();
    if fmt.array_separator.is_empty() {
        errors.push(YamlError::Validation(format!(
            "endpoints.{endpoint}.filter_format: 'array_separator' must not be empty"
        )));
    }
    errors
}

/// Validates the top-level `cache` block: names must be non-empty and free of
/// `:`/`/` (host namespace strings forbid them), and `ttl` must not exceed 30
/// days. Returns the validated entries when there are no errors.
fn validate_cache(
    cache: &BTreeMap<String, CacheEntry>,
) -> Result<Vec<ValidatedCacheEntry>, Vec<YamlError>> {
    const MAX_TTL_SECONDS: u32 = 30 * 24 * 60 * 60;
    let mut errors = Vec::new();
    let mut entries = Vec::new();

    if cache.len() > kani_shared::MAX_CACHE_NAMESPACES {
        errors.push(YamlError::Validation(format!(
            "cache: at most {} namespaces may be declared, found {}",
            kani_shared::MAX_CACHE_NAMESPACES,
            cache.len()
        )));
    }

    for (name, entry) in cache {
        if name.is_empty() {
            errors.push(YamlError::Validation(
                "cache: namespace name must not be empty".to_string(),
            ));
        } else if name.contains(':') || name.contains('/') {
            errors.push(YamlError::Validation(format!(
                "cache.{name}: namespace name must not contain ':' or '/'"
            )));
        }

        if entry.ttl > MAX_TTL_SECONDS {
            errors.push(YamlError::Validation(format!(
                "cache.{name}: ttl must not exceed 30 days ({MAX_TTL_SECONDS} seconds)"
            )));
        }

        if let Some(max) = entry.max_entries
            && !(1..=kani_shared::MAX_CACHE_NAMESPACE_ENTRIES).contains(&max)
        {
            errors.push(YamlError::Validation(format!(
                "cache.{name}: max_entries must be between 1 and {}",
                kani_shared::MAX_CACHE_NAMESPACE_ENTRIES
            )));
        }

        entries.push(ValidatedCacheEntry {
            name: name.clone(),
            ttl: entry.ttl,
            max_entries: entry.max_entries,
        });
    }

    if errors.is_empty() {
        Ok(entries)
    } else {
        Err(errors)
    }
}

/// Validates the optional `metadata` block: icon must be valid base64 decoding
/// to a PNG/WebP/SVG payload no larger than 64KB; rate_limit.rps must be
/// positive; section ids must be non-empty.
fn validate_metadata(
    metadata: Option<&MetadataBlock>,
) -> Result<ValidatedMetadata, Vec<YamlError>> {
    const MAX_ICON_BYTES: usize = 64 * 1024;
    let mut errors = Vec::new();

    let Some(metadata) = metadata else {
        return Ok(ValidatedMetadata::default());
    };

    if let Some(icon) = &metadata.icon {
        use base64::Engine;
        match base64::engine::general_purpose::STANDARD.decode(icon) {
            Ok(bytes) => {
                if bytes.len() > MAX_ICON_BYTES {
                    errors.push(YamlError::Validation(format!(
                        "metadata.icon: decoded icon is {} bytes, exceeding the {MAX_ICON_BYTES}-byte limit",
                        bytes.len()
                    )));
                }
                if !is_known_image_format(&bytes) {
                    errors.push(YamlError::Validation(
                        "metadata.icon: decoded bytes do not match a supported PNG/WebP/SVG signature".to_string(),
                    ));
                }
            }
            Err(e) => {
                errors.push(YamlError::Validation(format!(
                    "metadata.icon: not valid base64: {e}"
                )));
            }
        }
    }

    let rate_limit = metadata.rate_limit.as_ref().map(|cfg| {
        if cfg.rps <= 0.0 {
            errors.push(YamlError::Validation(
                "metadata.rate_limit.rps: must be greater than 0".to_string(),
            ));
        }
        ValidatedRateLimit {
            requests_per_second: cfg.rps,
            burst: cfg.burst,
            max_concurrent: cfg.max_concurrent,
            max_hook_requests: cfg.max_hook_requests,
        }
    });

    let mut seen_ids: HashSet<&str> = HashSet::new();
    let mut sections = Vec::new();
    for SectionEntry { id, name, nsfw } in &metadata.sections {
        if id.is_empty() {
            errors.push(YamlError::Validation(
                "metadata.sections: section 'id' must not be empty".to_string(),
            ));
        } else if !seen_ids.insert(id.as_str()) {
            errors.push(YamlError::Validation(format!(
                "metadata.sections: duplicate section id '{id}'"
            )));
        }
        sections.push(ValidatedSection {
            id: id.clone(),
            name: name.clone(),
            nsfw: *nsfw,
        });
    }

    if errors.is_empty() {
        Ok(ValidatedMetadata {
            icon: metadata.icon.clone(),
            rate_limit,
            languages: metadata.languages.clone(),
            description: metadata.description.clone(),
            sections,
        })
    } else {
        Err(errors)
    }
}

/// Recognizes the magic bytes of the icon formats we support: PNG, WebP
/// (RIFF....WEBP), and SVG (sniffed as UTF-8 text starting with `<`, ignoring
/// leading whitespace/BOM).
fn is_known_image_format(bytes: &[u8]) -> bool {
    const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.starts_with(PNG_MAGIC) {
        return true;
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return true;
    }
    let text = String::from_utf8_lossy(bytes);
    text.trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with('<')
}

/// Validates the `chapter_sort` block: `options` must be non-empty, each
/// option id must be non-empty, and `default` (when present) must name one of
/// the declared option ids.
fn validate_chapter_sort(block: &ChapterSortBlock) -> Result<ValidatedChapterSort, Vec<YamlError>> {
    let mut errors = Vec::new();

    if block.options.is_empty() {
        errors.push(YamlError::Validation(
            "chapter_sort: 'options' must not be empty".to_string(),
        ));
    }

    for opt in &block.options {
        if opt.id.is_empty() {
            errors.push(YamlError::Validation(
                "chapter_sort: option 'id' must not be empty".to_string(),
            ));
        }
    }

    if let Some(default) = &block.default
        && !block.options.iter().any(|o| &o.id == default)
    {
        errors.push(YamlError::Validation(format!(
            "chapter_sort: default '{}' is not listed in options",
            default
        )));
    }

    if errors.is_empty() {
        Ok(ValidatedChapterSort {
            default: block.default.clone(),
            options: block
                .options
                .iter()
                .map(|o| ValidatedChapterSortOption {
                    id: o.id.clone(),
                    label: o.label.clone(),
                })
                .collect(),
        })
    } else {
        Err(errors)
    }
}

/// Validates top-level schema/version metadata: `schema_version` must not
/// exceed the version this `kani-cli` understands, and `min_kani_version`
/// (when present) must be a valid semver requirement string.
fn validate_versioning(schema_version: u32, min_kani_version: Option<&str>) -> Vec<YamlError> {
    let mut errors = Vec::new();

    if schema_version > CURRENT_SCHEMA_VERSION {
        errors.push(YamlError::Validation(format!(
            "schema_version: {schema_version} is newer than the schema version this kani-cli supports ({CURRENT_SCHEMA_VERSION})"
        )));
    }

    if let Some(v) = min_kani_version
        && semver::Version::parse(v).is_err()
    {
        errors.push(YamlError::Validation(format!(
            "min_kani_version: '{v}' is not a valid semver version"
        )));
    }

    errors
}

/// Validates the top-level `id_encoding` block: each declared role's `fields`
/// must be non-empty with no duplicates, and `delimiter` must be non-empty
/// when more than one field is declared.
fn validate_id_encoding(block: &IdEncodingBlock) -> Vec<YamlError> {
    let mut errors = Vec::new();
    for (role, entry) in [("manga", &block.manga), ("chapter", &block.chapter)] {
        let Some(entry) = entry else { continue };
        if entry.fields.is_empty() {
            errors.push(YamlError::Validation(format!(
                "id_encoding.{role}: 'fields' must not be empty"
            )));
        }
        if entry.fields.len() > 1 && entry.delimiter.is_empty() {
            errors.push(YamlError::Validation(format!(
                "id_encoding.{role}: 'delimiter' must not be empty when more than one field is declared"
            )));
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for f in &entry.fields {
            if f.is_empty() {
                errors.push(YamlError::Validation(format!(
                    "id_encoding.{role}: field names must not be empty"
                )));
            } else if !seen.insert(f.as_str()) {
                errors.push(YamlError::Validation(format!(
                    "id_encoding.{role}: duplicate field name '{f}'"
                )));
            }
        }
    }
    errors
}

/// Maps an `id_encoding` role to the Rust function argument that carries its
/// encoded composite ID.
fn role_fn_arg(role: &str) -> Option<&'static str> {
    match role {
        "manga" => Some("manga_id"),
        "chapter" => Some("chapter_id"),
        _ => None,
    }
}

/// Endpoints that implicitly produce a manga or chapter `id` field, used to
/// resolve which `id_encoding` role a `fields.id: { ... }` composite map refers to.
fn default_composite_role(endpoint_name: &str) -> Option<&'static str> {
    match endpoint_name {
        "popular" | "search" | "manga_details" => Some("manga"),
        "chapter_list" => Some("chapter"),
        _ => None,
    }
}

fn id_encoding_entry_for_role<'a>(
    id_encoding: Option<&'a IdEncodingBlock>,
    role: &str,
) -> Option<&'a super::schema::IdEncodingEntry> {
    let block = id_encoding?;
    match role {
        "manga" => block.manga.as_ref(),
        "chapter" => block.chapter.as_ref(),
        _ => None,
    }
}

/// Validates a single `$var$` placeholder (from a route or query value).
/// Plain vars must be one of `fn_args`; dotted vars (`role.field`) must
/// reference a configured `id_encoding` role/field whose backing fn arg is
/// available on this endpoint.
fn validate_dollar_var(
    var: &str,
    endpoint: &str,
    location: &str,
    fn_args: &[&str],
    id_encoding: Option<&IdEncodingBlock>,
) -> Option<YamlError> {
    if let Some((role, field)) = var.split_once('.') {
        let Some(fn_arg) = role_fn_arg(role) else {
            return Some(YamlError::Validation(format!(
                "endpoints.{endpoint}.{location}: '${var}$' has unknown id_encoding role '{role}' (expected 'manga' or 'chapter')"
            )));
        };
        if !fn_args.contains(&fn_arg) {
            return Some(YamlError::Validation(format!(
                "endpoints.{endpoint}.{location}: '${var}$' is not available ('{fn_arg}' is not an argument of this endpoint)"
            )));
        }
        match id_encoding_entry_for_role(id_encoding, role) {
            None => Some(YamlError::Validation(format!(
                "endpoints.{endpoint}.{location}: '${var}$' references id_encoding.{role}, but no such block is declared"
            ))),
            Some(entry) if !entry.fields.iter().any(|f| f == field) => {
                Some(YamlError::Validation(format!(
                    "endpoints.{endpoint}.{location}: '${var}$' references field '{field}', which is not declared in id_encoding.{role}.fields"
                )))
            }
            Some(_) => None,
        }
    } else if !fn_args.contains(&var) {
        Some(YamlError::Validation(format!(
            "endpoints.{endpoint}.{location}: '${var}$' is not available \
             (available args: {})",
            fn_args.join(", ")
        )))
    } else {
        None
    }
}

/// Scans a route template and query map for dotted `role.field` placeholders
/// and builds the per-role decode descriptors codegen needs to emit
/// `decode_composite(...)` binding locals.
fn collect_composite_id_decodes(
    route: &str,
    queries: &BTreeMap<String, String>,
    id_encoding: Option<&IdEncodingBlock>,
) -> Vec<CompositeIdDecode> {
    let Some(id_encoding) = id_encoding else {
        return vec![];
    };

    let mut vars = extract_dollar_vars(route);
    for v in queries.values() {
        let trimmed = v.trim();
        if trimmed.starts_with('$') && trimmed.ends_with('$') && trimmed.len() > 2 {
            vars.push(trimmed[1..trimmed.len() - 1].to_string());
        }
    }

    let mut by_role: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for v in &vars {
        if let Some((role, field)) = v.split_once('.')
            && let Some(canonical) = match role {
                "manga" => Some("manga"),
                "chapter" => Some("chapter"),
                _ => None,
            }
            && id_encoding_entry_for_role(Some(id_encoding), canonical).is_some()
        {
            by_role
                .entry(canonical)
                .or_default()
                .push(field.to_string());
        }
    }

    by_role
        .into_iter()
        .filter_map(|(role, mut referenced)| {
            let entry = id_encoding_entry_for_role(Some(id_encoding), role)?;
            let fn_arg = role_fn_arg(role)?;
            referenced.sort();
            referenced.dedup();
            Some(CompositeIdDecode {
                role: role.to_string(),
                fn_arg: fn_arg.to_string(),
                fields: entry.fields.clone(),
                delimiter: entry.delimiter.clone(),
                encoding: entry.encoding,
                referenced_fields: referenced,
            })
        })
        .collect()
}

/// Builds an `Expr::EncodedField` for a `fields.id: { subfield: dsl, ... }`
/// composite map, validating the map's keys against the resolved
/// `id_encoding` role's declared `fields`.
fn build_composite_field(
    composite: &BTreeMap<String, String>,
    endpoint_name: &str,
    field_name: &str,
    field_path_base: &str,
    id_encoding: Option<&IdEncodingBlock>,
) -> Result<Expr, Vec<YamlError>> {
    let mut errors = Vec::new();
    let Some(role) = default_composite_role(endpoint_name) else {
        return Err(vec![YamlError::Validation(format!(
            "endpoints.{endpoint_name}.fields.{field_name}: composite id fields are not supported on this endpoint"
        ))]);
    };
    let Some(entry) = id_encoding_entry_for_role(id_encoding, role) else {
        return Err(vec![YamlError::Validation(format!(
            "endpoints.{endpoint_name}.fields.{field_name}: declares a composite id, but id_encoding.{role} is not configured"
        ))]);
    };

    let declared: HashSet<&str> = entry.fields.iter().map(String::as_str).collect();
    let provided: HashSet<&str> = composite.keys().map(String::as_str).collect();
    if declared != provided {
        let mut provided_sorted: Vec<&str> = provided.into_iter().collect();
        provided_sorted.sort();
        return Err(vec![YamlError::Validation(format!(
            "endpoints.{endpoint_name}.fields.{field_name}: composite id keys {:?} do not match id_encoding.{role}.fields {:?}",
            provided_sorted, entry.fields
        ))]);
    }

    let mut subfields = Vec::new();
    for fname in &entry.fields {
        let Some(dsl) = composite.get(fname) else {
            continue;
        };
        let field_path = format!("{field_path_base}.{fname}");
        match parse_dsl(dsl, &field_path) {
            Ok(expr) => subfields.push((fname.clone(), expr)),
            Err(mut errs) => errors.append(&mut errs),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(Expr::encoded_field(
        subfields,
        entry.delimiter.clone(),
        entry.encoding.to_ast(),
    ))
}

fn validate_popular(
    popular: &PopularEndpoint,
    filename: &str,
    _filters: &[FilterEntry],
    id_encoding: Option<&IdEncodingBlock>,
    browser_scripts: &std::collections::BTreeMap<String, String>,
) -> Result<ValidatedPopular, Vec<YamlError>> {
    match popular {
        PopularEndpoint::Delegated {
            delegate_to,
            empty_without_filters,
        } => {
            let valid = ["search", "manga_details", "chapter_list", "pages"];
            if !valid.contains(&delegate_to.as_str()) {
                return Err(vec![YamlError::Validation(format!(
                    "popular.delegate_to '{}' must be one of: search, manga_details, chapter_list, pages",
                    delegate_to
                ))]);
            }
            Ok(ValidatedPopular::Delegated {
                delegate_to: delegate_to.clone(),
                empty_without_filters: *empty_without_filters,
            })
        }
        PopularEndpoint::Full(body) => {
            let endpoint = validate_endpoint(
                body,
                "popular",
                POPULAR_ARGS,
                MANGA_LIST_REQUIRED,
                filename,
                id_encoding,
                browser_scripts,
            )?;
            Ok(ValidatedPopular::Full(Box::new(endpoint)))
        }
    }
}

fn validate_endpoint(
    body: &EndpointBody,
    name: &str,
    fn_args: &[&str],
    required_fields: &[&str],
    filename: &str,
    id_encoding: Option<&IdEncodingBlock>,
    browser_scripts: &std::collections::BTreeMap<String, String>,
) -> Result<ValidatedEndpoint, Vec<YamlError>> {
    use super::schema::EndpointVia;
    let mut errors: Vec<YamlError> = Vec::new();

    let (via, page_url, script_name) = if let Some(via) = body.via {
        match via {
            EndpointVia::BrowserPayload => {
                let page_url = match &body.page_url {
                    Some(u) if !u.is_empty() => Some(u.clone()),
                    _ => {
                        errors.push(YamlError::Validation(format!(
                            "endpoints.{name}: 'page_url' is required when 'via: browser_payload' is set"
                        )));
                        None
                    }
                };
                let script_name = match &body.script {
                    Some(s) if !s.is_empty() => {
                        if !browser_scripts.contains_key(s.as_str()) {
                            errors.push(YamlError::Validation(format!(
                                "endpoints.{name}: script '{s}' is not declared in browser_scripts"
                            )));
                        }
                        Some(s.clone())
                    }
                    _ => {
                        errors.push(YamlError::Validation(format!(
                            "endpoints.{name}: 'script' is required when 'via: browser_payload' is set"
                        )));
                        None
                    }
                };
                if body.route.is_some() {
                    eprintln!(
                        "warning: endpoints.{name}: 'route' is ignored when 'via: browser_payload' is set"
                    );
                }
                (Some(via), page_url, script_name)
            }
        }
    } else {
        (None, None, None)
    };

    let route = if via.is_some() {
        page_url.clone().unwrap_or_default()
    } else {
        match &body.route {
            Some(r) => {
                let mut route_errs = validate_route_vars(r, name, fn_args, id_encoding);
                errors.append(&mut route_errs);
                r.clone()
            }
            None => {
                errors.push(YamlError::Validation(format!(
                    "endpoints.{name}: 'route' is required"
                )));
                String::new()
            }
        }
    };

    let container = body
        .container
        .clone()
        .unwrap_or_else(|| ":root".to_string());

    let headers: Vec<(String, String)> = body
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let queries = match build_query_entries(&body.queries, name, fn_args, id_encoding) {
        Ok(q) => q,
        Err(mut errs) => {
            errors.append(&mut errs);
            vec![]
        }
    };

    let composite_id_decodes = collect_composite_id_decodes(&route, &body.queries, id_encoding);

    let filter_mapping: Vec<(String, FilterMappingEntry)> = body
        .filter_mapping
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    for (key, entry) in &filter_mapping {
        if let FilterMappingEntry::SortPair { key_template, .. } = entry
            && !key_template.contains("{}")
        {
            errors.push(YamlError::Validation(format!(
                "endpoints.{name}.filter_mapping.{key}: sort_pair key_template must contain '{{}}'"
            )));
        }
    }

    if let Some(fmt) = &body.filter_format {
        errors.append(&mut validate_filter_format(fmt, name));
    }

    let mut bindings = Vec::new();
    for (var_name, dsl) in &body.bindings {
        let field_path = format!("{filename}:endpoints.{name}.bindings.{var_name}");
        match parse_dsl(dsl, &field_path) {
            Ok(expr) => bindings.push(ValidatedBinding {
                name: var_name.clone(),
                expr,
            }),
            Err(mut errs) => errors.append(&mut errs),
        }
    }

    let mut fields = Vec::new();
    let mut present_fields: HashSet<&str> = HashSet::new();
    for (field_name, def) in &body.fields {
        present_fields.insert(field_name.as_str());
        let field_path = format!("{filename}:endpoints.{name}.fields.{field_name}");

        if let Some(composite) = def.as_composite() {
            match build_composite_field(composite, name, field_name, &field_path, id_encoding) {
                Ok(expr) => fields.push(ValidatedField {
                    name: field_name.clone(),
                    source: FieldSource::Blueprint(expr),
                    optional: false,
                }),
                Err(mut errs) => errors.append(&mut errs),
            }
            continue;
        }

        let dsl = def.expr_str();
        let optional = def.optional();

        let source = if let Some(arg_name) = extract_fn_arg_literal(dsl) {
            if !fn_args.contains(&arg_name.as_str()) {
                errors.push(YamlError::Validation(format!(
                    "endpoints.{name}.fields.{field_name}: '${arg_name}$' is not available \
                     (available args: {})",
                    fn_args.join(", ")
                )));
                continue;
            }
            FieldSource::FnArg(arg_name)
        } else {
            match parse_dsl(dsl, &field_path) {
                Ok(expr) => FieldSource::Blueprint(expr),
                Err(mut errs) => {
                    errors.append(&mut errs);
                    continue;
                }
            }
        };

        fields.push(ValidatedField {
            name: field_name.clone(),
            source,
            optional,
        });
    }

    for req in required_fields {
        if !present_fields.contains(*req) {
            errors.push(YamlError::Validation(format!(
                "endpoints.{name}: required field '{req}' is missing"
            )));
        }
    }

    let mut scalars = Vec::new();
    for (scalar_name, def) in &body.scalars {
        let dsl = def.expr_str();
        let optional = def.optional();
        let field_path = format!("{filename}:endpoints.{name}.scalars.{scalar_name}");

        let source = if let Some(arg_name) = extract_fn_arg_literal(dsl) {
            FieldSource::FnArg(arg_name)
        } else {
            match parse_dsl(dsl, &field_path) {
                Ok(expr) => FieldSource::Blueprint(expr),
                Err(mut errs) => {
                    errors.append(&mut errs);
                    continue;
                }
            }
        };

        scalars.push(ValidatedField {
            name: scalar_name.clone(),
            source,
            optional,
        });
    }

    let has_next_page = match &body.has_next_page {
        None => ValidatedHnp::Default,
        Some(HasNextPage::Static(b)) => ValidatedHnp::Static(*b),
        Some(HasNextPage::Expr(dsl)) => {
            let field_path = format!("{filename}:endpoints.{name}.has_next_page");
            match parse_dsl(dsl, &field_path) {
                Ok(expr) => ValidatedHnp::Scalar(expr),
                Err(mut errs) => {
                    errors.append(&mut errs);
                    ValidatedHnp::Default
                }
            }
        }
    };

    let total_pages = match &body.total_pages {
        None => ValidatedTotalPages::None,
        Some(TotalPages::Static(n)) => ValidatedTotalPages::Static(*n),
        Some(TotalPages::Expr(dsl)) => {
            let field_path = format!("{filename}:endpoints.{name}.total_pages");
            match parse_dsl(dsl, &field_path) {
                Ok(expr) => ValidatedTotalPages::Scalar(expr),
                Err(mut errs) => {
                    errors.append(&mut errs);
                    ValidatedTotalPages::None
                }
            }
        }
    };

    let mut then_steps = Vec::new();
    for (i, step) in body.then.iter().enumerate() {
        let step_path = format!("{filename}:endpoints.{name}.then[{i}]");
        match validate_then_step(step, &step_path) {
            Ok(s) => then_steps.push(s),
            Err(mut errs) => errors.append(&mut errs),
        }
    }

    let mut for_each_steps = Vec::new();
    for (i, step) in body.for_each.iter().enumerate() {
        let step_path = format!("{filename}:endpoints.{name}.for_each[{i}]");
        match validate_for_each_step(step, &step_path) {
            Ok(s) => for_each_steps.push(s),
            Err(mut errs) => errors.append(&mut errs),
        }
    }

    if errors.is_empty() {
        Ok(ValidatedEndpoint {
            route,
            method: body.method.clone(),
            headers,
            queries,
            filter_mapping,
            filter_format: body.filter_format.clone(),
            response_type: body.response_type,
            container,
            bindings,
            fields,
            scalars,
            has_next_page,
            total_pages,
            pagination: body.pagination.clone(),
            composite_id_decodes,
            then_steps,
            for_each_steps,
            via,
            page_url,
            script_name,
            timeout_ms: body.timeout_ms,
            auto_scroll: body
                .auto_scroll
                .unwrap_or(kani_shared::types::DEFAULT_BROWSER_AUTO_SCROLL),
        })
    } else {
        Err(errors)
    }
}

fn compile_on_failure(
    on_failure: Option<&OnFailure>,
    path: &str,
) -> Result<OnFailurePolicy, Vec<YamlError>> {
    match on_failure {
        None | Some(OnFailure::Fail) => Ok(OnFailurePolicy::Fail),
        Some(OnFailure::Skip) => Ok(OnFailurePolicy::Skip),
        Some(OnFailure::Use(dsl)) => {
            let fallback_path = format!("{path}.on_failure");
            parse_dsl(dsl, &fallback_path).map(|expr| OnFailurePolicy::Use(Box::new(expr)))
        }
    }
}

fn validate_then_step(step: &ThenStep, path: &str) -> Result<ValidatedThenStep, Vec<YamlError>> {
    let mut errors = Vec::new();

    if step.merge_as.is_empty() {
        errors.push(YamlError::Validation(format!(
            "{path}: 'merge_as' must not be empty"
        )));
    }

    let url_expr = match parse_dsl(&step.url_expr, &format!("{path}.url_expr")) {
        Ok(e) => Some(e),
        Err(mut errs) => {
            errors.append(&mut errs);
            None
        }
    };

    let on_failure = match compile_on_failure(step.on_failure.as_ref(), path) {
        Ok(p) => p,
        Err(mut errs) => {
            errors.append(&mut errs);
            OnFailurePolicy::Fail
        }
    };

    if errors.is_empty() {
        Ok(ValidatedThenStep {
            url_expr: url_expr.expect("checked"),
            merge_as: step.merge_as.clone(),
            endpoint_name: step.endpoint.clone(),
            on_failure,
        })
    } else {
        Err(errors)
    }
}

fn validate_for_each_step(
    step: &ForEachStep,
    path: &str,
) -> Result<ValidatedForEachStep, Vec<YamlError>> {
    let mut errors = Vec::new();

    if step.merge_as.is_empty() {
        errors.push(YamlError::Validation(format!(
            "{path}: 'merge_as' must not be empty"
        )));
    }

    let url_expr = match parse_dsl(&step.url_expr, &format!("{path}.url_expr")) {
        Ok(e) => Some(e),
        Err(mut errs) => {
            errors.append(&mut errs);
            None
        }
    };

    let deduplicate_by = if let Some(dsl) = &step.deduplicate_by {
        match parse_dsl(dsl, &format!("{path}.deduplicate_by")) {
            Ok(e) => Some(e),
            Err(mut errs) => {
                errors.append(&mut errs);
                None
            }
        }
    } else {
        None
    };

    let on_failure = match compile_on_failure(step.on_failure.as_ref(), path) {
        Ok(p) => p,
        Err(mut errs) => {
            errors.append(&mut errs);
            OnFailurePolicy::Fail
        }
    };

    if errors.is_empty() {
        Ok(ValidatedForEachStep {
            url_expr: url_expr.expect("checked"),
            merge_as: step.merge_as.clone(),
            endpoint_name: step.endpoint.clone(),
            on_failure,
            deduplicate_by,
        })
    } else {
        Err(errors)
    }
}

fn parse_dsl(dsl: &str, field_path: &str) -> Result<Expr, Vec<YamlError>> {
    let parse_ast = parse_dsl_expression(dsl.trim_end()).map_err(|errors| {
        vec![YamlError::DslParse {
            field_path: field_path.to_string(),
            expression: dsl.to_string(),
            errors,
        }]
    })?;
    parse_ast.try_into()
}
/// Detects `"$varname$"` — a DSL string literal wrapping a dollar-fenced identifier.
/// These are emitted as direct function-arg passthroughs rather than blueprint fields.
fn extract_fn_arg_literal(dsl: &str) -> Option<String> {
    let trimmed = dsl.trim();
    if trimmed.len() > 4 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        let inner = &trimmed[1..trimmed.len() - 1];
        if inner.starts_with('$') && inner.ends_with('$') && inner.len() > 2 {
            let name = &inner[1..inner.len() - 1];
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Extracts all `$varname$` placeholders from a route or query string.
/// `varname` may contain a single `.` separator (e.g. `manga.hid`) to
/// reference a composite-id subfield declared in `id_encoding`.
fn extract_dollar_vars(s: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'.')
            {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'$' && end > start {
                vars.push(s[start..end].to_string());
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    vars
}

fn validate_route_vars(
    route: &str,
    endpoint: &str,
    fn_args: &[&str],
    id_encoding: Option<&IdEncodingBlock>,
) -> Vec<YamlError> {
    extract_dollar_vars(route)
        .into_iter()
        .filter_map(|v| validate_dollar_var(&v, endpoint, "route", fn_args, id_encoding))
        .collect()
}

fn build_query_entries(
    queries: &BTreeMap<String, String>,
    endpoint: &str,
    fn_args: &[&str],
    id_encoding: Option<&IdEncodingBlock>,
) -> Result<Vec<QueryEntry>, Vec<YamlError>> {
    let mut entries = Vec::new();
    let mut errors = Vec::new();

    for (key, value) in queries {
        let trimmed = value.trim();
        let query_value = if trimmed.starts_with('$') && trimmed.ends_with('$') && trimmed.len() > 2
        {
            let var = &trimmed[1..trimmed.len() - 1];
            let location = format!("queries.{key}");
            if let Some(err) = validate_dollar_var(var, endpoint, &location, fn_args, id_encoding) {
                errors.push(err);
                continue;
            }
            QueryValue::Arg(var.to_string())
        } else {
            QueryValue::Static(value.clone())
        };
        entries.push(QueryEntry {
            key: key.clone(),
            value: query_value,
        });
    }

    if errors.is_empty() {
        Ok(entries)
    } else {
        Err(errors)
    }
}

fn validate_browser_scripts(
    scripts: &std::collections::BTreeMap<String, String>,
) -> Vec<YamlError> {
    let mut errors = Vec::new();
    for (name, src) in scripts {
        if name.is_empty() {
            errors.push(YamlError::Validation(
                "browser_scripts: script name must not be empty".to_string(),
            ));
        }
        if src.is_empty() {
            errors.push(YamlError::Validation(format!(
                "browser_scripts.{name}: script source must not be empty"
            )));
        } else if !src.contains("passPayload") {
            eprintln!(
                "warning: browser_scripts.{name}: script does not call passPayload — the browser runtime will not receive any data"
            );
        }
    }
    errors
}

fn validate_pure_scripts(scripts: &std::collections::BTreeMap<String, String>) -> Vec<YamlError> {
    let mut errors = Vec::new();
    let engine = make_validation_sandbox();
    for (name, src) in scripts {
        if name.is_empty() {
            errors.push(YamlError::Validation(
                "scripts.pure: function name must not be empty".to_string(),
            ));
        }
        if src.is_empty() {
            errors.push(YamlError::Validation(format!(
                "scripts.pure.{name}: script source must not be empty"
            )));
        } else if let Err(e) = engine.compile(src) {
            errors.push(YamlError::Validation(format!("scripts.pure.{name}: {e}")));
        }
    }
    errors
}

fn validate_hook_body(context: &str, src: &str, engine: &rhai::Engine) -> Vec<YamlError> {
    let mut errors = Vec::new();
    if src.is_empty() {
        errors.push(YamlError::Validation(format!(
            "{context}: hook body must not be empty"
        )));
    } else if let Err(e) = engine.compile(src) {
        errors.push(YamlError::Validation(format!("{context}: {e}")));
    }
    errors
}

fn is_valid_on_status_key(key: &str) -> bool {
    if key == "default" {
        return true;
    }
    let bytes = key.as_bytes();
    if bytes.len() == 3 {
        if bytes.iter().all(|b| b.is_ascii_digit()) {
            return true;
        }
        if bytes[0].is_ascii_digit() && bytes[1] == b'x' && bytes[2] == b'x' {
            return true;
        }
    }
    false
}

fn validate_hook_scripts(ext: &super::schema::YamlExtension) -> Vec<YamlError> {
    let engine = make_validation_sandbox();
    let mut errors = Vec::new();

    if let Some(body) = &ext.pre_request {
        errors.append(&mut validate_hook_body("pre_request", body, &engine));
    }
    for (pattern, body) in &ext.on_status {
        if !is_valid_on_status_key(pattern) {
            errors.push(YamlError::Validation(format!(
                "on_status key `{pattern}` is not valid — use a 3-digit status code (e.g. `401`), a wildcard pattern (e.g. `4xx`), or `default`"
            )));
        }
        errors.append(&mut validate_hook_body(
            &format!("on_status.{pattern}"),
            body,
            &engine,
        ));
    }

    for (ep_name, ep_body) in endpoint_iter(ext) {
        if let Some(body) = &ep_body.pre_request {
            errors.append(&mut validate_hook_body(
                &format!("endpoints.{ep_name}.pre_request"),
                body,
                &engine,
            ));
        }
        for (pattern, body) in &ep_body.on_status {
            if !is_valid_on_status_key(pattern) {
                errors.push(YamlError::Validation(format!(
                    "endpoints.{ep_name}.on_status key `{pattern}` is not valid — use a 3-digit status code, a wildcard pattern (e.g. `4xx`), or `default`"
                )));
            }
            errors.append(&mut validate_hook_body(
                &format!("endpoints.{ep_name}.on_status.{pattern}"),
                body,
                &engine,
            ));
        }
    }
    errors
}

fn endpoint_iter(ext: &super::schema::YamlExtension) -> Vec<(&str, &super::schema::EndpointBody)> {
    use super::schema::PopularEndpoint;
    let mut out = Vec::new();
    if let Some(PopularEndpoint::Full(body)) = &ext.endpoints.popular {
        out.push(("popular", body.as_ref()));
    }
    if let Some(ep) = &ext.endpoints.search {
        out.push(("search", ep));
    }
    if let Some(ep) = &ext.endpoints.manga_details {
        out.push(("manga_details", ep));
    }
    if let Some(ep) = &ext.endpoints.chapter_list {
        out.push(("chapter_list", ep));
    }
    if let Some(ep) = &ext.endpoints.pages {
        out.push(("pages", ep));
    }
    out
}

fn collect_endpoint_pre_requests(
    ext: &super::schema::YamlExtension,
) -> std::collections::BTreeMap<String, String> {
    endpoint_iter(ext)
        .into_iter()
        .filter_map(|(name, ep)| {
            ep.pre_request
                .as_ref()
                .map(|body| (name.to_string(), body.clone()))
        })
        .collect()
}

fn collect_endpoint_on_status(
    ext: &super::schema::YamlExtension,
) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> {
    endpoint_iter(ext)
        .into_iter()
        .filter(|(_, ep)| !ep.on_status.is_empty())
        .map(|(name, ep)| (name.to_string(), ep.on_status.clone()))
        .collect()
}

pub fn make_validation_sandbox() -> rhai::Engine {
    let mut engine = rhai::Engine::new();
    engine.set_max_operations(100_000);
    engine.set_max_expr_depths(64, 32);
    engine.set_max_call_levels(16);
    engine.set_max_string_size(1_000_000);
    engine.set_max_array_size(10_000);
    engine.set_max_map_size(1_000);
    engine.disable_symbol("eval");
    engine.disable_symbol("import");
    engine.disable_symbol("export");
    engine
}

pub fn validate_factory(block: &super::schema::FactoryBlock) -> Vec<YamlError> {
    let mut errors = Vec::new();

    if block.sources.is_empty() {
        errors.push(YamlError::Validation(
            "factory.sources must not be empty".to_string(),
        ));
        return errors;
    }

    let mut seen_ids = std::collections::HashSet::new();
    for source in &block.sources {
        if source.id.is_empty() {
            errors.push(YamlError::Validation(
                "factory.sources: source id must not be empty".to_string(),
            ));
        } else if !seen_ids.insert(source.id.as_str()) {
            errors.push(YamlError::Validation(format!(
                "factory.sources: duplicate source id '{}'",
                source.id
            )));
        }
        if source.base_url.is_empty() {
            errors.push(YamlError::Validation(format!(
                "factory.sources.{}: base_url must not be empty",
                source.id
            )));
        }
        if source.name.is_empty() {
            errors.push(YamlError::Validation(format!(
                "factory.sources.{}: name must not be empty",
                source.id
            )));
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::is_valid_on_status_key;

    #[test]
    fn on_status_key_accepts_specific_codes() {
        assert!(is_valid_on_status_key("401"));
        assert!(is_valid_on_status_key("200"));
        assert!(is_valid_on_status_key("500"));
    }

    #[test]
    fn on_status_key_accepts_wildcard_patterns() {
        assert!(is_valid_on_status_key("4xx"));
        assert!(is_valid_on_status_key("5xx"));
        assert!(is_valid_on_status_key("2xx"));
    }

    #[test]
    fn on_status_key_accepts_default() {
        assert!(is_valid_on_status_key("default"));
    }

    #[test]
    fn on_status_key_rejects_invalid() {
        assert!(!is_valid_on_status_key("40x"));
        assert!(!is_valid_on_status_key("4X0"));
        assert!(!is_valid_on_status_key(""));
        assert!(!is_valid_on_status_key("xx"));
        assert!(!is_valid_on_status_key("4000"));
        assert!(!is_valid_on_status_key("retry"));
    }

    #[test]
    fn validate_hook_body_rejects_broken_rhai() {
        let engine = super::make_validation_sandbox();
        let errors = super::validate_hook_body("pre_request", "let", &engine);
        assert!(!errors.is_empty(), "broken Rhai should produce errors");
    }

    #[test]
    fn validate_hook_body_accepts_valid_rhai() {
        let engine = super::make_validation_sandbox();
        let errors =
            super::validate_hook_body("pre_request", r#"req.set_header("X-Foo", "bar")"#, &engine);
        assert!(
            errors.is_empty(),
            "valid Rhai expression should produce no errors"
        );
    }
}
