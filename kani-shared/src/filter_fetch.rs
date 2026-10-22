//! Schema for filter options that the host fetches when rendering an extension's filter panel.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Describes a filter whose options are fetched by the host at filter-panel
/// render time. Returned as JSON from `get_fetched_option_sets()`.
///
/// `fields` maps option property names to extraction expressions:
/// - For HTML (`response_type = "html"`): CSS selector (text) or `"sel|attr"` for attribute.
/// - For JSON (`response_type = "json"`): JSON Pointer path (e.g. `/name`).
///
/// `nsfw_field` names the field in `fields` whose value is a boolean nsfw flag.
/// Options where that field is `"true"` are filtered out by the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterFetchDef {
    pub filter_id: String,
    pub option_set_name: String,
    pub route: String,
    pub response_type: String,
    pub container: Option<String>,
    pub fields: BTreeMap<String, String>,
    pub nsfw_field: Option<String>,
    pub cache_key: Option<String>,
    pub cache_ttl: u32,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn serde_round_trip() {
        let def = FilterFetchDef {
            filter_id: "genres".to_string(),
            option_set_name: "genre_list".to_string(),
            route: "https://example.com/genres".to_string(),
            response_type: "html".to_string(),
            container: Some(".genre".to_string()),
            fields: [
                ("name".to_string(), "span".to_string()),
                ("value".to_string(), "a|href".to_string()),
            ]
            .into_iter()
            .collect(),
            nsfw_field: None,
            cache_key: Some("genres".to_string()),
            cache_ttl: 3600,
        };
        let json = serde_json::to_string(&def).unwrap();
        let back: FilterFetchDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.filter_id, "genres");
        assert_eq!(back.cache_ttl, 3600);
    }
}
