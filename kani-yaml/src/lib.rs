//! Authoritative parser, validator, and lowering pipeline for declarative Kani extensions.
//!
//! YAML input is first deserialized into [`YamlExtension`], validated and compiled into
//! [`ValidatedExtension`], then lowered into the same extraction blueprints consumed by generated
//! WASM guests and the interpreted runtime.

pub mod dsl;
pub mod error;
pub mod yaml;

pub use error::YamlError;
pub use yaml::model::{ValidatedEndpoint, ValidatedExtension};
pub use yaml::schema::YamlExtension;

/// Parse and validate a YAML extension from source text.
pub fn parse_and_validate(
    text: &str,
    path: &std::path::Path,
) -> Result<ValidatedExtension, Vec<YamlError>> {
    let ext: YamlExtension = serde_yaml::from_str(text)
        .map_err(|e| vec![YamlError::Validation(format!("YAML parse error: {e}"))])?;

    yaml::validate::validate(&ext, text, path)
}

/// Build a `Blueprint` from a validated endpoint at runtime, with a request attached.
///
/// Used by the interpreted YAML tier, which always has a live `RequestDef` in hand.
/// Codegen (`kani-cli`) uses [`build_blueprint_core`] directly instead, since its
/// `emit_blueprint_bytes` path serialises the blueprint at build time with no request
/// attached (the request is built and attached separately in the generated Rust).
pub fn build_blueprint(
    ep: &yaml::model::ValidatedEndpoint,
    ext: &yaml::model::ValidatedExtension,
    endpoint_name: &str,
    req: kani_shared::ast::RequestDef,
) -> kani_shared::ast::Blueprint {
    build_blueprint_core(ep, ext, endpoint_name)
        .with_request(req)
        .build()
}

/// Build the `BlueprintBuilder` for a validated endpoint: bindings, `then`/`for_each`
/// sub-fetches, fields, scalars, `has_next_page`, and pagination — everything except the
/// request, which callers attach (or omit) as needed.
///
/// Shared by the interpreted tier ([`build_blueprint`]) and `kani-cli`'s codegen
/// (`emit_blueprint_bytes`), so the two consumption paths can't silently diverge.
pub fn build_blueprint_core(
    ep: &yaml::model::ValidatedEndpoint,
    ext: &yaml::model::ValidatedExtension,
    endpoint_name: &str,
) -> kani_shared::ast::BlueprintBuilder {
    use kani_shared::ast::{BlueprintBuilder, OffsetType};
    use yaml::model::{FieldSource, ValidatedHnp};
    use yaml::schema::YamlOffsetType;

    let mut builder = BlueprintBuilder::new(&ep.container);

    for b in &ep.bindings {
        builder = builder.bind(&b.name, b.expr.clone());
    }

    for step in &ep.then_steps {
        if let Some(sub_ep) = ext.endpoint_by_name(&step.endpoint_name) {
            let endpoint_id = Some(format!("{endpoint_name}/{}", step.merge_as));
            let fetch = make_fetch_expr(&step.url_expr, sub_ep, &step.on_failure, endpoint_id);
            builder = builder.bind(&step.merge_as, fetch);
        }
    }

    for f in &ep.fields {
        if let FieldSource::Blueprint(expr) = &f.source {
            if f.optional {
                builder = builder.field_opt(&f.name, expr.clone());
            } else {
                builder = builder.field(&f.name, expr.clone());
            }
        }
    }

    for step in &ep.for_each_steps {
        if let Some(sub_ep) = ext.endpoint_by_name(&step.endpoint_name) {
            let endpoint_id = Some(format!("{endpoint_name}/{}", step.merge_as));
            let fetch = make_fetch_expr(&step.url_expr, sub_ep, &step.on_failure, endpoint_id);
            builder = builder.field(&step.merge_as, fetch);
        }
    }

    for s in &ep.scalars {
        if let FieldSource::Blueprint(expr) = &s.source {
            if s.optional {
                builder = builder.scalar_opt(&s.name, expr.clone());
            } else {
                builder = builder.scalar(&s.name, expr.clone());
            }
        }
    }

    if let ValidatedHnp::Scalar(expr) = &ep.has_next_page {
        builder = builder.scalar("has_next_page", expr.clone());
    }

    if let Some(pag) = &ep.pagination {
        let offset_type = match pag.offset_type {
            YamlOffsetType::Item => OffsetType::ItemOffset,
            YamlOffsetType::Page => OffsetType::PageNumber {
                start: pag.page_start,
            },
        };
        builder = builder.paginated(pag.native_page_size, &pag.offset_param, offset_type);
    }

    builder
}

/// Build a request-free, non-chaining sub-blueprint for `then` and `for_each` fetch expressions.
pub(crate) fn build_sub_blueprint(
    ep: &yaml::model::ValidatedEndpoint,
) -> kani_shared::ast::Blueprint {
    use kani_shared::ast::BlueprintBuilder;
    use yaml::model::FieldSource;

    let mut builder = BlueprintBuilder::new(&ep.container);
    for b in &ep.bindings {
        builder = builder.bind(&b.name, b.expr.clone());
    }
    for f in &ep.fields {
        if let FieldSource::Blueprint(expr) = &f.source {
            if f.optional {
                builder = builder.field_opt(&f.name, expr.clone());
            } else {
                builder = builder.field(&f.name, expr.clone());
            }
        }
    }
    for s in &ep.scalars {
        if let FieldSource::Blueprint(expr) = &s.source {
            if s.optional {
                builder = builder.scalar_opt(&s.name, expr.clone());
            } else {
                builder = builder.scalar(&s.name, expr.clone());
            }
        }
    }
    builder.build()
}

/// Build the `Expr::Fetch` node for a `then`/`for_each` step: constructs the sub-endpoint's
/// blueprint, wraps it in a `fetch_html`/`fetch_json` expr, and attaches endpoint-id + on-failure.
pub fn make_fetch_expr(
    url_expr: &kani_shared::ast::Expr,
    sub_ep: &yaml::model::ValidatedEndpoint,
    on_failure: &kani_shared::ast::OnFailurePolicy,
    endpoint_id: Option<String>,
) -> kani_shared::ast::Expr {
    use kani_shared::ast::Expr;
    use yaml::schema::ResponseType;

    let sub_bp = build_sub_blueprint(sub_ep);
    let fetch = match sub_ep.response_type {
        ResponseType::Html => Expr::fetch_html(url_expr.clone(), sub_bp),
        ResponseType::Json => Expr::fetch_json(url_expr.clone(), sub_bp),
    };
    let fetch = if let Some(id) = endpoint_id {
        fetch.with_endpoint_id(id)
    } else {
        fetch
    };
    fetch.with_on_failure(on_failure.clone())
}

/// Build a URL by substituting `$var$` placeholders in `route` from `args`.
///
/// Delegates to [`kani_shared::request::build_url`] — the single implementation
/// both YAML engines share. Composite-id sub-field placeholders (`$manga.hid$`)
/// are looked up by the dot-replaced key (`manga_hid`).
pub fn build_url_with_args(
    base_url: &str,
    route: &str,
    args: &std::collections::HashMap<String, String>,
) -> Result<String, String> {
    kani_shared::request::build_url(base_url, route, args)
}

impl From<&yaml::model::QueryEntry> for kani_shared::request::QuerySpec {
    fn from(e: &yaml::model::QueryEntry) -> Self {
        use yaml::model::QueryValue as Y;
        kani_shared::request::QuerySpec {
            key: e.key.clone(),
            value: match &e.value {
                Y::Static(s) => kani_shared::request::QueryValue::Static(s.clone()),
                Y::Arg(a) => kani_shared::request::QueryValue::Arg(a.clone()),
            },
        }
    }
}

/// Resolve query parameters from an endpoint's query list and a runtime args map.
pub fn build_queries(
    entries: &[yaml::model::QueryEntry],
    args: &std::collections::HashMap<String, String>,
) -> Vec<(String, String)> {
    let specs: Vec<kani_shared::request::QuerySpec> = entries.iter().map(Into::into).collect();
    kani_shared::request::build_queries(&specs, args)
}

/// Decode a single composite id `arg_name` in `args` against `entry`, inserting each
/// decoded sub-field back into `args` as `<role>_<field>`. A no-op when `arg_name`
/// isn't present, or when decoding fails (a malformed id is left for the caller's
/// own build_url to reject as an unresolved placeholder).
fn decode_composite_arg(
    entry: &yaml::schema::IdEncodingEntry,
    role: &str,
    arg_name: &str,
    args: &mut std::collections::HashMap<String, String>,
) {
    use kani_shared::ast::IdEncoding;
    use yaml::schema::YamlIdEncoding;

    let Some(raw_id) = args.get(arg_name).cloned() else {
        return;
    };
    let encoding = match entry.encoding {
        YamlIdEncoding::Base64Url => IdEncoding::Base64Url,
        YamlIdEncoding::Base64 => IdEncoding::Base64,
        YamlIdEncoding::Passthrough => IdEncoding::Passthrough,
        YamlIdEncoding::Hex => IdEncoding::Hex,
    };
    let field_names: Vec<&str> = entry.fields.iter().map(|f| f.as_str()).collect();
    if let Ok(decoded) =
        kani_shared::encoding::decode_composite(&raw_id, &entry.delimiter, &encoding, &field_names)
    {
        for (field, value) in decoded {
            args.insert(format!("{role}_{field}"), value);
        }
    }
}

/// Decode composite IDs referenced by `ep` and add the decoded sub-fields to `args`.
pub fn resolve_composite_ids(
    ep: &yaml::model::ValidatedEndpoint,
    args: &mut std::collections::HashMap<String, String>,
) {
    for decode in &ep.composite_id_decodes {
        let entry = yaml::schema::IdEncodingEntry {
            fields: decode.fields.clone(),
            delimiter: decode.delimiter.clone(),
            encoding: decode.encoding,
        };
        decode_composite_arg(&entry, &decode.role, &decode.fn_arg, args);
    }
}

/// Decode the `manga_id` arg against the extension's top-level `id_encoding.manga`
/// block and add the decoded sub-fields (`manga_<field>`) to `args`.
///
/// `get_url` is not an endpoint, so it carries no `composite_id_decodes` of its own
/// (those are only collected per-endpoint from a route/page_url — see
/// `collect_composite_id_decodes`); this is the equivalent for the one template that
/// lives outside the endpoints map. A no-op when the extension declares no
/// `id_encoding.manga`, or when `manga_id` isn't present in `args`.
pub fn resolve_get_url_manga_id(
    ext: &yaml::model::ValidatedExtension,
    args: &mut std::collections::HashMap<String, String>,
) {
    if let Some(entry) = ext.id_encoding.as_ref().and_then(|b| b.manga.as_ref()) {
        decode_composite_arg(entry, "manga", "manga_id", args);
    }
}

/// Maps active filters onto query parameters, per an endpoint's `filter_mapping`
/// and `filter_format`.
///
/// This is the reference implementation for compiled and interpreted YAML sources. The
/// `kani-fixture-source` conformance suite requires both paths to emit equivalent requests.
pub fn apply_filters(
    filter_mapping: &[(String, yaml::schema::FilterMappingEntry)],
    filter_format: Option<&yaml::schema::FilterFormatCfg>,
    filters: &[kani_shared::types::ActiveFilter],
) -> Vec<(String, String)> {
    let mapping: Vec<(String, kani_shared::request::FilterMapping)> = filter_mapping
        .iter()
        .map(|(group, entry)| (group.clone(), entry.into()))
        .collect();
    let format = filter_format.map(kani_shared::request::FilterFormat::from);
    kani_shared::request::apply_filters(&mapping, format.as_ref(), filters)
}

impl From<&yaml::schema::FilterMappingEntry> for kani_shared::request::FilterMapping {
    fn from(e: &yaml::schema::FilterMappingEntry) -> Self {
        use kani_shared::request::FilterMapping as S;
        use yaml::schema::FilterMappingEntry as Y;
        match e {
            Y::Simple(p) => S::Simple(p.clone()),
            Y::SortPair {
                key_template,
                direction_param,
                ..
            } => S::SortPair {
                key_template: key_template.clone(),
                direction_param: direction_param.clone(),
            },
            Y::TupleSplit {
                from_param,
                to_param,
                ..
            } => S::TupleSplit {
                from_param: from_param.clone(),
                to_param: to_param.clone(),
            },
        }
    }
}

impl From<&yaml::schema::FilterFormatCfg> for kani_shared::request::FilterFormat {
    fn from(f: &yaml::schema::FilterFormatCfg) -> Self {
        use kani_shared::request::{ArrayFormat as SA, BoolFormat as SB};
        use yaml::schema::{ArrayFormat as YA, BoolFormat as YB};
        kani_shared::request::FilterFormat {
            // The interpreter always treated `Default` as `Repeated`.
            multiselect: match f.multiselect {
                YA::Default | YA::Repeated => SA::Repeated,
                YA::Bracket => SA::Bracket,
                YA::CommaSeparated => SA::CommaSeparated,
            },
            omit_empty: f.omit_empty,
            bool_format: match f.bool_format {
                YB::TrueFalse => SB::TrueFalse,
                YB::OneZero => SB::OneZero,
                YB::YesNo => SB::YesNo,
            },
            array_separator: f.array_separator.clone(),
        }
    }
}

#[cfg(test)]
mod url_tests {
    #![allow(clippy::unwrap_used)]
    use super::build_url_with_args;
    use std::collections::HashMap;

    fn args(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn substitutes_a_plain_id() {
        let url = build_url_with_args(
            "https://src.example/",
            "/manga/$manga_id$",
            &args(&[("manga_id", "abc123")]),
        )
        .unwrap();
        assert_eq!(url, "https://src.example/manga/abc123");
    }

    #[test]
    fn a_source_supplied_id_cannot_rewrite_the_path() {
        let url = build_url_with_args(
            "https://src.example",
            "/manga/$manga_id$/details",
            &args(&[("manga_id", "../admin")]),
        )
        .unwrap();
        assert_eq!(url, "https://src.example/manga/..%2Fadmin/details");

        let url = build_url_with_args(
            "https://src.example",
            "/manga/$manga_id$",
            &args(&[("manga_id", "x?y=1")]),
        )
        .unwrap();
        assert_eq!(url, "https://src.example/manga/x%3Fy%3D1");

        let url = build_url_with_args(
            "https://src.example",
            "/manga/$manga_id$",
            &args(&[("manga_id", "a b")]),
        )
        .unwrap();
        assert_eq!(url, "https://src.example/manga/a%20b");
    }

    #[test]
    fn an_unresolved_placeholder_is_an_error_not_a_literal() {
        let err = build_url_with_args(
            "https://src.example",
            "/list/$page$",
            &args(&[("manga_id", "x")]),
        )
        .unwrap_err();
        assert!(
            err.contains("page"),
            "error should name the placeholder: {err}"
        );
    }

    #[test]
    fn a_lone_dollar_is_kept_literally() {
        let url = build_url_with_args("https://src.example", "/price/$5", &args(&[])).unwrap();
        assert_eq!(url, "https://src.example/price/$5");
    }

    #[test]
    fn get_url_resolves_a_dotted_composite_id() {
        // `resolve_get_url_manga_id` is a thin `id_encoding.manga` lookup around this
        // same decode step, exercised directly here to avoid hand-building the ~25-field
        // `ValidatedExtension` just to reach it.
        use super::decode_composite_arg;
        use super::yaml::schema::{IdEncodingEntry, YamlIdEncoding};

        let entry = IdEncodingEntry {
            fields: vec!["hid".to_string(), "slug".to_string()],
            delimiter: "|".to_string(),
            encoding: YamlIdEncoding::Base64Url,
        };
        let encoded = kani_shared::encoding::encode_composite(
            &["h1", "some-title-slug"],
            "|",
            &kani_shared::ast::IdEncoding::Base64Url,
        )
        .expect("no delimiter in a non-final part");

        let mut resolved = args(&[("manga_id", &encoded)]);
        decode_composite_arg(&entry, "manga", "manga_id", &mut resolved);
        assert_eq!(
            resolved.get("manga_slug").map(String::as_str),
            Some("some-title-slug")
        );

        let url =
            build_url_with_args("https://src.example", "/title/$manga.slug$", &resolved).unwrap();
        assert_eq!(url, "https://src.example/title/some-title-slug");
    }
}
