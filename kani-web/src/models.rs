//! Server-specific models (database entities and API request/response types).

use kani_app::ids::{ChapterId, MangaId};
use serde::Deserialize;

fn validate_https_url(value: &str, _: &()) -> garde::Result {
    if value.starts_with("https://") {
        Ok(())
    } else {
        Err(garde::Error::new("URL must use HTTPS scheme"))
    }
}

#[derive(garde::Validate, Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct UpdateSource {
    #[garde(inner(length(min = 1, max = 100)))]
    pub name: Option<String>,
    #[garde(inner(length(min = 1, max = 50)))]
    pub version: Option<String>,
}

#[derive(garde::Validate, Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetLocalHostsRequest {
    /// Private hosts the source may reach, each `host` or `host:port`. Empty revokes the grant.
    #[garde(length(max = 32), inner(length(min = 1, max = 255)))]
    pub hosts: Vec<String>,
}

#[derive(garde::Validate, Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct FetchWasmRequest {
    #[garde(length(min = 1, max = 2048), custom(validate_https_url))]
    pub url: String,
}

#[derive(garde::Validate, Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct InstallYamlRequest {
    #[garde(length(min = 1, max = 262_144))]
    pub content: String,
}

#[derive(garde::Validate, Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct FetchYamlRequest {
    #[garde(length(min = 1, max = 2048), custom(validate_https_url))]
    pub url: String,
}

// Types moved to kani-app. Re-exported here so that existing kani-web code
// (rest.rs, tests) continues to compile unchanged.
pub use kani_app::models::{DownloadRuleRow, LibraryManga, Manga, Settings};

#[derive(serde::Deserialize, Default, Debug, utoipa::ToSchema)]
pub(crate) struct RefreshMangaRequest {
    pub fields: Option<Vec<String>>,
    pub fetch_chapters: Option<bool>,
    pub clear_overrides: Option<bool>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct UpdateLocalMetadataRequest {
    pub local_name: Option<String>,
    pub local_description: Option<String>,
    pub local_status: Option<i64>,
    pub authors: Option<Vec<String>>,
    pub artists: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(garde::Validate, Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SearchMangaRequest {
    #[garde(inner(length(max = 200)))]
    pub query: Option<String>,
    #[garde(skip)]
    pub filters: Option<String>,
}

#[derive(garde::Validate, Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct PopularMangaQuery {
    #[garde(skip)]
    pub filters: Option<String>,
}

#[derive(garde::Validate, Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ProxyQuery {
    #[garde(length(min = 1, max = 4096))]
    pub token: String,
    #[garde(skip)]
    pub transform: Option<String>,
    #[garde(skip)]
    pub w: Option<u32>,
    #[garde(skip)]
    pub format: Option<String>,
    #[garde(skip)]
    pub q: Option<u8>,
}

#[derive(sqlx::FromRow)]
pub struct FilterOptionResult {
    pub id: i64,
    pub name: String,
}

#[derive(garde::Validate, serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct LibraryQuery {
    #[garde(range(min = 1))]
    #[serde(default = "default_page")]
    pub page: i32,
    #[garde(range(min = 1, max = 200))]
    #[serde(default = "default_library_page_size")]
    pub page_size: i32,
    #[garde(skip)]
    pub search: Option<String>,
    #[garde(skip)]
    pub status_filter: Option<i64>,
    #[garde(skip)]
    pub tag_filter: Option<i64>,
    #[garde(skip)]
    pub author_filter: Option<i64>,
    #[garde(skip)]
    pub artist_filter: Option<i64>,
    #[garde(skip)]
    pub category_filter: Option<i64>,
    #[garde(skip)]
    pub reading_status_filter: Option<i64>,
    #[garde(skip)]
    #[serde(default)]
    pub hide_no_unread: bool,
    #[garde(skip)]
    #[serde(default)]
    pub hide_completed_status: bool,
    #[garde(skip)]
    pub source_id: Option<i64>,
    #[garde(skip)]
    pub collection_id: Option<i64>,
    #[garde(skip)]
    #[serde(default)]
    #[schema(value_type = String)]
    pub sort_by: kani_shared::MangaSortOrder,
}

#[derive(garde::Validate, serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct LocalChaptersQuery {
    #[garde(range(min = 1))]
    #[serde(default = "default_page")]
    pub page: i32,
    #[garde(range(min = 1, max = 200))]
    #[serde(default = "default_chapter_page_size")]
    pub page_size: i32,
    #[garde(skip)]
    #[serde(default)]
    #[schema(value_type = String)]
    pub sort_order: kani_shared::ChapterSortOrder,
    /// `true` = downloaded only, `false` = undownloaded only, absent = all
    #[garde(skip)]
    pub filter_downloaded: Option<bool>,
    /// `true` = unread only
    #[garde(skip)]
    pub filter_unread: Option<bool>,
    /// Limit to a specific scanlator when set
    #[garde(skip)]
    pub filter_scanlator: Option<String>,
    /// `true` = only chapters a migration orphaned; absent = hide them
    #[garde(skip)]
    pub filter_orphaned: Option<bool>,
}

/// Query parameters for the chapter-IDs endpoint (no pagination).
#[derive(garde::Validate, serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ChapterIdsQuery {
    #[garde(skip)]
    #[serde(default)]
    #[schema(value_type = String)]
    pub sort_order: kani_shared::ChapterSortOrder,
    /// `true` = downloaded only, `false` = undownloaded only, absent = all
    #[garde(skip)]
    pub filter_downloaded: Option<bool>,
    /// `true` = unread only
    #[garde(skip)]
    pub filter_unread: Option<bool>,
    /// Limit to a specific scanlator when set
    #[garde(skip)]
    pub filter_scanlator: Option<String>,
    /// When true, applies scanlator preferences + download rules to return
    /// only the preferred one version per chapter number (undownloaded only).
    #[garde(skip)]
    #[serde(default)]
    pub preferred_only: bool,
    /// `true` = only chapters a migration orphaned; absent = hide them
    #[garde(skip)]
    pub filter_orphaned: Option<bool>,
}

fn default_chapter_page_size() -> i32 {
    50
}

/// Paginated endpoints document `page` and `page_size` as optional in their
/// OpenAPI parameters, so omitting them must work — a client generated from the
/// spec sends neither. These supply the documented defaults.
fn default_page() -> i32 {
    1
}

fn default_library_page_size() -> i32 {
    20
}

fn default_search_scope() -> kani_shared::SearchScope {
    kani_shared::SearchScope::AllEnabled
}

#[derive(garde::Validate, serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct PageQuery {
    #[garde(range(min = 1))]
    #[serde(default = "default_page")]
    pub page: i32,
}

fn deserialize_search_scope<'de, D>(deserializer: D) -> Result<kani_shared::SearchScope, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use kani_shared::SearchScope;
    use serde::Deserialize;
    let s = String::deserialize(deserializer)?;
    // Accept both structured JSON and the legacy bare unit-variant representation.
    if let Ok(scope) = serde_json::from_str::<SearchScope>(&s) {
        return Ok(scope);
    }
    serde_json::from_str::<SearchScope>(&format!("\"{}\"", s)).map_err(serde::de::Error::custom)
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct GlobalSearchQuery {
    pub query: String,
    #[serde(
        deserialize_with = "deserialize_search_scope",
        default = "default_search_scope"
    )]
    #[schema(value_type = Object)]
    pub scope: kani_shared::SearchScope,
    #[serde(default = "default_page")]
    pub page: i32,
    #[serde(default = "default_library_page_size")]
    pub page_size: i32,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct AddDownloadRuleRequest {
    #[schema(value_type = Object)]
    pub kind: kani_shared::DownloadRuleKind,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct UpdateDownloadRuleRequest {
    #[schema(value_type = Object)]
    pub kind: kani_shared::DownloadRuleKind,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ReorderDownloadRulesRequest {
    pub ordered_ids: Vec<i64>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct PreviewDownloadRulesRequest {
    #[schema(value_type = Vec<Object>)]
    pub kinds: Vec<kani_shared::DownloadRuleKind>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetScanlatorPrefRequest {
    pub scanlator: String,
    pub priority: i64,
    #[serde(default)]
    pub blocked: bool,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetScanlatorModeRequest {
    pub mode: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct CreateCategoryRequest {
    pub name: String,
    pub sort_order: i64,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct RenameCategoryRequest {
    pub name: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ReorderCategoriesRequest {
    pub ordered_ids: Vec<i64>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetMangaCategoriesRequest {
    pub category_ids: Vec<i64>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetPreferenceRequest {
    pub value: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ListItemRequest {
    pub item: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ToggleSelectRequest {
    pub item: String,
    pub selected: bool,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ToggleEnabledRequest {
    pub enabled: bool,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ToggleFavouritedRequest {
    pub favourited: bool,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ToggleAutoDownloadRequest {
    pub enabled: bool,
}

/// Body for `POST /manga/scan`. Either scan all manga or a specific list.
#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
#[serde(untagged)]
pub enum ScanMangaRequest {
    /// Scan specific manga by ID.
    Ids {
        #[schema(value_type = Vec<i64>)]
        ids: Vec<MangaId>,
    },
    /// Scan all manga in the library. Send `{ "ids": "all" }`.
    All { ids: ScanAll },
}

/// Sentinel value — used in `ScanMangaRequest::All`.
#[derive(Debug, utoipa::ToSchema)]
pub struct ScanAll;

impl<'de> serde::Deserialize<'de> for ScanAll {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s == "all" {
            Ok(ScanAll)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected \"all\", got {s:?}"
            )))
        }
    }
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct PreviewMigrationRequest {
    pub target_source_id: i64,
    pub target_source_manga_id: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct MigrateMangaRequest {
    pub target_source_id: i64,
    pub target_source_manga_id: String,
    pub keep_orphaned_downloads: bool,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct MigrationMatchItem {
    #[schema(value_type = i64)]
    pub manga_id: kani_app::ids::MangaId,
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct MatchMigrationTargetsRequest {
    pub target_source_id: i64,
    pub items: Vec<MigrationMatchItem>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct BulkMigrationRequestItem {
    #[schema(value_type = i64)]
    pub manga_id: kani_app::ids::MangaId,
    pub target_source_manga_id: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct BulkMigrationRequest {
    pub target_source_id: i64,
    pub items: Vec<BulkMigrationRequestItem>,
    pub keep_orphaned_downloads: bool,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct MigrationStatusRequest {
    #[schema(value_type = Vec<String>)]
    pub job_ids: Vec<uuid::Uuid>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetChapterProgressRequest {
    pub page: i64,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetReadStatusRequest {
    #[schema(value_type = Vec<i64>)]
    pub chapter_ids: Vec<ChapterId>,
    pub is_read: bool,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetMangaTrackingRequest {
    #[schema(value_type = Option<String>)]
    pub status: Option<kani_shared::types::MangaTrackingStatus>,
    pub score: Option<f64>,
    pub tracking_enabled: Option<bool>,
    pub notify_new_chapters: Option<bool>,
    pub reading_direction: Option<String>,
    pub reader_prefs: Option<String>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ToggleBookmarkRequest {
    pub page_index: i64,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetChapterNoteRequest {
    pub note: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct TrackerAuthUrlQuery {
    pub redirect_uri: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct TrackerCallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetTrackerConfigRequest {
    pub client_id: String,
    /// Omit to keep existing secret; set to empty string to clear.
    pub client_secret: Option<String>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct TrackerSearchQuery {
    pub query: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SetTrackerMappingRequest {
    pub tracker_id: i64,
    pub tracker_manga_id: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct MarkUpToRequest {
    pub chapter_number: f64,
    pub is_read: bool,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct ContinueReadingShelfQuery {
    #[serde(default = "default_shelf_limit")]
    pub limit: i64,
}

fn default_shelf_limit() -> i64 {
    12
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct AdminCreateUserRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    /// Roles to assign in addition to the default "user" role.
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct AdminUpdateUserRequest {
    pub username: Option<String>,
    pub email: Option<String>,
    pub is_active: Option<bool>,
    pub password: Option<String>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct AdminGrantRoleRequest {
    pub role_slug: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct AdminCreateRoleRequest {
    pub slug: String,
    pub parent: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct AdminUpdateRoleRequest {
    pub description: Option<String>,
    pub permissions: Option<Vec<String>>,
}

#[derive(garde::Validate, serde::Deserialize, Debug, Default, utoipa::ToSchema)]
pub(crate) struct LogsQuery {
    /// Comma-separated levels, e.g. "error,warn". Empty = all levels.
    #[garde(skip)]
    pub level: Option<String>,
    /// Comma-separated source tags, e.g. "app,http". Empty = all sources.
    #[garde(skip)]
    pub source: Option<String>,
    #[garde(skip)]
    pub from: Option<String>,
    #[garde(skip)]
    pub to: Option<String>,
    #[garde(skip)]
    pub search: Option<String>,
    #[garde(range(min = 1))]
    pub page: Option<i32>,
    #[garde(range(min = 1, max = 500))]
    pub page_size: Option<i32>,
    /// "json" | "text" — only used by the download endpoint.
    #[garde(skip)]
    pub format: Option<String>,
}

#[derive(garde::Validate, serde::Deserialize, Debug, Default, utoipa::ToSchema)]
pub(crate) struct AuditLogQuery {
    #[garde(skip)]
    pub user_id: Option<i64>,
    #[garde(skip)]
    pub action: Option<String>,
    #[garde(skip)]
    pub from: Option<String>,
    #[garde(skip)]
    pub to: Option<String>,
    #[garde(skip)]
    pub search: Option<String>,
    #[garde(range(min = 1))]
    pub page: Option<i32>,
    #[garde(range(min = 1, max = 200))]
    pub page_size: Option<i32>,
    /// "json" | "csv" — only used by the download endpoint.
    #[garde(skip)]
    pub format: Option<String>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct PasswordResetRequestBody {
    pub email: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct PasswordResetConfirmBody {
    pub token: String,
    pub new_password: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct TokenQuery {
    pub token: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SolverTestBody {
    pub url: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct SendTestEmailBody {
    pub to: String,
}

#[derive(garde::Validate, serde::Deserialize, Debug, utoipa::ToSchema)]
pub struct StatsQuery {
    /// Number of days for the daily_activity window. Default 90.
    #[garde(range(min = 1, max = 365))]
    pub period: Option<i32>,
}

#[derive(garde::Validate, serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct FsBrowseQuery {
    #[garde(length(min = 1, max = 4096))]
    pub path: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct FsMkdirBody {
    pub path: String,
    pub name: String,
}

#[derive(serde::Serialize, Debug, utoipa::ToSchema)]
pub(crate) struct FsBrowseResponse {
    pub path: String,
    pub segments: Vec<String>,
    pub dirs: Vec<String>,
    pub drives: Vec<String>,
}

#[derive(serde::Serialize, Debug, utoipa::ToSchema)]
pub(crate) struct FsMkdirResponse {
    pub path: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct PathMigrateBody {
    pub field: String,
    pub new_path: String,
}

#[derive(serde::Serialize, Debug, utoipa::ToSchema)]
pub(crate) struct PathMigrateEstimateResponse {
    pub current_bytes: u64,
    pub available_bytes: u64,
    pub can_migrate: bool,
    pub reason: Option<String>,
}

#[derive(garde::Validate, serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct AddRepoRequest {
    #[garde(length(min = 1, max = 2048), custom(validate_https_url))]
    pub url: String,
    #[garde(skip)]
    pub confirm_fingerprint: Option<String>,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct InstallFromRepoRequest {
    pub repo_id: i64,
    pub extension_id: String,
}

#[derive(serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct UpdateFromRepoRequest {
    pub repo_id: i64,
    pub extension_id: String,
}

#[derive(garde::Validate, serde::Deserialize, Debug, utoipa::ToSchema)]
pub(crate) struct BlockRepoRequest {
    #[garde(length(min = 1, max = 2048), custom(validate_https_url))]
    pub url: String,
    #[garde(length(min = 1, max = 500))]
    pub reason: String,
}
