//! Fluent filter application for extension HTTP requests.

use crate::host_abi::HttpRequest;
use crate::{ActiveFilter, FilterState};
use std::collections::HashMap;

/// Controls how [`FilterState::Multiselect`] values are serialised as query parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayFormat {
    /// `tag=a&tag=b` — repeated keys (default; most REST APIs)
    Repeated,
    /// `tag[]=a&tag[]=b` — PHP/Laravel bracket notation
    Bracket,
    /// `tag=a,b` — single comma-joined value
    CommaSeparated,
}

/// Extension trait for appending [`ActiveFilter`] values to an [`HttpRequest`] as query parameters.
///
/// Uses the `"group:action"` filter-name convention:
/// - The part before `:` is the query parameter key (or remapped via `apply_filters_mapped`).
/// - For [`FilterState::Checkbox`], the part after `:` is used as the value when checked
///   instead of `"true"`.
///
/// # Example
/// ```ignore
/// let req = HttpRequest::get(&url)
///     .query("q", query)
///     .apply_filters(filters);
/// ```
pub trait ApplyFilters: Sized {
    /// Append filters using standard repeated-key format (`tag=a&tag=b`).
    fn apply_filters(self, filters: &[ActiveFilter]) -> Self {
        self.apply_filters_mapped_fmt(filters, &[], ArrayFormat::Repeated)
    }

    /// Append filters with a custom [`ArrayFormat`] for multiselect values.
    fn apply_filters_fmt(self, filters: &[ActiveFilter], fmt: ArrayFormat) -> Self {
        self.apply_filters_mapped_fmt(filters, &[], fmt)
    }

    /// Append filters, translating group names via `mapping`.
    ///
    /// Each entry in `mapping` is `(filter_group, api_param)`. Groups not present in
    /// `mapping` fall through and use the group name directly as the query key.
    fn apply_filters_mapped(self, filters: &[ActiveFilter], mapping: &[(&str, &str)]) -> Self {
        self.apply_filters_mapped_fmt(filters, mapping, ArrayFormat::Repeated)
    }

    /// Append filters with both name remapping and array-format control.
    fn apply_filters_mapped_fmt(
        self,
        filters: &[ActiveFilter],
        mapping: &[(&str, &str)],
        fmt: ArrayFormat,
    ) -> Self;
}

impl ApplyFilters for HttpRequest {
    fn apply_filters_mapped_fmt(
        mut self,
        filters: &[ActiveFilter],
        mapping: &[(&str, &str)],
        fmt: ArrayFormat,
    ) -> Self {
        for f in filters {
            let (group, action) = f
                .filter_name
                .split_once(':')
                .unwrap_or((&f.filter_name, ""));
            let param = mapping
                .iter()
                .find(|(g, _)| *g == group)
                .map_or(group, |(_, p)| p);

            match &f.state {
                FilterState::Multiselect(values) if !values.is_empty() => match fmt {
                    ArrayFormat::Repeated => {
                        for v in values {
                            self = self.query(param, v.as_str());
                        }
                    }
                    ArrayFormat::Bracket => {
                        let key = format!("{}[]", param);
                        for v in values {
                            self = self.query(key.clone(), v.as_str());
                        }
                    }
                    ArrayFormat::CommaSeparated => {
                        self = self.query(param, values.join(","));
                    }
                },
                FilterState::Checkbox(true) => {
                    let value = if action.is_empty() { "true" } else { action };
                    self = self.query(param, value);
                }
                FilterState::TextInput(s) if !s.is_empty() => {
                    self = self.query(param, s.as_str());
                }
                FilterState::Selection { value, .. } if !value.is_empty() => {
                    self = self.query(param, value.as_str());
                }
                _ => {}
            }
        }
        self
    }
}

/// Pre-groups [`ActiveFilter`] values by their `group` component for structured custom dispatch.
///
/// Used when filters cannot be applied 1:1 to query parameters — for example, when a single
/// filter value must map to two API parameters, or when default fallbacks are needed per group.
///
/// # Example
/// ```ignore
/// let fg = FilterGroups::from(filters);
/// if let Some(sort_val) = fg.selection_value("sort") {
///     let (key, dir) = sort_val.split_once(':').unwrap_or((sort_val, "desc"));
///     req = req.query("sort", key).query("order", dir);
/// }
/// ```
pub struct FilterGroups<'a> {
    groups: HashMap<&'a str, Vec<&'a ActiveFilter>>,
}

impl<'a> FilterGroups<'a> {
    pub fn from(filters: &'a [ActiveFilter]) -> Self {
        let mut groups: HashMap<&'a str, Vec<&'a ActiveFilter>> = HashMap::new();
        for f in filters {
            let group = f
                .filter_name
                .split_once(':')
                .map_or(f.filter_name.as_str(), |(g, _)| g);
            groups.entry(group).or_default().push(f);
        }
        Self { groups }
    }

    pub fn get(&self, group: &str) -> &[&'a ActiveFilter] {
        self.groups.get(group).map_or(&[], Vec::as_slice)
    }

    pub fn has_any(&self, group: &str) -> bool {
        self.groups.contains_key(group)
    }

    pub fn multiselect_values(&self, group: &str) -> Vec<&'a str> {
        self.get(group)
            .iter()
            .filter_map(|f| {
                if let FilterState::Multiselect(values) = &f.state {
                    Some(values.iter().map(String::as_str))
                } else {
                    None
                }
            })
            .flatten()
            .collect()
    }

    /// Returns the first non-empty [`FilterState::Selection`] value in `group`.
    pub fn selection_value(&self, group: &str) -> Option<&'a str> {
        self.get(group).iter().find_map(|f| {
            if let FilterState::Selection { value, .. } = &f.state {
                if !value.is_empty() {
                    Some(value.as_str())
                } else {
                    None
                }
            } else {
                None
            }
        })
    }

    /// Returns the checked value for a [`FilterState::Checkbox`] in `group`.
    ///
    /// Respects the `group:action` convention: returns `action` when present, otherwise `"true"`.
    /// Returns `None` if no checkbox in this group is checked.
    pub fn checkbox_value(&self, group: &str) -> Option<&'a str> {
        self.get(group).iter().find_map(|f| {
            if matches!(&f.state, FilterState::Checkbox(true)) {
                let action = f.filter_name.split_once(':').map_or("", |(_, a)| a);
                Some(if action.is_empty() { "true" } else { action })
            } else {
                None
            }
        })
    }

    /// Returns the non-empty [`FilterState::TextInput`] value in `group`.
    pub fn text_value(&self, group: &str) -> Option<&'a str> {
        self.get(group).iter().find_map(|f| {
            if let FilterState::TextInput(s) = &f.state {
                if !s.is_empty() {
                    Some(s.as_str())
                } else {
                    None
                }
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(name: &str, state: FilterState) -> ActiveFilter {
        ActiveFilter {
            filter_name: name.to_string(),
            state,
        }
    }

    fn queries(req: HttpRequest) -> Vec<(String, String)> {
        req.into_queries()
    }

    #[test]
    fn multiselect_repeated() {
        let filters = vec![active(
            "genre",
            FilterState::Multiselect(vec!["action".into(), "romance".into()]),
        )];
        let q = queries(HttpRequest::get("http://x.com").apply_filters(&filters));
        assert_eq!(
            q,
            vec![
                ("genre".into(), "action".into()),
                ("genre".into(), "romance".into())
            ]
        );
    }

    #[test]
    fn multiselect_bracket() {
        let filters = vec![active(
            "tag",
            FilterState::Multiselect(vec!["a".into(), "b".into()]),
        )];
        let q = queries(
            HttpRequest::get("http://x.com").apply_filters_fmt(&filters, ArrayFormat::Bracket),
        );
        assert_eq!(
            q,
            vec![("tag[]".into(), "a".into()), ("tag[]".into(), "b".into())]
        );
    }

    #[test]
    fn multiselect_comma_separated() {
        let filters = vec![active(
            "tag",
            FilterState::Multiselect(vec!["a".into(), "b".into(), "c".into()]),
        )];
        let q = queries(
            HttpRequest::get("http://x.com")
                .apply_filters_fmt(&filters, ArrayFormat::CommaSeparated),
        );
        assert_eq!(q, vec![("tag".into(), "a,b,c".into())]);
    }

    #[test]
    fn checkbox_true_default_value() {
        let filters = vec![active("adult", FilterState::Checkbox(true))];
        let q = queries(HttpRequest::get("http://x.com").apply_filters(&filters));
        assert_eq!(q, vec![("adult".into(), "true".into())]);
    }

    #[test]
    fn checkbox_true_action_value() {
        let filters = vec![active("adult:yes", FilterState::Checkbox(true))];
        let q = queries(HttpRequest::get("http://x.com").apply_filters(&filters));
        assert_eq!(q, vec![("adult".into(), "yes".into())]);
    }

    #[test]
    fn text_input_nonempty() {
        let filters = vec![active("q", FilterState::TextInput("hello".into()))];
        let q = queries(HttpRequest::get("http://x.com").apply_filters(&filters));
        assert_eq!(q, vec![("q".into(), "hello".into())]);
    }

    #[test]
    fn selection_nonempty() {
        let filters = vec![active(
            "type",
            FilterState::Selection {
                name: "Type".into(),
                value: "manga".into(),
            },
        )];
        let q = queries(HttpRequest::get("http://x.com").apply_filters(&filters));
        assert_eq!(q, vec![("type".into(), "manga".into())]);
    }

    #[test]
    fn mapped_translates_name() {
        let filters = vec![active(
            "genre",
            FilterState::Multiselect(vec!["action".into()]),
        )];
        let q = queries(
            HttpRequest::get("http://x.com").apply_filters_mapped(&filters, &[("genre", "type")]),
        );
        assert_eq!(q, vec![("type".into(), "action".into())]);
    }

    #[test]
    fn mapped_fallthrough_unmapped_group() {
        let filters = vec![active(
            "sort",
            FilterState::Selection {
                name: "Sort".into(),
                value: "latest".into(),
            },
        )];
        let q = queries(
            HttpRequest::get("http://x.com").apply_filters_mapped(&filters, &[("genre", "type")]),
        );
        assert_eq!(q, vec![("sort".into(), "latest".into())]);
    }

    #[test]
    fn groups_has_any() {
        let filters = vec![active(
            "genre",
            FilterState::Multiselect(vec!["action".into()]),
        )];
        let fg = FilterGroups::from(&filters);
        assert!(fg.has_any("genre"));
        assert!(!fg.has_any("type"));
    }

    #[test]
    fn groups_multiselect_values() {
        let filters = vec![active(
            "genre",
            FilterState::Multiselect(vec!["a".into(), "b".into()]),
        )];
        let fg = FilterGroups::from(&filters);
        assert_eq!(fg.multiselect_values("genre"), vec!["a", "b"]);
        assert!(fg.multiselect_values("other").is_empty());
    }

    #[test]
    fn groups_selection_value() {
        let filters = vec![active(
            "sort",
            FilterState::Selection {
                name: "Sort".into(),
                value: "latest".into(),
            },
        )];
        let fg = FilterGroups::from(&filters);
        assert_eq!(fg.selection_value("sort"), Some("latest"));
        assert_eq!(fg.selection_value("other"), None);
    }

    #[test]
    fn groups_checkbox_value_default() {
        let filters = vec![active("adult", FilterState::Checkbox(true))];
        let fg = FilterGroups::from(&filters);
        assert_eq!(fg.checkbox_value("adult"), Some("true"));
    }

    #[test]
    fn groups_checkbox_value_action() {
        let filters = vec![active("adult:yes", FilterState::Checkbox(true))];
        let fg = FilterGroups::from(&filters);
        assert_eq!(fg.checkbox_value("adult"), Some("yes"));
    }

    #[test]
    fn groups_text_value() {
        let filters = vec![active("q", FilterState::TextInput("hello".into()))];
        let fg = FilterGroups::from(&filters);
        assert_eq!(fg.text_value("q"), Some("hello"));
        assert_eq!(fg.text_value("other"), None);
    }

    #[test]
    fn groups_empty_text_not_returned() {
        let filters = vec![active("q", FilterState::TextInput(String::new()))];
        let fg = FilterGroups::from(&filters);
        assert_eq!(fg.text_value("q"), None);
    }

    #[test]
    fn a_filter_in_its_empty_state_contributes_no_query_parameter() {
        let empty = [
            ("multiselect", FilterState::Multiselect(vec![])),
            ("checkbox", FilterState::Checkbox(false)),
            ("text input", FilterState::TextInput(String::new())),
            (
                "selection",
                FilterState::Selection {
                    name: "Type".into(),
                    value: String::new(),
                },
            ),
        ];
        for (label, state) in empty {
            let filters = vec![active("f", state)];
            let q = queries(HttpRequest::get("http://x.com").apply_filters(&filters));
            assert!(q.is_empty(), "an empty {label} must not be sent, got {q:?}");
        }
    }
}
