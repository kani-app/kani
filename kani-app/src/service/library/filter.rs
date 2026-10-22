use kani_shared::types::MangaSortOrder;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryFilter {
    #[serde(default = "default_page")]
    pub page: i32,
    #[serde(default = "default_page_size")]
    pub page_size: i32,
    pub search: Option<String>,
    pub status_filter: Option<i64>,
    pub tag_filter: Option<i64>,
    pub author_filter: Option<i64>,
    pub artist_filter: Option<i64>,
    pub category_filter: Option<i64>,
    pub reading_status_filter: Option<i64>,
    #[serde(default)]
    pub hide_no_unread: bool,
    #[serde(default)]
    pub hide_completed_status: bool,
    pub source_id: Option<i64>,
    #[serde(default)]
    pub sort_by: MangaSortOrder,
    #[serde(default)]
    pub include_trashed: bool,
    #[serde(skip)]
    pub manga_id_filter: Option<Vec<i64>>,
}

fn default_page() -> i32 {
    1
}

fn default_page_size() -> i32 {
    20
}

impl Default for LibraryFilter {
    fn default() -> Self {
        Self {
            page: 1,
            page_size: 20,
            search: None,
            status_filter: None,
            tag_filter: None,
            author_filter: None,
            artist_filter: None,
            category_filter: None,
            reading_status_filter: None,
            hide_no_unread: false,
            hide_completed_status: false,
            source_id: None,
            sort_by: MangaSortOrder::default(),
            include_trashed: false,
            manga_id_filter: None,
        }
    }
}
