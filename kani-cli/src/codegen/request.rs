//! Emit Rust source for the HTTP request block of an endpoint method.

use crate::yaml::model::{QueryEntry, QueryValue};
use crate::yaml::schema::{ArrayFormat, BoolFormat, FilterFormatCfg, FilterMappingEntry};

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_request_block(
    route: &str,
    method: &str,
    headers: &[(String, String)],
    queries: &[QueryEntry],
    filter_mapping: &[(String, FilterMappingEntry)],
    filter_format: Option<&FilterFormatCfg>,
    endpoint_id: Option<&str>,
    body: Option<&kani_shared::request::BodyTemplate>,
) -> String {
    let mut lines = Vec::new();

    let ctor = match method.to_uppercase().as_str() {
        "POST" => "post",
        "PUT" => "put",
        "DELETE" => "delete",
        _ => "get",
    };

    let url_expr = emit_route_format(route, "self.base_url");
    let mutability = if filter_mapping.is_empty() {
        ""
    } else {
        "mut "
    };
    lines.push(format!(
        "let {mutability}req = HttpRequest::{ctor}({url_expr})"
    ));

    if let Some(id) = endpoint_id {
        lines.push(format!("    .endpoint_id(\"{id}\")"));
    }

    for (k, v) in headers {
        let value = if kani_shared::request::template_parts(v).1.is_empty() {
            format!("{v:?}")
        } else {
            emit_text_format(v)
        };
        lines.push(format!("    .header({k:?}, {value})"));
    }

    for entry in queries {
        let val = match &entry.value {
            QueryValue::Static(s) => format!("{s:?}"),
            QueryValue::Arg(name) => name.replace('.', "_"),
            QueryValue::Template(template) => emit_text_format(template),
        };
        lines.push(format!("    .query(\"{}\", {})", entry.key, val));
    }

    let last = lines.pop().expect("at least one line");
    lines.push(format!("{last};"));

    if !filter_mapping.is_empty() {
        lines.push(String::new());
        lines.push(emit_filter_apply(filter_mapping, filter_format));
    }

    if let Some(body) = body {
        lines.push(emit_body(body));
    }

    lines.join("\n")
}

fn emit_body(body: &kani_shared::request::BodyTemplate) -> String {
    use kani_shared::request::BodyTemplate;
    let mut vars = body.placeholders();
    vars.sort();
    vars.dedup();
    let args: String = vars
        .iter()
        .map(|v| {
            format!(
                "({:?}.to_string(), {}.to_string()), ",
                v.replace('.', "_"),
                v.replace('.', "_")
            )
        })
        .collect();
    let template = match body {
        BodyTemplate::Json(value) => format!(
            "kani_shared::request::BodyTemplate::Json(kani_shared::serde_json::from_str({:?}).map_err(|e| kani_shared::ExtensionError::internal(e.to_string()))?)",
            value.to_string()
        ),
        BodyTemplate::Form(fields) => {
            let pairs: String = fields
                .iter()
                .map(|(k, v)| format!("({k:?}.to_string(), {v:?}.to_string()), "))
                .collect();
            format!("kani_shared::request::BodyTemplate::Form(vec![{pairs}])")
        }
        BodyTemplate::Raw {
            content,
            content_type,
        } => format!(
            "kani_shared::request::BodyTemplate::Raw {{ content: {content:?}.to_string(), content_type: {content_type:?}.to_string() }}"
        ),
    };
    format!(
        "let body_args: std::collections::HashMap<String, String> = [{args}].into_iter().collect();\n\
         let (body_type, body_bytes) = kani_shared::request::render_body(&{template}, &body_args)\n    \
         .map_err(kani_shared::ExtensionError::invalid_input)?;\n\
         let req = req.header(\"Content-Type\", body_type).body(body_bytes);"
    )
}

/// Emit a call to the shared `kani_shared::request::apply_filters`, with the
/// endpoint's mapping and format rendered as literals so codegen and interpretation share the
/// same filter semantics.
fn emit_filter_apply(
    filter_mapping: &[(String, FilterMappingEntry)],
    filter_format: Option<&FilterFormatCfg>,
) -> String {
    let mapping: Vec<String> = filter_mapping
        .iter()
        .map(|(group, entry)| {
            let fm = match entry {
                FilterMappingEntry::Simple(param) => format!(
                    "kani_shared::request::FilterMapping::Simple({param:?}.to_string())"
                ),
                FilterMappingEntry::SortPair {
                    key_template,
                    direction_param,
                    ..
                } => {
                    let dir = match direction_param {
                        Some(d) => format!("Some({d:?}.to_string())"),
                        None => "None".to_string(),
                    };
                    format!(
                        "kani_shared::request::FilterMapping::SortPair {{ key_template: {key_template:?}.to_string(), direction_param: {dir} }}"
                    )
                }
                FilterMappingEntry::TupleSplit {
                    from_param,
                    to_param,
                    ..
                } => format!(
                    "kani_shared::request::FilterMapping::TupleSplit {{ from_param: {from_param:?}.to_string(), to_param: {to_param:?}.to_string() }}"
                ),
            };
            format!("({group:?}.to_string(), {fm})")
        })
        .collect();

    let format_lit = match filter_format {
        Some(f) => {
            let arr = match f.multiselect {
                ArrayFormat::Default | ArrayFormat::Repeated => "Repeated",
                ArrayFormat::Bracket => "Bracket",
                ArrayFormat::CommaSeparated => "CommaSeparated",
            };
            let boolf = match f.bool_format {
                BoolFormat::TrueFalse => "TrueFalse",
                BoolFormat::OneZero => "OneZero",
                BoolFormat::YesNo => "YesNo",
            };
            format!(
                "Some(kani_shared::request::FilterFormat {{ multiselect: kani_shared::request::ArrayFormat::{arr}, omit_empty: {omit}, bool_format: kani_shared::request::BoolFormat::{boolf}, array_separator: {sep:?}.to_string() }})",
                omit = f.omit_empty,
                sep = f.array_separator,
            )
        }
        None => "None".to_string(),
    };

    format!(
        "let __filter_mapping: [(String, kani_shared::request::FilterMapping); {n}] = [{mapping}];\n\
         let __filter_format: Option<kani_shared::request::FilterFormat> = {format_lit};\n\
         for (k, v) in kani_shared::request::apply_filters(&__filter_mapping, __filter_format.as_ref(), filters) {{\n\
             req = req.query(k, v);\n\
         }}",
        n = mapping.len(),
        mapping = mapping.join(", "),
    )
}

/// Convert a route with `$var$` placeholders into a `format!(...)` call.
/// A placeholder may contain a single `.` (e.g. `$manga.hid$`) to reference a
/// composite-id subfield; these are sanitized to `manga_hid` since `.` is not
/// a valid Rust identifier character (the matching local is emitted by the
/// decode prologue).
fn emit_text_format(template: &str) -> String {
    let (texts, vars) = kani_shared::request::template_parts(template);
    let fmt: String = texts
        .iter()
        .map(|t| t.replace('{', "{{").replace('}', "}}"))
        .collect::<Vec<_>>()
        .join("{}");
    let args: String = vars
        .iter()
        .map(|v| format!(", {}", v.replace('.', "_")))
        .collect();
    format!("format!({fmt:?}{args})")
}

pub(crate) fn emit_route_format(route: &str, base_url_expr: &str) -> String {
    let mut vars: Vec<String> = Vec::new();
    let mut fmt = String::new();
    let bytes = route.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                fmt.push_str("{{");
                i += 1;
            }
            b'}' => {
                fmt.push_str("}}");
                i += 1;
            }
            b'$' => {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric()
                        || bytes[end] == b'_'
                        || bytes[end] == b'.')
                {
                    end += 1;
                }
                if end < bytes.len() && bytes[end] == b'$' && end > start {
                    vars.push(route[start..end].replace('.', "_"));
                    fmt.push_str("{}");
                    i = end + 1;
                } else {
                    fmt.push('$');
                    i += 1;
                }
            }
            b => {
                fmt.push(b as char);
                i += 1;
            }
        }
    }

    let mut args = vec![base_url_expr.to_string()];
    args.extend(
        vars.into_iter()
            .map(|v| format!("kani_shared::request::encode_path_value(&{v})")),
    );
    format!("format!(\"{{}}{fmt}\", {})", args.join(", "))
}
