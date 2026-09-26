//! Database models and application-layer data types.

use crate::ids::{ChapterId, MangaId, SourceId};
use kani_shared::types::{DownloadRule, DownloadRuleKind, NamedItem, Source};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SourceHealthRow {
    pub source_id: i64,
    pub source_name: String,
    pub last_success_at: Option<String>,
    pub last_error_at: Option<String>,
    pub consecutive_error_count: i64,
    pub avg_response_ms: Option<f64>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Settings {
    pub flaresolverr_url: String,
    pub library_path: std::path::PathBuf,
    pub wasm_storage_path: std::path::PathBuf,
    pub concurrent_page_downloads: i64,
    pub max_retries: i64,
    pub initial_retry_delay_ms: i64,
    pub max_wasm_instances: i64,
    pub auto_scan: bool,
    pub scan_interval_minutes: i64,
    pub scan_exclude_completed: bool,
    pub auto_download_category_ids: String,
    pub default_tracking_enabled: bool,
    pub http_request_logging: bool,
    pub v8_debug_logging: bool,
    pub registration_enabled: bool,
    pub cover_max_dimension: Option<i64>,
    pub email_enabled: bool,
    pub email_provider: String,
    pub email_provider_config: String,
    pub email_from_address: String,
    pub app_url: String,
    pub password_reset_enabled: bool,
    pub email_verification_required: bool,
    pub first_run_complete: bool,
    pub scan_concurrency: i64,
    pub per_source_download_concurrency: i64,
    pub job_max_history: i64,
    pub job_shutdown_timeout_secs: i64,
    pub trash_retention_days: i64,
    pub audit_retention_days: i64,
    pub audit_security_retention_days: i64,
    pub disk_warn_threshold: f64,
    pub thumbnail_formats: String,
    pub max_login_attempts: i64,
    pub max_ip_attempts: i64,
    pub login_lockout_seconds: i64,
    pub session_timeout_secs: i64,
    pub tracker_auto_sync_enabled: bool,
    pub tracker_sync_interval_hours: i64,
    pub max_concurrent_jobs: i64,
    pub db_maintenance_interval_hours: i64,
    pub db_vacuum_interval_hours: i64,
    pub audit_prune_interval_hours: i64,
    pub trash_purge_interval_hours: i64,
    pub integrity_quick_scrub_interval_hours: i64,
    pub integrity_deep_scrub_interval_hours: i64,
    pub scrub_on_startup: bool,
    pub integrity_revalidate_after_days: i64,
    pub upgrade_detection_enabled: bool,
    pub upgrade_min_res_gain: f64,
    pub upgrade_confirm_fetches: i64,
    pub upgrade_axis_resolution: String,
    pub upgrade_axis_colour: String,
    pub upgrade_axis_encoder: String,
    pub upgrade_axis_bitrate: String,
    pub upgrade_show_downgrades: bool,
    pub upgrade_auto_replace_reasons: String,
    pub v8_max_memory_mb: i64,
    pub v8_idle_timeout_s: i64,
    pub update_check_enabled: bool,
    pub opds_page_index_zero_based: bool,
    pub scan_barren_page_tolerance: i64,
    pub global_search_timeout_secs: i64,
}

#[derive(sqlx::FromRow)]
pub struct DownloadRuleRow {
    pub id: i64,
    pub manga_id: i64,
    pub rule_type: String,
    pub value: String,
}

/// A single page entry in a downloaded chapter's CBZ archive.
#[derive(Debug, Serialize)]
pub struct PageInfo {
    pub index: usize,
    pub filename: String,
    /// `true` when the server has determined this page is a double-page spread
    /// (either a wide/landscape single image, or the first half of a split
    /// portrait scan pair).
    pub double_page: bool,
}

/// An alternative downloaded version of the same chapter from a different scanlator group.
#[derive(Debug, Serialize)]
pub struct ScanlatorAlt {
    pub chapter_id: ChapterId,
    pub scanlator: Option<String>,
    /// Volume number, included so the frontend can disambiguate identical scanlator strings.
    pub volume: Option<i64>,
}

/// Manifest returned when opening a downloaded chapter for reading.
#[derive(Debug, Serialize)]
pub struct ChapterPageManifest {
    pub chapter_id: ChapterId,
    pub chapter_title: String,
    pub chapter_number: f64,
    pub scanlator: Option<String>,
    pub source_name: String,
    pub manga_id: MangaId,
    pub manga_title: String,
    pub page_count: usize,
    pub pages: Vec<PageInfo>,
    pub prev_chapter_id: Option<ChapterId>,
    pub next_chapter_id: Option<ChapterId>,
    pub last_page_read: Option<i64>,
    /// `true` when the CBZ contains server-analysed spread metadata (a
    /// `<Pages>` block in `ComicInfo.xml`).  When `false`, the reader
    /// should fall back to client-side edge detection.
    pub spread_analysed: bool,
    /// Other downloaded chapters for the same chapter number from different scanlators.
    pub scanlator_alternatives: Vec<ScanlatorAlt>,
}

/// Full manga row as stored in the database.
#[derive(Serialize, Deserialize, Clone, Debug, sqlx::FromRow)]
pub struct Manga {
    pub id: MangaId,
    pub source_id: i64,
    pub source_manga_id: String,
    pub name: String,
    pub cover_url: Option<String>,
    pub local_cover_path: Option<String>,
    pub local_name: Option<String>,
    pub local_description: Option<String>,
    pub local_status: Option<i64>,
    pub cover_overridden: bool,
    pub description: Option<String>,
    pub auto_download: bool,
    pub auto_scan: bool,
    #[sqlx(try_from = "i64")]
    pub status: kani_shared::MangaStatus,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: time::OffsetDateTime,
    pub scanlator_mode: String,
    pub download_all_preferred_only: bool,
    pub notes: Option<String>,
    pub is_orphaned: bool,
    pub cover_hash: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub deleted_at: Option<time::OffsetDateTime>,
    pub upgrade_auto_replace: bool,
    pub suppressed_chapter_count: i64,
}

/// DB row fetched when listing chapters for a manga.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ChapterRow {
    pub id: ChapterId,
    pub source_chapter_id: String,
    pub name: Option<String>,
    pub chapter_number: f64,
    pub volume: Option<i64>,
    pub language: String,
    pub scanlator: Option<String>,
    pub uploaded_at: Option<i64>,
    pub download_status: kani_shared::types::DownloadStatus,
    pub is_orphaned: bool,
    pub page_count: Option<i64>,
    pub is_read: Option<bool>,
    pub last_page_read: Option<i64>,
    pub download_error: Option<String>,
    pub upgrade_available: Option<String>,
}

/// Slim row returned by filtered library queries (joins manga + source).
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct LibraryManga {
    pub id: MangaId,
    pub source_id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub local_cover_path: Option<String>,
    pub cover_hash: Option<String>,
    pub base_url: String,
    /// Total matching rows (populated by COUNT(*) OVER() in get_library_filtered).
    #[sqlx(default)]
    pub total_count: i64,
    /// Chapters discovered after the user last viewed this manga.
    #[sqlx(default)]
    pub new_chapter_count: i64,
    #[sqlx(default)]
    pub is_orphaned: bool,
    /// `pending` or `unlinked` while an imported id has no proven replacement.
    #[sqlx(default)]
    pub import_link_status: Option<String>,
    /// In-progress chapter (partially read) to power a hover "resume" affordance
    /// on the library grid card. `None` when the user has no in-progress chapter
    /// for this manga.
    #[sqlx(default)]
    pub resume_chapter_id: Option<i64>,
    #[sqlx(default)]
    pub resume_chapter_number: Option<f64>,
    #[sqlx(default)]
    pub resume_last_page: Option<i64>,
    #[sqlx(default)]
    pub resume_page_count: Option<i64>,
    #[sqlx(default)]
    pub match_field: Option<String>,
    #[sqlx(default)]
    pub match_text: Option<String>,
}

/// One item in the "continue reading" shelf.
#[derive(Debug, Serialize)]
pub struct ContinueReadingItem {
    pub manga_id: MangaId,
    pub source_id: i64,
    pub manga_name: String,
    pub cover_url: Option<String>,
    pub local_cover_path: Option<String>,
    pub cover_hash: Option<String>,
    pub base_url: String,
    pub chapter_id: ChapterId,
    pub chapter_number: f64,
    pub last_page: i64,
    pub page_count: i64,
}

/// Payload for updating local metadata overrides on a manga.
/// `None` for scalar fields means "no change"; pass an explicit `None`-wrapped `Option<String>`
/// as `Some(None)` if you need to clear — but the REST layer serialises nulls as `None` directly.
/// For people/tags lists: `None` = no change, `Some(vec![])` = clear override.
pub struct LocalMetadataUpdate {
    pub local_name: Option<String>,
    pub local_description: Option<String>,
    pub local_status: Option<i64>,
    pub authors: Option<Vec<String>>,
    pub artists: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RefreshFields {
    pub cover: bool,
    pub title: bool,
    pub description: bool,
    pub status: bool,
    pub people: bool,
    pub tags: bool,
}

impl Default for RefreshFields {
    fn default() -> Self {
        Self {
            cover: true,
            title: true,
            description: true,
            status: true,
            people: true,
            tags: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RefreshOptions {
    pub fields: RefreshFields,
    pub fetch_chapters: bool,
    pub clear_overrides: bool,
}

impl Default for RefreshOptions {
    fn default() -> Self {
        Self {
            fields: RefreshFields::default(),
            fetch_chapters: true,
            clear_overrides: false,
        }
    }
}

/// All data required to render the manga-details page.
/// URL signing and markdown rendering are left to the HTTP layer.
pub struct LocalMangaDetails {
    pub manga: Manga,
    pub source: Source,
    pub auto_scan: bool,
    /// Effective display values — local override applied when present, otherwise source.
    pub authors: Vec<NamedItem>,
    pub artists: Vec<NamedItem>,
    pub tags: Vec<NamedItem>,
    /// Raw source values from the extension (for the "restore" affordance).
    pub source_authors: Vec<NamedItem>,
    pub source_artists: Vec<NamedItem>,
    pub source_tags: Vec<NamedItem>,
    /// User-entered local override names (empty when no override is active).
    pub local_authors: Vec<String>,
    pub local_artists: Vec<String>,
    pub local_tags: Vec<String>,
    pub has_local_people: bool,
    /// 'pending' or 'unlinked' while an imported id has not been proved.
    pub import_link_status: Option<String>,
    pub has_local_tags: bool,
    /// Chapters currently held for this series, excluding ones a migration
    /// orphaned. The rail states it; the chapter list's own count is filtered.
    pub chapter_count: i64,
}

/// Orphaned manga — the source they came from has been soft-deleted.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct OrphanedManga {
    pub id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub local_cover_path: Option<String>,
    pub source_name: String,
}

/// One item in the pending imports queue.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PendingImportRow {
    pub id: i64,
    pub origin: String,
    pub title: String,
    pub source_hint: Option<SourceId>,
    pub source_manga_id: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub authors: Option<String>,
    pub tags: Option<String>,
    pub status: Option<i64>,
    pub tracking: Option<String>,
    pub chapter_progress: Option<String>,
    pub possible_duplicate_of: Option<i64>,
    pub possible_duplicate_title: Option<String>,
    pub duplicate_similarity: Option<f64>,
    pub created_at: Option<String>,
}

impl TryFrom<DownloadRuleRow> for DownloadRule {
    type Error = String;

    fn try_from(row: DownloadRuleRow) -> Result<Self, Self::Error> {
        let kind = match row.rule_type.as_str() {
            "language_include" => DownloadRuleKind::LanguageInclude(row.value),
            "language_exclude" => DownloadRuleKind::LanguageExclude(row.value),
            "title_contains" => DownloadRuleKind::TitleContains(row.value),
            "title_excludes" => DownloadRuleKind::TitleExcludes(row.value),
            "chapter_number_min" => {
                let n: f64 = row
                    .value
                    .parse()
                    .map_err(|_| format!("Bad f64: {}", row.value))?;
                DownloadRuleKind::ChapterNumberMin(n)
            }
            "chapter_number_max" => {
                let n: f64 = row
                    .value
                    .parse()
                    .map_err(|_| format!("Bad f64: {}", row.value))?;
                DownloadRuleKind::ChapterNumberMax(n)
            }
            "exclude_fractional" => DownloadRuleKind::ExcludeFractional,
            "max_age_days" => {
                let n: i32 = row
                    .value
                    .parse()
                    .map_err(|_| format!("Bad i32: {}", row.value))?;
                DownloadRuleKind::MaxAgeDays(n)
            }
            "published_after" => {
                let n: i64 = row
                    .value
                    .parse()
                    .map_err(|_| format!("Bad i64: {}", row.value))?;
                DownloadRuleKind::PublishedAfter(n)
            }
            // Scanlator rules were migrated to scanlator_preferences — skip silently.
            "scanlator_include" | "scanlator_exclude" => {
                return Err(format!("Migrated rule type skipped: {}", row.rule_type));
            }
            other => return Err(format!("Unknown rule_type in DB: {}", other)),
        };
        Ok(DownloadRule {
            id: row.id,
            manga_id: row.manga_id,
            kind,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RepoRow {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub maintainer_key: String,
    pub trusted_level: String,
    pub last_refreshed_at: Option<String>,
    pub index_cache: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct BlockedRepo {
    pub id: i64,
    pub url: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AuditEntry {
    pub id: i64,
    pub user_id: Option<i64>,
    pub username: Option<String>,
    pub action: String,
    pub target: Option<String>,
    pub details: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DailyActivity {
    pub date: String,
    pub chapters_read: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PaceEntry {
    pub date: String,
    pub pages: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MangaReadCount {
    pub manga_id: i64,
    pub manga_name: String,
    pub chapters_read: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct GenreCount {
    pub genre: String,
    pub chapters_read: i64,
}

/// Computed reading statistics for a single user.
///
/// All vector fields hold data for the requested period (default 90 days).
/// Future stat blocks should be added as `#[serde(skip_serializing_if = "Option::is_none")]`
/// `Option<T>` fields so existing clients are unaffected.
#[derive(Debug, Clone, Serialize)]
pub struct ReadingStats {
    pub total_chapters_read: i64,
    pub total_manga_read: i64,
    pub completed_manga: i64,
    pub current_streak: i64,
    pub longest_streak: i64,
    pub daily_activity: Vec<DailyActivity>,
    pub top_manga: Vec<MangaReadCount>,
    pub genre_breakdown: Vec<GenreCount>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reading_pace: Vec<PaceEntry>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn sample_manga() -> Manga {
        Manga {
            id: MangaId(1),
            source_id: 1,
            source_manga_id: "abc".into(),
            name: "Test".into(),
            cover_url: None,
            local_cover_path: None,
            local_name: None,
            local_description: None,
            local_status: None,
            cover_overridden: false,
            description: None,
            auto_download: false,
            auto_scan: false,
            status: kani_shared::MangaStatus::Ongoing,
            created_at: time::macros::datetime!(2026-07-13 04:33:34 UTC),
            updated_at: time::macros::datetime!(2026-07-13 04:33:34 UTC),
            scanlator_mode: "priority".into(),
            download_all_preferred_only: false,
            notes: None,
            is_orphaned: false,
            cover_hash: None,
            deleted_at: Some(time::macros::datetime!(2026-07-13 05:00:00 UTC)),
            upgrade_auto_replace: false,
            suppressed_chapter_count: 0,
        }
    }

    #[test]
    fn manga_timestamps_serialize_as_rfc3339_strings() {
        let json = serde_json::to_value(sample_manga()).unwrap();
        assert_eq!(json["deleted_at"], "2026-07-13T05:00:00Z");
        assert_eq!(json["created_at"], "2026-07-13T04:33:34Z");
        assert!(
            json["deleted_at"].is_string(),
            "JS `new Date(...)` needs a string, not the component array time's default serde emits"
        );
    }

    #[test]
    fn manga_absent_deleted_at_serializes_as_null() {
        let mut m = sample_manga();
        m.deleted_at = None;
        let json = serde_json::to_value(m).unwrap();
        assert!(json["deleted_at"].is_null());
    }

    #[test]
    fn manga_timestamps_round_trip() {
        let original = sample_manga();
        let json = serde_json::to_string(&original).unwrap();
        let back: Manga = serde_json::from_str(&json).unwrap();
        assert_eq!(back.created_at, original.created_at);
        assert_eq!(back.deleted_at, original.deleted_at);
    }
}
