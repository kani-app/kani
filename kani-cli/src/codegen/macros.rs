//! Emit `filter_list!` and `preference_list!` macro invocations from schema entries.

use std::collections::BTreeMap;

use crate::yaml::schema::{
    FilterDefault, FilterEntry, FilterKind, FilterOption, FilterSemantic, OptionSetDef,
    OptionSetItem, PrefOption, PreferenceEntry, PreferenceKind, ResponseType,
};

pub(crate) fn emit_filter_list(
    filters: &[FilterEntry],
    option_sets: &BTreeMap<String, OptionSetDef>,
) -> String {
    if filters.is_empty() {
        return "Ok(filter_list!{})".to_string();
    }
    let mut entries = Vec::new();
    for f in filters {
        entries.push(emit_filter_entry(f, option_sets));
    }
    format!("Ok(filter_list!{{\n{}\n}})", entries.join(";\n"))
}

/// Resolves `options_ref` against `option_sets`: static sets are inlined as
/// `FilterOption`s; fetched sets resolve to an empty list (populated by the
/// host at render time in a later phase).
fn resolve_options<'a>(
    f: &'a FilterEntry,
    option_sets: &'a BTreeMap<String, OptionSetDef>,
) -> Vec<FilterOption> {
    if let Some(options_ref) = &f.options_ref {
        match option_sets.get(options_ref) {
            Some(OptionSetDef::Static(items)) => items
                .iter()
                .map(|i: &OptionSetItem| FilterOption {
                    name: i.name.clone(),
                    value: i.value.clone(),
                    nsfw: i.nsfw,
                })
                .collect(),
            Some(OptionSetDef::Fetched { .. }) | None => Vec::new(),
        }
    } else {
        f.options.clone()
    }
}

fn emit_filter_entry(f: &FilterEntry, option_sets: &BTreeMap<String, OptionSetDef>) -> String {
    let id = escape_str(&f.id);
    let name = escape_str(&f.name);
    let resolved_options = resolve_options(f, option_sets);
    let semantic_suffix = f
        .semantic
        .as_ref()
        .map(|s| {
            let tag = match s {
                FilterSemantic::Author => "kani_shared::wit_types::FilterSemantic::Author",
                FilterSemantic::Artist => "kani_shared::wit_types::FilterSemantic::Artist",
                FilterSemantic::Tag => "kani_shared::wit_types::FilterSemantic::Tag",
            };
            format!(", semantic: {tag}")
        })
        .unwrap_or_default();

    match f.kind {
        FilterKind::Checkbox => {
            let def = match &f.default {
                Some(FilterDefault::Bool(b)) => format!(", default: {b}"),
                _ => String::new(),
            };
            format!("    \"{id}\", \"{name}\", Checkbox{def}{semantic_suffix}")
        }

        FilterKind::Select | FilterKind::Sort => {
            let kind = if f.kind == FilterKind::Sort {
                "Sort"
            } else {
                "Select"
            };
            let opts = emit_name_value_opts(
                resolved_options
                    .iter()
                    .map(|o| (o.name.as_str(), o.value.as_str())),
            );
            let def = match &f.default {
                Some(FilterDefault::Option { name: n, value: v }) => {
                    format!(", default: (\"{}\", \"{}\")", escape_str(n), escape_str(v))
                }
                Some(FilterDefault::Text(s)) => format!(", default: (\"{}\")", escape_str(s)),
                _ => String::new(),
            };
            format!("    \"{id}\", \"{name}\", {kind}, [{opts}]{def}{semantic_suffix}")
        }

        FilterKind::Multiselect => {
            let opts = emit_opts_multiselect(&resolved_options);
            format!("    \"{id}\", \"{name}\", Multiselect, [{opts}]{semantic_suffix}")
        }

        FilterKind::TextInput | FilterKind::IntRange | FilterKind::DateRange => {
            let def = match &f.default {
                Some(FilterDefault::Text(s)) => format!(", default: \"{}\"", escape_str(s)),
                _ => String::new(),
            };
            format!("    \"{id}\", \"{name}\", TextInput{def}{semantic_suffix}")
        }
    }
}

/// Emits `("Name", "value")` tuples, the form Select, Sort, and preference option
/// lists all take. Shared so the escaping cannot diverge between them.
fn emit_name_value_opts<'a>(opts: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    opts.into_iter()
        .map(|(name, value)| format!("(\"{}\", \"{}\")", escape_str(name), escape_str(value)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Emit Multiselect options: bare string if name==value, else tuple.
fn emit_opts_multiselect(opts: &[FilterOption]) -> String {
    opts.iter()
        .map(|o| {
            if o.name == o.value {
                format!("\"{}\"", escape_str(&o.name))
            } else {
                format!(
                    "(\"{}\", \"{}\")",
                    escape_str(&o.name),
                    escape_str(&o.value)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn emit_preference_list(
    prefs: &[PreferenceEntry],
    option_sets: &BTreeMap<String, OptionSetDef>,
) -> String {
    if prefs.is_empty() {
        return "Ok(vec![])".to_string();
    }
    let mut entries = Vec::new();
    for p in prefs {
        entries.push(emit_pref_entry(p, option_sets));
    }
    format!("Ok(preference_list![\n{}\n])", entries.join(";\n"))
}

fn resolve_pref_options(
    p: &PreferenceEntry,
    option_sets: &BTreeMap<String, OptionSetDef>,
) -> Vec<PrefOption> {
    if let Some(options_ref) = &p.options_ref {
        match option_sets.get(options_ref) {
            Some(OptionSetDef::Static(items)) => items
                .iter()
                .map(|i: &OptionSetItem| PrefOption {
                    name: i.name.clone(),
                    value: i.value.clone(),
                })
                .collect(),
            Some(OptionSetDef::Fetched { .. }) | None => Vec::new(),
        }
    } else {
        p.options.clone()
    }
}

fn emit_pref_entry(p: &PreferenceEntry, option_sets: &BTreeMap<String, OptionSetDef>) -> String {
    let key = escape_str(&p.key);
    let label = escape_str(&p.label);
    let desc_suffix = p
        .description
        .as_ref()
        .map(|d| format!(", description: \"{}\"", escape_str(d)))
        .unwrap_or_default();

    match p.kind {
        PreferenceKind::Toggle => {
            let def = p.default.as_str() == "true";
            format!("    \"{key}\", \"{label}\", Toggle, default: {def}{desc_suffix}")
        }

        PreferenceKind::Select => {
            let resolved = resolve_pref_options(p, option_sets);
            let opts =
                emit_name_value_opts(resolved.iter().map(|o| (o.name.as_str(), o.value.as_str())));
            let def = escape_str(&p.default);
            format!("    \"{key}\", \"{label}\", Select, [{opts}], default: \"{def}\"{desc_suffix}")
        }

        PreferenceKind::Text => {
            let def = escape_str(&p.default);
            let secret_suffix = if p.secret { ", secret: true" } else { "" };
            format!(
                "    \"{key}\", \"{label}\", Text, default: \"{def}\"{desc_suffix}{secret_suffix}"
            )
        }

        PreferenceKind::MultiValueList => {
            format!("    \"{key}\", \"{label}\", MultiValueList{desc_suffix}")
        }
    }
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Builds the JSON array literal (as a Rust string) for `get_fetched_option_sets`.
/// For each filter with an `options_ref` pointing to a `Fetched` option_set, emits
/// one `FilterFetchDef` entry so the host can fetch and merge options at render time.
pub(crate) fn emit_fetched_option_sets(
    filters: &[FilterEntry],
    option_sets: &BTreeMap<String, OptionSetDef>,
) -> String {
    let entries: Vec<String> = filters
        .iter()
        .filter_map(|f| {
            let options_ref = f.options_ref.as_ref()?;
            let OptionSetDef::Fetched { options_fetched_by: def } = option_sets.get(options_ref)? else {
                return None;
            };
            let response_type = match def.response_type {
                ResponseType::Html => "html",
                ResponseType::Json => "json",
            };
            let container = def.container.as_deref().map(|c| format!("\"{}\"", c.replace('\\', "\\\\").replace('"', "\\\""))).unwrap_or_else(|| "null".to_string());
            let fields: String = def.fields.iter()
                .map(|(k, v)| format!("\"{}\":\"{}\"", escape_str(k), escape_str(v)))
                .collect::<Vec<_>>()
                .join(",");
            let (cache_key, cache_ttl) = match &def.cache {
                Some(c) => (format!("\"{}\"", escape_str(&c.key)), c.ttl),
                None => ("null".to_string(), 300),
            };
            let nsfw_field = def.nsfw_field.as_deref()
                .map(|s| format!("\"{}\"", escape_str(s)))
                .unwrap_or_else(|| "null".to_string());
            Some(format!(
                "{{\"filter_id\":\"{}\",\"option_set_name\":\"{}\",\"route\":\"{}\",\"response_type\":\"{}\",\"container\":{},\"fields\":{{{}}},\"nsfw_field\":{},\"cache_key\":{},\"cache_ttl\":{}}}",
                escape_str(&f.id),
                escape_str(options_ref),
                escape_str(&def.route),
                response_type,
                container,
                fields,
                nsfw_field,
                cache_key,
                cache_ttl,
            ))
        })
        .collect();
    format!("r#\"[{}]\"#", entries.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_emit_as_name_then_value_tuples() {
        let opts = [("Action", "1"), ("Comedy", "4")];
        assert_eq!(
            emit_name_value_opts(opts),
            r#"("Action", "1"), ("Comedy", "4")"#,
            "the display name comes first and the wire value second"
        );
    }

    /// The output is pasted into a Rust string literal, so a quote or backslash in
    /// a source's own option list would end the literal early and the generated
    /// crate would not compile.
    #[test]
    fn quotes_and_backslashes_in_an_option_are_escaped() {
        assert_eq!(
            emit_name_value_opts([(r#"He said "hi""#, r"a\b")]),
            r#"("He said \"hi\"", "a\\b")"#
        );
    }

    #[test]
    fn an_empty_option_list_emits_nothing() {
        assert_eq!(emit_name_value_opts([]), "");
    }
}
