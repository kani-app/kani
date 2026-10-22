//! Guest-safe request construction shared by both YAML execution engines.
//!
//! The interpreted `YamlSource` and `kani-cli` codegen both lower declarative
//! endpoints through this implementation. It remains `wasm32`-clean for generated
//! guest code and native-compatible for the interpreter.
//!
//! It covers the request *envelope* — URL substitution, static queries, and filter
//! mapping. Pagination offsets are added by the evaluator, and the extraction
//! result is unpacked separately.

use crate::types::{ActiveFilter, FilterState};
use std::collections::HashMap;

/// How a query parameter's value is sourced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryValue {
    /// A literal string, used verbatim.
    Static(String),
    /// A `$var$` placeholder resolved from the runtime args (by the dot-replaced key).
    Arg(String),
    /// Text with `$var$` placeholders, each replaced by its argument's value.
    Template(String),
}

/// Refuses any URL that is not absolute `http` or `https` with a host. Every place Kani fetches
/// or loads a URL on a source's behalf applies it, so `file:`, `ftp:` or `javascript:` never
/// reach a client or a browser.
pub fn require_http_url(url: &str) -> Result<(), String> {
    let rest = ["https://", "http://"].iter().find_map(|scheme| {
        url.get(..scheme.len())
            .filter(|head| head.eq_ignore_ascii_case(scheme))
            .map(|_| &url[scheme.len()..])
    });
    match rest.and_then(|r| r.chars().next()) {
        Some(c) if !matches!(c, '/' | '?' | '#') && !c.is_whitespace() => Ok(()),
        _ => Err(format!(
            "only http and https URLs can be requested, not {url:?}"
        )),
    }
}

/// Percent-encodes a value for one route slot, so `/`, `?`, `#` or `%` in an id can never change
/// the path's structure. Both backends encode route values with this.
pub fn encode_path_value(value: impl std::fmt::Display) -> String {
    urlencoding::encode(&value.to_string()).into_owned()
}

/// Refuses a header value that would split the header or the request: CR, LF or NUL.
pub fn check_header_value(name: &str, value: &str) -> Result<(), String> {
    if value.contains(['\r', '\n', '\0']) {
        return Err(format!(
            "header {name}: a value may not contain CR or LF or NUL"
        ));
    }
    Ok(())
}

/// The largest request body sent, checked on the final bytes after placeholders are filled and
/// after any hook has replaced the body.
pub const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;

/// Arguments whose values are numbers; a JSON body placeholder standing alone for one of these
/// is written as a JSON number, and every other argument as a JSON string.
pub const NUMERIC_ARGS: &[&str] = &["page", "page_size"];

/// A request body as declared: its kind decides how placeholders are filled and encoded.
#[derive(Debug, Clone, PartialEq)]
#[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
pub enum BodyTemplate {
    /// A JSON value; string leaves may contain `$var$` placeholders.
    Json(serde_json::Value),
    /// Form fields, each value a text template, sent `application/x-www-form-urlencoded`.
    Form(Vec<(String, String)>),
    /// A text template sent as-is with the declared content type.
    Raw {
        content: String,
        content_type: String,
    },
}

#[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
impl BodyTemplate {
    /// Every `$var$` placeholder the template fills, in order of appearance.
    pub fn placeholders(&self) -> Vec<String> {
        fn json_strings(v: &serde_json::Value, out: &mut Vec<String>) {
            match v {
                serde_json::Value::String(s) => out.push(s.clone()),
                serde_json::Value::Array(a) => a.iter().for_each(|v| json_strings(v, out)),
                serde_json::Value::Object(m) => m.values().for_each(|v| json_strings(v, out)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        match self {
            BodyTemplate::Json(v) => json_strings(v, &mut texts),
            BodyTemplate::Form(fields) => texts.extend(fields.iter().map(|(_, v)| v.clone())),
            BodyTemplate::Raw { content, .. } => texts.push(content.clone()),
        }
        texts.iter().flat_map(|t| template_parts(t).1).collect()
    }
}

/// Refuses a body larger than [`MAX_REQUEST_BODY_BYTES`].
pub fn check_body_size(len: usize) -> Result<(), String> {
    if len > MAX_REQUEST_BODY_BYTES {
        return Err(format!(
            "request body is {len} bytes, over the {MAX_REQUEST_BODY_BYTES}-byte limit"
        ));
    }
    Ok(())
}

/// Fills a body template's placeholders from `args` and encodes it, returning the content type
/// and the exact bytes to send. Shared by the interpreted and generated backends.
#[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
pub fn render_body(
    template: &BodyTemplate,
    args: &HashMap<String, String>,
) -> Result<(String, Vec<u8>), String> {
    let fill = |text: &str| {
        interpolate(text, args).ok_or_else(|| format!("body: unresolved placeholder in {text:?}"))
    };
    let (content_type, bytes) = match template {
        BodyTemplate::Json(value) => {
            let rendered = render_json(value, &fill)?;
            let bytes = serde_json::to_vec(&rendered).map_err(|e| e.to_string())?;
            ("application/json".to_string(), bytes)
        }
        BodyTemplate::Form(fields) => {
            let mut out = String::new();
            for (i, (key, value)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push('&');
                }
                out.push_str(&form_encode(key));
                out.push('=');
                out.push_str(&form_encode(&fill(value)?));
            }
            (
                "application/x-www-form-urlencoded".to_string(),
                out.into_bytes(),
            )
        }
        BodyTemplate::Raw {
            content,
            content_type,
        } => (content_type.clone(), fill(content)?.into_bytes()),
    };
    check_body_size(bytes.len())?;
    Ok((content_type, bytes))
}

#[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
fn render_json(
    value: &serde_json::Value,
    fill: &dyn Fn(&str) -> Result<String, String>,
) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    Ok(match value {
        Value::String(text) => {
            let (texts, vars) = template_parts(text);
            match vars.as_slice() {
                [var]
                    if texts.iter().all(String::is_empty)
                        && NUMERIC_ARGS.contains(&var.as_str()) =>
                {
                    let raw = fill(text)?;
                    raw.parse::<i64>()
                        .map(Value::from)
                        .map_err(|_| format!("body: ${var}$ is {raw:?}, not a number"))?
                }
                _ => Value::String(fill(text)?),
            }
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| render_json(v, fill))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), render_json(v, fill)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other.clone(),
    })
}

/// The WHATWG `application/x-www-form-urlencoded` byte serializer: ASCII letters, digits and
/// `*-._` are kept, a space becomes `+`, and every other byte of the UTF-8 text is `%XX`.
pub fn form_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// How a source packs its composite manga id, for resolving `$manga.<field>$` placeholders.
pub struct CompositeId<'a> {
    pub delimiter: &'a str,
    pub encoding: crate::ast::IdEncoding,
    pub fields: &'a [&'a str],
}

/// The source-site URL for a manga, from the extension's `get_url` template. Both backends call
/// this, so a composite id decodes and every placeholder is encoded the same way.
pub fn source_url(
    base_url: &str,
    template: &str,
    manga_id: &str,
    composite: Option<&CompositeId<'_>>,
) -> Result<String, String> {
    let mut args = HashMap::from([("manga_id".to_string(), manga_id.to_string())]);
    if let Some(c) = composite {
        let decoded =
            crate::encoding::decode_composite(manga_id, c.delimiter, &c.encoding, c.fields)
                .map_err(|e| {
                    format!("manga_id {manga_id:?} is not a valid manga id for this source: {e}")
                })?;
        for (field, value) in decoded {
            args.insert(format!("manga_{field}"), value);
        }
    }
    let absolute = ["https://", "http://"].iter().any(|scheme| {
        template
            .get(..scheme.len())
            .is_some_and(|h| h.eq_ignore_ascii_case(scheme))
    });
    let base = if absolute { "" } else { base_url };
    build_url(base, template, &args)
}

/// Splits `template` at its `$var$` placeholders: literal text alternates with placeholder
/// names, starting and ending with text. A `$` that opens no placeholder stays literal.
pub fn template_parts(template: &str) -> (Vec<String>, Vec<String>) {
    let mut texts = vec![String::new()];
    let mut vars = Vec::new();
    let bytes = template.as_bytes();
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
                vars.push(template[start..end].to_string());
                texts.push(String::new());
                i = end + 1;
                continue;
            }
        }
        let ch = template[i..].chars().next().unwrap_or_default();
        if let Some(last) = texts.last_mut() {
            last.push(ch);
        }
        i += ch.len_utf8();
    }
    (texts, vars)
}

/// Replaces each `$var$` in `template` with its argument (looked up by the dot-replaced key),
/// as plain text. `None` when an argument is missing, so a placeholder never reaches the wire.
pub fn interpolate(template: &str, args: &HashMap<String, String>) -> Option<String> {
    let (texts, vars) = template_parts(template);
    let mut out = String::with_capacity(template.len());
    for (i, text) in texts.iter().enumerate() {
        out.push_str(text);
        if let Some(var) = vars.get(i) {
            out.push_str(args.get(&var.replace('.', "_"))?);
        }
    }
    Some(out)
}

/// One declared query parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuerySpec {
    pub key: String,
    pub value: QueryValue,
}

/// How a filter group maps onto query parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterMapping {
    /// One group → one parameter.
    Simple(String),
    /// A `key:dir` selection → a templated key plus an optional direction param.
    SortPair {
        key_template: String,
        direction_param: Option<String>,
    },
    /// A `from:to` text input → two parameters.
    TupleSplit {
        from_param: String,
        to_param: String,
    },
}

/// How multiselect values are serialised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArrayFormat {
    /// `tag=a&tag=b`
    #[default]
    Repeated,
    /// `tag[]=a&tag[]=b`
    Bracket,
    /// `tag=a,b` (joined by `array_separator`)
    CommaSeparated,
}

/// How a boolean checkbox value is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoolFormat {
    #[default]
    TrueFalse,
    OneZero,
    YesNo,
}

/// Presentation options for filter serialisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterFormat {
    pub multiselect: ArrayFormat,
    pub omit_empty: bool,
    pub bool_format: BoolFormat,
    pub array_separator: String,
}

impl Default for FilterFormat {
    fn default() -> Self {
        Self {
            multiselect: ArrayFormat::default(),
            omit_empty: true,
            bool_format: BoolFormat::default(),
            array_separator: ",".to_string(),
        }
    }
}

/// Render a boolean as its configured literal.
pub fn bool_literal(fmt: BoolFormat, value: bool) -> &'static str {
    match (fmt, value) {
        (BoolFormat::TrueFalse, true) => "true",
        (BoolFormat::TrueFalse, false) => "false",
        (BoolFormat::OneZero, true) => "1",
        (BoolFormat::OneZero, false) => "0",
        (BoolFormat::YesNo, true) => "yes",
        (BoolFormat::YesNo, false) => "no",
    }
}

/// Substitute `$var$` placeholders in `route` from `args`, percent-encoding each
/// value as a single path segment, and prepend `base_url`.
///
/// A source-supplied value fills exactly one route slot, so it is encoded: an id
/// like `../admin`, `x?y=1` or `a b` must not smuggle in a path traversal, a
/// query, or a space. An unresolved placeholder is an error, never a literal
/// `$page$` on the wire. Composite-id sub-fields (`$manga.hid$`) are looked up by
/// the dot-replaced key (`manga_hid`).
pub fn build_url(
    base_url: &str,
    route: &str,
    args: &HashMap<String, String>,
) -> Result<String, String> {
    let mut result = String::with_capacity(route.len());
    let bytes = route.as_bytes();
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
                let placeholder = &route[start..end];
                let key = placeholder.replace('.', "_");
                match args.get(&key) {
                    Some(val) => result.push_str(&encode_path_value(val)),
                    None => {
                        return Err(format!(
                            "unresolved route placeholder `${placeholder}$` (no argument supplied)"
                        ));
                    }
                }
                i = end + 1;
            } else {
                result.push('$');
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(format!("{}{}", base_url.trim_end_matches('/'), result))
}

/// Resolve declared query parameters against the runtime args.
pub fn build_queries(
    queries: &[QuerySpec],
    args: &HashMap<String, String>,
) -> Vec<(String, String)> {
    queries
        .iter()
        .filter_map(|e| {
            let val = match &e.value {
                QueryValue::Static(s) => s.clone(),
                QueryValue::Arg(name) => args.get(name.as_str())?.clone(),
                QueryValue::Template(template) => interpolate(template, args)?,
            };
            Some((e.key.clone(), val))
        })
        .collect()
}

/// Maps active filters onto query parameters using the endpoint mapping and
/// formatting contract shared by both execution engines.
pub fn apply_filters(
    filter_mapping: &[(String, FilterMapping)],
    filter_format: Option<&FilterFormat>,
    filters: &[ActiveFilter],
) -> Vec<(String, String)> {
    let bool_fmt = filter_format.map(|f| f.bool_format).unwrap_or_default();
    let omit_empty = filter_format.map(|f| f.omit_empty).unwrap_or(true);
    let array_fmt = filter_format.map(|f| f.multiselect).unwrap_or_default();
    let array_sep = filter_format
        .map(|f| f.array_separator.as_str())
        .unwrap_or(",");

    let mut out: Vec<(String, String)> = Vec::new();

    for f in filters {
        // `group:action` — the action half lets one filter group carry a value in
        // its name, e.g. `genre:include`.
        let (group, action) = f
            .filter_name
            .split_once(':')
            .unwrap_or((f.filter_name.as_str(), ""));

        let Some((_, entry)) = filter_mapping.iter().find(|(k, _)| k == group) else {
            continue;
        };

        match entry {
            FilterMapping::Simple(param) => match &f.state {
                FilterState::Checkbox(true) => {
                    let v = if action.is_empty() {
                        bool_literal(bool_fmt, true).to_string()
                    } else {
                        action.to_string()
                    };
                    out.push((param.clone(), v));
                }
                FilterState::Checkbox(false) if !omit_empty && action.is_empty() => {
                    out.push((param.clone(), bool_literal(bool_fmt, false).to_string()));
                }
                FilterState::Multiselect(values) => match array_fmt {
                    ArrayFormat::Repeated => {
                        for v in values {
                            out.push((param.clone(), v.clone()));
                        }
                    }
                    ArrayFormat::Bracket => {
                        for v in values {
                            out.push((format!("{param}[]"), v.clone()));
                        }
                    }
                    ArrayFormat::CommaSeparated => {
                        out.push((param.clone(), values.join(array_sep)));
                    }
                },
                FilterState::Selection { value, .. } => out.push((param.clone(), value.clone())),
                FilterState::TextInput(s) => out.push((param.clone(), s.clone())),
                _ => {}
            },
            FilterMapping::SortPair {
                key_template,
                direction_param,
            } => {
                if let FilterState::Selection { value, .. } = &f.state
                    && let Some((key_part, dir)) = value.split_once(':')
                {
                    out.push((key_template.replace("{}", key_part), dir.to_string()));
                    if let Some(dir_param) = direction_param {
                        out.push((dir_param.clone(), dir.to_string()));
                    }
                }
            }
            FilterMapping::TupleSplit {
                from_param,
                to_param,
            } => {
                if let FilterState::TextInput(s) = &f.state
                    && let Some((from, to)) = s.split_once(':')
                {
                    out.push((from_param.clone(), from.to_string()));
                    out.push((to_param.clone(), to.to_string()));
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn args(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn build_url_encodes_a_traversal_into_one_segment() {
        let url = build_url(
            "https://s.example",
            "/manga/$manga_id$/x",
            &args(&[("manga_id", "../admin")]),
        )
        .unwrap();
        assert_eq!(url, "https://s.example/manga/..%2Fadmin/x");
    }

    #[test]
    fn build_url_errors_on_unresolved_placeholder() {
        let err = build_url("https://s.example", "/list/$page$", &args(&[])).unwrap_err();
        assert!(err.contains("page"));
    }

    #[test]
    fn checked_grouped_checkboxes_each_send_their_value_and_unchecked_send_nothing() {
        let mapping = vec![("genre".to_string(), FilterMapping::Simple("genre".into()))];
        let format = FilterFormat {
            omit_empty: false,
            ..FilterFormat::default()
        };
        let boxes = [("Action", true), ("Adventure", true), ("Comedy", false)];
        let filters: Vec<ActiveFilter> = boxes
            .iter()
            .map(|(value, on)| ActiveFilter {
                filter_name: format!("genre:{value}"),
                state: FilterState::Checkbox(*on),
            })
            .collect();

        assert_eq!(
            apply_filters(&mapping, Some(&format), &filters),
            vec![
                ("genre".to_string(), "Action".to_string()),
                ("genre".to_string(), "Adventure".to_string()),
            ]
        );
    }

    #[test]
    fn apply_filters_maps_a_multiselect() {
        let mapping = vec![("genre".to_string(), FilterMapping::Simple("g".to_string()))];
        let filters = vec![ActiveFilter {
            filter_name: "genre".to_string(),
            state: FilterState::Multiselect(vec!["a".to_string(), "b".to_string()]),
        }];
        let out = apply_filters(&mapping, None, &filters);
        assert_eq!(
            out,
            vec![
                ("g".to_string(), "a".to_string()),
                ("g".to_string(), "b".to_string())
            ]
        );
    }
}

#[cfg(all(test, any(feature = "host", feature = "builder")))]
mod body_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn args() -> HashMap<String, String> {
        [("query", "say \"hi\" \\ back\nnext é 🐉"), ("page", "2")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_json_body_is_rendered_byte_exact_with_typed_placeholders() {
        let template = BodyTemplate::Json(serde_json::json!({
            "q": "$query$",
            "label": "p$page$",
            "page": "$page$",
            "tags": ["$query$", 3, true, null]
        }));
        let (content_type, bytes) = render_body(&template, &args()).unwrap();
        assert_eq!(content_type, "application/json");
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            r#"{"label":"p2","page":2,"q":"say \"hi\" \\ back\nnext é 🐉","tags":["say \"hi\" \\ back\nnext é 🐉",3,true,null]}"#
        );
    }

    #[test]
    fn a_form_body_is_encoded_once_byte_exact() {
        let template = BodyTemplate::Form(vec![
            ("q".into(), "$query$".into()),
            ("page".into(), "$page$".into()),
        ]);
        let (content_type, bytes) = render_body(&template, &args()).unwrap();
        assert_eq!(content_type, "application/x-www-form-urlencoded");
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "q=say+%22hi%22+%5C+back%0Anext+%C3%A9+%F0%9F%90%89&page=2"
        );
    }

    #[test]
    fn a_body_over_64_kib_after_substitution_is_refused() {
        let mut big = args();
        big.insert("query".into(), "x".repeat(MAX_REQUEST_BODY_BYTES + 1));
        let template = BodyTemplate::Raw {
            content: "$query$".into(),
            content_type: "text/plain".into(),
        };
        let err = render_body(&template, &big).unwrap_err();
        assert!(err.contains("over the 65536-byte limit"), "{err}");
        let fits = BodyTemplate::Raw {
            content: "q=$page$".into(),
            content_type: "text/plain".into(),
        };
        assert_eq!(render_body(&fits, &args()).unwrap().1, b"q=2");
    }
}

#[cfg(test)]
mod source_url_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_composite_id_resolves_to_the_canonical_page_with_each_part_encoded() {
        let id = crate::encoding::encode_composite(
            &["123", "one-piece: part 1/2 & more?"],
            "|",
            &crate::ast::IdEncoding::Base64Url,
        )
        .unwrap();
        let composite = CompositeId {
            delimiter: "|",
            encoding: crate::ast::IdEncoding::Base64Url,
            fields: &["hid", "slug"],
        };
        let url = source_url(
            "https://ex.com/",
            "/series/$manga.hid$/$manga.slug$",
            &id,
            Some(&composite),
        )
        .unwrap();
        assert_eq!(
            url,
            "https://ex.com/series/123/one-piece%3A%20part%201%2F2%20%26%20more%3F"
        );
        let err = source_url(
            "https://ex.com",
            "/s/$manga.slug$",
            "not*base64",
            Some(&composite),
        )
        .unwrap_err();
        assert!(err.contains("is not a valid manga id"), "{err}");
    }

    #[test]
    fn a_plain_or_absolute_template_is_joined_and_encoded() {
        assert_eq!(
            source_url("https://ex.com", "/m/$manga_id$", "a b/c", None).unwrap(),
            "https://ex.com/m/a%20b%2Fc"
        );
        assert_eq!(
            source_url(
                "https://ex.com",
                "https://other.example/m/$manga_id$",
                "x",
                None
            )
            .unwrap(),
            "https://other.example/m/x"
        );
    }
}
