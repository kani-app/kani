//! Guest implementation contract, typed extension failures, and extension metadata.

use crate::{
    types::ActiveFilter,
    wit_types::{
        Chapter, ChapterList, FilterList, MangaInfo, MangaList, PreferenceSpec, SortOption,
    },
};

pub type ExtensionResult<T> = Result<T, ExtensionError>;

/// Marker the host evaluator puts on an error whose HTTP status the caller
/// should turn into a typed [`ExtensionError`] — `__http_status__:<code>:<retry>`
/// (the retry part may be empty). The string is the wire contract between the
/// evaluator that produces it (kani-core) and the two backends that consume it:
/// the interpreted-YAML source host-side and the WASM guest's extraction
/// wrappers. Keeping it here — the lowest shared crate — lets both classify the
/// same way across interpreted and compiled sources.
pub const HTTP_STATUS_ERR_PREFIX: &str = "__http_status__:";

/// Decodes an [`HTTP_STATUS_ERR_PREFIX`] marker into a typed [`ExtensionError`],
/// or returns `None` if `msg` is an ordinary (non-HTTP) error. 404 never reaches
/// here — the evaluator excludes it so "no more pages" stays an empty result.
pub fn classify_status_error(msg: &str) -> Option<ExtensionError> {
    let rest = msg.strip_prefix(HTTP_STATUS_ERR_PREFIX)?;
    let mut parts = rest.splitn(2, ':');
    let code: u16 = parts.next().and_then(|c| c.parse().ok()).unwrap_or(0);
    let retry_after: Option<u32> = parts.next().and_then(|s| s.parse().ok());
    Some(match code {
        429 => match retry_after {
            Some(secs) => ExtensionError::rate_limited_with_retry(secs),
            None => ExtensionError::rate_limited(),
        },
        401 | 403 => ExtensionError::auth(format!("HTTP {code}")),
        c if (500..600).contains(&c) => ExtensionError::network(format!("HTTP {c}")),
        c => ExtensionError::parse(format!("HTTP {c}")),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Stable error categories carried across the component boundary.
pub enum ExtensionErrorKind {
    Network,
    Parse,
    NotFound,
    RateLimited,
    Auth,
    ContentUnavailable,
    Timeout,
    InvalidInput,
    Internal,
    Unknown,
    Updating,
}

impl std::fmt::Display for ExtensionErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network => write!(f, "network"),
            Self::Parse => write!(f, "parse"),
            Self::NotFound => write!(f, "not-found"),
            Self::RateLimited => write!(f, "rate-limited"),
            Self::Auth => write!(f, "auth"),
            Self::ContentUnavailable => write!(f, "content-unavailable"),
            Self::Timeout => write!(f, "timeout"),
            Self::InvalidInput => write!(f, "invalid-input"),
            Self::Internal => write!(f, "internal"),
            Self::Unknown => write!(f, "unknown"),
            Self::Updating => write!(f, "source-updating"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Structured failure returned by an extension to the host.
///
/// `source_url` identifies the failed upstream request when known, and
/// `retry_after_secs` is meaningful for retryable conditions such as rate limiting.
pub struct ExtensionError {
    pub kind: ExtensionErrorKind,
    pub message: String,
    pub source_url: Option<String>,
    pub retry_after_secs: Option<u32>,
}

impl ExtensionError {
    pub fn network(message: String) -> Self {
        Self {
            kind: ExtensionErrorKind::Network,
            message,
            source_url: None,
            retry_after_secs: None,
        }
    }

    pub fn parse(message: String) -> Self {
        Self {
            kind: ExtensionErrorKind::Parse,
            message,
            source_url: None,
            retry_after_secs: None,
        }
    }

    pub fn not_found(message: String) -> Self {
        Self {
            kind: ExtensionErrorKind::NotFound,
            message,
            source_url: None,
            retry_after_secs: None,
        }
    }

    pub fn rate_limited() -> Self {
        Self {
            kind: ExtensionErrorKind::RateLimited,
            message: "Rate limited".to_string(),
            source_url: None,
            retry_after_secs: None,
        }
    }

    pub fn rate_limited_with_retry(secs: u32) -> Self {
        Self {
            kind: ExtensionErrorKind::RateLimited,
            message: "Rate limited".to_string(),
            source_url: None,
            retry_after_secs: Some(secs),
        }
    }

    pub fn auth(message: String) -> Self {
        Self {
            kind: ExtensionErrorKind::Auth,
            message,
            source_url: None,
            retry_after_secs: None,
        }
    }

    pub fn content_unavailable(message: String) -> Self {
        Self {
            kind: ExtensionErrorKind::ContentUnavailable,
            message,
            source_url: None,
            retry_after_secs: None,
        }
    }

    pub fn timeout(message: String) -> Self {
        Self {
            kind: ExtensionErrorKind::Timeout,
            message,
            source_url: None,
            retry_after_secs: None,
        }
    }

    pub fn invalid_input(message: String) -> Self {
        Self {
            kind: ExtensionErrorKind::InvalidInput,
            message,
            source_url: None,
            retry_after_secs: None,
        }
    }

    pub fn internal(message: String) -> Self {
        Self {
            kind: ExtensionErrorKind::Internal,
            message,
            source_url: None,
            retry_after_secs: None,
        }
    }

    pub fn unknown(message: String) -> Self {
        Self {
            kind: ExtensionErrorKind::Unknown,
            message,
            source_url: None,
            retry_after_secs: None,
        }
    }

    pub fn source_updating() -> Self {
        Self {
            kind: ExtensionErrorKind::Updating,
            message: "Source is being updated".to_string(),
            source_url: None,
            retry_after_secs: Some(2),
        }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.source_url = Some(url.into());
        self
    }

    pub fn into_wit(self) -> crate::bindings::kani::extension::types::ExtensionError {
        use crate::bindings::kani::extension::types::{
            ExtensionError as WitErr, ExtensionErrorKind as WitKind,
        };
        let wit_kind = match self.kind {
            ExtensionErrorKind::Network => WitKind::Network,
            ExtensionErrorKind::Parse => WitKind::Parse,
            ExtensionErrorKind::NotFound => WitKind::NotFound,
            ExtensionErrorKind::RateLimited => WitKind::RateLimited,
            ExtensionErrorKind::Auth => WitKind::Auth,
            ExtensionErrorKind::ContentUnavailable => WitKind::ContentUnavailable,
            ExtensionErrorKind::Timeout => WitKind::Timeout,
            ExtensionErrorKind::InvalidInput => WitKind::InvalidInput,
            ExtensionErrorKind::Internal => WitKind::Internal,
            ExtensionErrorKind::Unknown => WitKind::Unknown,
            ExtensionErrorKind::Updating => WitKind::SourceUpdating,
        };
        WitErr {
            kind: wit_kind,
            message: self.message,
            source_url: self.source_url,
            retry_after_secs: self.retry_after_secs,
        }
    }
}

impl std::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message)?;
        if let Some(url) = &self.source_url {
            write!(f, " (url: {})", url)?;
        }
        Ok(())
    }
}

impl std::error::Error for ExtensionError {}

impl From<String> for ExtensionError {
    fn from(s: String) -> Self {
        Self::unknown(s)
    }
}

/// Synchronous source-provider contract implemented by extension logic.
///
/// The WIT guest export delegates to one process-wide implementation. Methods may call
/// host-imported capabilities synchronously; Wasmtime suspends the guest while their async host
/// work completes. Page numbers are source-listing pages, not archive page indices.
pub trait MangaExtension {
    fn name(&self) -> &str;

    fn get_popular_manga(
        &self,
        page: i32,
        page_size: i32,
        filters: &[ActiveFilter],
    ) -> ExtensionResult<MangaList>;

    fn search_manga(
        &self,
        query: &str,
        page: i32,
        page_size: i32,
        filters: &[ActiveFilter],
    ) -> ExtensionResult<MangaList>;

    fn get_manga_details(&self, manga_id: &str) -> ExtensionResult<MangaInfo>;

    fn get_chapter_list(
        &self,
        manga_id: &str,
        page: i32,
        page_size: Option<i32>,
        sort: Option<String>,
    ) -> ExtensionResult<ChapterList>;

    fn get_pages(&self, manga_id: &str, chapter_id: &str) -> ExtensionResult<Chapter>;

    fn get_chapter_sort_list(&self) -> ExtensionResult<Vec<SortOption>>;

    fn default_chapter_sort(&self) -> Option<String> {
        None
    }

    fn get_filter_list(&self) -> ExtensionResult<FilterList>;

    fn get_fetched_option_sets(&self) -> ExtensionResult<String> {
        Ok("[]".to_string())
    }

    fn get_preferences(&self) -> ExtensionResult<Vec<PreferenceSpec>>;

    fn get_url(&self, _manga_id: &str) -> ExtensionResult<String> {
        Err(ExtensionError::not_found(
            "get_url not implemented for this source".into(),
        ))
    }
}

#[cfg(target_family = "wasm")]
pub fn bridge_chapter_list_stream<E: MangaExtension + Sync + 'static>(
    ext: &'static E,
    manga_id: String,
    sort: Option<String>,
) -> wit_bindgen::StreamReader<
    Result<crate::wit_types::ChapterInfo, crate::wit_types::ExtensionError>,
> {
    let (mut tx, rx) = crate::bindings::wit_stream::new();
    wit_bindgen::spawn_local(async move {
        let mut page = 1;
        loop {
            match ext.get_chapter_list(&manga_id, page, None, sort.clone()) {
                Ok(list) => {
                    if list.chapters.is_empty() {
                        break;
                    }
                    let has_next_page = list.has_next_page;
                    let items: Vec<_> = list.chapters.into_iter().map(Ok).collect();
                    // `write_all`, not `write`: a write transfers only what the reader
                    // takes. Non-empty here means the reader is gone.
                    let unwritten = tx.write_all(items).await;
                    if !unwritten.is_empty() || !has_next_page {
                        break;
                    }
                    page += 1;
                }
                Err(e) => {
                    let _ = tx.write_all(vec![Err(e.into_wit())]).await;
                    break;
                }
            }
        }
    });
    rx
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder", feature = "meta"),
    derive(serde::Serialize, serde::Deserialize)
)]
/// Host-enforced request budget declared by an extension.
pub struct RateLimitConfig {
    /// Sustained requests permitted per second.
    pub requests_per_second: f32,
    /// Requests permitted to exceed the sustained rate in one burst.
    pub burst: u32,
    /// Maximum upstream requests in flight for this extension.
    pub max_concurrent: u32,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default = "default_max_hook_requests")
    )]
    /// Maximum extra requests an `on_status` hook may issue for one original request.
    pub max_hook_requests: u32,
}

#[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
fn default_max_hook_requests() -> u32 {
    3
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 2.0,
            burst: 8,
            max_concurrent: 4,
            max_hook_requests: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder", feature = "meta"),
    derive(serde::Serialize, serde::Deserialize)
)]
/// A named content section exposed by a source, optionally restricted as adult content.
pub struct Section {
    pub id: String,
    pub name: String,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub nsfw: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    any(feature = "host", feature = "builder", feature = "meta"),
    derive(serde::Serialize, serde::Deserialize)
)]
/// Installation and capability metadata published by an extension.
///
/// Schema and minimum-version fields let the host reject incompatible guests before invoking
/// provider operations. Missing optional collections deserialize as empty for older extensions.
pub struct ExtensionMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub base_url: String,
    pub language: String,
    pub nsfw: bool,
    pub unrestricted_http: bool,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub mihon_source_id: Option<i64>,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub rate_limit: Option<RateLimitConfig>,
    /// Base64-encoded icon image (PNG/WebP/SVG); kept as a string to avoid a
    /// byte-array WIT/JSON encoding blowup for what is already a small image.
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub icon: Option<String>,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub languages: Vec<String>,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub description: Option<String>,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default = "default_schema_version")
    )]
    pub schema_version: u32,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub min_kani_version: Option<String>,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub requires_capabilities: Vec<String>,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub sections: Vec<Section>,
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub scripts: std::collections::BTreeMap<String, String>,
    /// Source-level pre_request Rhai hook body. `None` means no hook.
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub pre_request: Option<String>,
    /// Source-level on_status Rhai hook bodies keyed by status pattern.
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub on_status: std::collections::BTreeMap<String, String>,
    /// Per-endpoint pre_request hooks: endpoint_id → hook body.
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub endpoint_pre_request: std::collections::BTreeMap<String, String>,
    /// Per-endpoint on_status hooks: endpoint_id → (status_pattern → hook body).
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub endpoint_on_status:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// Cache namespaces hook scripts may use, from the YAML `cache:` block.
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub cache: std::collections::BTreeMap<String, CacheNamespaceLimits>,
    /// Blueprint schema version the extension was built with. `None` for extensions built
    /// before it was recorded, which the host cannot check until their first extraction.
    #[cfg_attr(
        any(feature = "host", feature = "builder", feature = "meta"),
        serde(default)
    )]
    pub dsl_schema_version: Option<u32>,
}

/// Most cache namespaces one extension may declare, so its total cache storage is bounded.
pub const MAX_CACHE_NAMESPACES: usize = 16;
/// Host limit on entries in any one cache namespace; `max_entries` may only lower it.
pub const MAX_CACHE_NAMESPACE_ENTRIES: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    any(feature = "host", feature = "builder", feature = "meta"),
    derive(serde::Serialize, serde::Deserialize)
)]
/// Limits for one declared cache namespace: `ttl_seconds` is both the default and the maximum
/// entry lifetime, and `max_entries` caps the namespace below the host's own limit.
pub struct CacheNamespaceLimits {
    pub ttl_seconds: u32,
    pub max_entries: Option<u32>,
}

#[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
fn default_schema_version() -> u32 {
    1
}

impl Default for ExtensionMetadata {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            version: String::new(),
            base_url: String::new(),
            language: String::new(),
            nsfw: false,
            unrestricted_http: false,
            mihon_source_id: None,
            rate_limit: None,
            icon: None,
            languages: Vec::new(),
            description: None,
            schema_version: 1,
            min_kani_version: None,
            requires_capabilities: Vec::new(),
            sections: Vec::new(),
            scripts: std::collections::BTreeMap::new(),
            pre_request: None,
            on_status: std::collections::BTreeMap::new(),
            endpoint_pre_request: std::collections::BTreeMap::new(),
            endpoint_on_status: std::collections::BTreeMap::new(),
            cache: std::collections::BTreeMap::new(),
            dsl_schema_version: Some(crate::ast::DSL_SCHEMA_VERSION),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn kind_display() {
        assert_eq!(ExtensionErrorKind::Network.to_string(), "network");
        assert_eq!(ExtensionErrorKind::RateLimited.to_string(), "rate-limited");
    }

    #[test]
    fn classify_status_error_decodes_429_with_retry_after() {
        let e = classify_status_error("__http_status__:429:120").unwrap();
        assert_eq!(e.kind, ExtensionErrorKind::RateLimited);
        assert_eq!(e.retry_after_secs, Some(120));
    }

    #[test]
    fn classify_status_error_429_without_retry_after() {
        let e = classify_status_error("__http_status__:429:").unwrap();
        assert_eq!(e.kind, ExtensionErrorKind::RateLimited);
        assert_eq!(e.retry_after_secs, None);
    }

    #[test]
    fn classify_status_error_maps_auth_and_server_ranges() {
        assert_eq!(
            classify_status_error("__http_status__:401:").unwrap().kind,
            ExtensionErrorKind::Auth
        );
        assert_eq!(
            classify_status_error("__http_status__:403:").unwrap().kind,
            ExtensionErrorKind::Auth
        );
        assert_eq!(
            classify_status_error("__http_status__:503:").unwrap().kind,
            ExtensionErrorKind::Network
        );
    }

    #[test]
    fn classify_status_error_ignores_non_sentinel_errors() {
        assert!(classify_status_error("selector matched nothing").is_none());
        assert!(classify_status_error("HTTP 404").is_none());
    }

    #[test]
    fn into_wit_maps_updating_to_source_updating() {
        use crate::bindings::kani::extension::types::ExtensionErrorKind as WitKind;

        let wit_err = ExtensionError::source_updating().into_wit();
        assert_eq!(wit_err.kind, WitKind::SourceUpdating);
    }

    #[test]
    fn constructors_set_kind() {
        assert_eq!(
            ExtensionError::network("x".into()).kind,
            ExtensionErrorKind::Network
        );
        assert_eq!(
            ExtensionError::parse("x".into()).kind,
            ExtensionErrorKind::Parse
        );
        assert_eq!(
            ExtensionError::not_found("x".into()).kind,
            ExtensionErrorKind::NotFound
        );
        assert_eq!(
            ExtensionError::rate_limited().kind,
            ExtensionErrorKind::RateLimited
        );
        assert_eq!(
            ExtensionError::auth("x".into()).kind,
            ExtensionErrorKind::Auth
        );
        assert_eq!(
            ExtensionError::timeout("x".into()).kind,
            ExtensionErrorKind::Timeout
        );
        assert_eq!(
            ExtensionError::unknown("x".into()).kind,
            ExtensionErrorKind::Unknown
        );
    }

    #[test]
    fn rate_limited_with_retry_sets_secs() {
        let e = ExtensionError::rate_limited_with_retry(30);
        assert_eq!(e.kind, ExtensionErrorKind::RateLimited);
        assert_eq!(e.retry_after_secs, Some(30));
    }

    #[test]
    fn with_url_stores_url() {
        let e = ExtensionError::network("err".into()).with_url("https://example.com/page");
        assert_eq!(e.source_url.as_deref(), Some("https://example.com/page"));
    }

    #[test]
    fn display_includes_kind_and_message() {
        let e = ExtensionError::network("connection refused".into());
        let s = e.to_string();
        assert!(s.contains("network"));
        assert!(s.contains("connection refused"));
    }

    #[test]
    #[allow(deprecated)]
    fn from_string_shim_produces_unknown() {
        let e = ExtensionError::from("oops".to_string());
        assert_eq!(e.kind, ExtensionErrorKind::Unknown);
        assert_eq!(e.message, "oops");
    }

    #[test]
    fn source_updating_kind_is_updating() {
        let e = ExtensionError::source_updating();
        assert_eq!(e.kind, ExtensionErrorKind::Updating);
    }

    #[test]
    fn source_updating_retry_after_is_2() {
        let e = ExtensionError::source_updating();
        assert_eq!(e.retry_after_secs, Some(2));
    }

    #[test]
    fn source_updating_has_no_source_url() {
        let e = ExtensionError::source_updating();
        assert!(e.source_url.is_none());
    }

    #[test]
    fn updating_kind_display_is_source_updating() {
        assert_eq!(ExtensionErrorKind::Updating.to_string(), "source-updating");
    }

    #[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
    #[test]
    fn metadata_serde_round_trip() {
        let meta = ExtensionMetadata {
            id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            base_url: "https://example.com".to_string(),
            language: "en".to_string(),
            nsfw: false,
            unrestricted_http: false,
            mihon_source_id: Some(42),
            rate_limit: Some(RateLimitConfig::default()),
            icon: Some("aWNvbg==".to_string()),
            languages: vec!["en".to_string(), "ja".to_string()],
            description: Some("A test extension".to_string()),
            schema_version: 1,
            min_kani_version: Some("0.5.0".to_string()),
            requires_capabilities: vec!["unrestricted_http".to_string()],
            sections: vec![Section {
                id: "latest".to_string(),
                name: "Latest".to_string(),
                nsfw: false,
            }],
            scripts: std::collections::BTreeMap::new(),
            pre_request: None,
            on_status: std::collections::BTreeMap::new(),
            endpoint_pre_request: std::collections::BTreeMap::new(),
            endpoint_on_status: std::collections::BTreeMap::new(),
            cache: std::collections::BTreeMap::new(),
            dsl_schema_version: Some(crate::ast::DSL_SCHEMA_VERSION),
        };

        let json = crate::serde_json::to_string(&meta).expect("serializes");
        let round_tripped: ExtensionMetadata =
            crate::serde_json::from_str(&json).expect("deserializes");
        assert_eq!(meta, round_tripped);
    }

    #[cfg(any(feature = "host", feature = "builder", feature = "meta"))]
    #[test]
    fn metadata_deserializes_from_json_missing_new_fields() {
        let legacy_json = r#"{
            "id": "legacy",
            "name": "Legacy",
            "version": "1.0.0",
            "base_url": "https://example.com",
            "language": "en",
            "nsfw": false,
            "unrestricted_http": false
        }"#;

        let meta: ExtensionMetadata =
            crate::serde_json::from_str(legacy_json).expect("deserializes despite missing fields");
        assert_eq!(meta.id, "legacy");
        assert_eq!(meta.icon, None);
        assert!(meta.languages.is_empty());
        assert_eq!(meta.description, None);
        assert_eq!(meta.schema_version, 1);
        assert_eq!(meta.min_kani_version, None);
        assert!(meta.requires_capabilities.is_empty());
        assert!(meta.sections.is_empty());
    }
}
