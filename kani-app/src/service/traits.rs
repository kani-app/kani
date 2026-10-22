//! Narrow service-domain traits used by interface adapters and test doubles.
//!
//! [`AppService`] implements these traits without changing authorization semantics; callers remain
//! responsible for enforcing permissions before entering the application layer.

use crate::error::Result;
use crate::ids::{ChapterId, MangaId, UserId};
use crate::models::{
    ChapterPageManifest, LocalMetadataUpdate, Manga, OrphanedManga, PendingImportRow,
    RefreshOptions, SourceHealthRow,
};
use crate::service::AppService;
use crate::service::backup::{BackupPreview, RestoreOptions, RestoreResult};
use crate::service::dedup::DuplicatePair;
use crate::service::import::tachiyomi::{
    TachiyomiImportOptions, TachiyomiImportResult, TachiyomiPreview,
};
use crate::service::trackers::{TrackerMangaResult, TrackerMappingItem, TrackerStatusItem};
use kani_shared::types::{
    AppSettings, Category, Chapter, ChapterSortOrder, ContinueReadingChapter, DownloadRule,
    DownloadRuleKind, MangaTracking, MangaTrackingStatus, MigrationPreview, ScanlatorPreference,
    SettingsUpdate, SortOption, Source,
};

#[async_trait::async_trait]
/// Source installation, configuration, discovery, and provider-call boundary.
pub trait SourceDomain: Send + Sync {
    async fn list_sources(&self) -> Result<Vec<Source>>;
    async fn get_source(&self, id: i64) -> Result<Source>;
    async fn update_source(
        &self,
        id: i64,
        name: Option<String>,
        version: Option<String>,
    ) -> Result<()>;
    async fn get_source_health(&self) -> Result<Vec<SourceHealthRow>>;
    async fn get_metadata(&self, id: i64) -> Result<String>;
    async fn toggle_source_enabled(&self, id: i64, enabled: bool) -> Result<()>;
    async fn toggle_source_favourite(&self, id: i64, favourited: bool) -> Result<()>;
    async fn get_filter_list(&self, id: i64) -> Result<kani_core::WitFilterList>;
    async fn get_all_preferences(&self, source_id: i64) -> Result<Vec<(String, String)>>;
    async fn set_preference(&self, source_id: i64, key: &str, value: &str) -> Result<()>;
    async fn append_pref_list_item(&self, source_id: i64, key: &str, item: String) -> Result<()>;
    async fn remove_pref_list_item(&self, source_id: i64, key: &str, item: &str) -> Result<()>;
    async fn toggle_pref_select_item(
        &self,
        source_id: i64,
        key: &str,
        item: String,
        selected: bool,
    ) -> Result<()>;
    async fn get_source_url(&self, id: i64, manga_id: &str) -> Result<String>;
    async fn get_chapter_list_paged(
        &self,
        id: i64,
        manga_id: &str,
        page: i32,
        page_size: i32,
        sort: Option<String>,
    ) -> Result<String>;
    async fn get_chapter_sort_list(&self, id: i64) -> Result<Vec<SortOption>>;
    async fn set_source_download_concurrency(
        &self,
        id: i64,
        concurrency: Option<i64>,
    ) -> Result<()>;
    async fn set_source_browser_enabled(&self, id: i64, enabled: bool) -> Result<()>;
    async fn delete_source(&self, id: i64, user_id: UserId) -> Result<()>;
    async fn list_active_source_ids(&self) -> Result<Vec<i64>>;
    async fn get_source_pref_schema(
        &self,
        source_id: i64,
    ) -> Result<Vec<kani_core::PreferenceSpec>>;
}

#[async_trait::async_trait]
impl SourceDomain for AppService {
    async fn list_sources(&self) -> Result<Vec<Source>> {
        self.list_sources().await
    }

    async fn get_source(&self, id: i64) -> Result<Source> {
        self.get_source(id).await
    }

    async fn update_source(
        &self,
        id: i64,
        name: Option<String>,
        version: Option<String>,
    ) -> Result<()> {
        self.update_source(id, name, version).await
    }

    async fn get_source_health(&self) -> Result<Vec<SourceHealthRow>> {
        self.get_source_health().await
    }

    async fn get_metadata(&self, id: i64) -> Result<String> {
        self.get_metadata(id).await
    }

    async fn toggle_source_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        self.toggle_source_enabled(id, enabled).await
    }

    async fn toggle_source_favourite(&self, id: i64, favourited: bool) -> Result<()> {
        self.toggle_source_favourite(id, favourited).await
    }

    async fn get_filter_list(&self, id: i64) -> Result<kani_core::WitFilterList> {
        self.get_filter_list(id).await
    }

    async fn get_all_preferences(&self, source_id: i64) -> Result<Vec<(String, String)>> {
        self.get_all_preferences(source_id).await
    }

    async fn set_preference(&self, source_id: i64, key: &str, value: &str) -> Result<()> {
        self.set_preference(source_id, key, value).await
    }

    async fn append_pref_list_item(&self, source_id: i64, key: &str, item: String) -> Result<()> {
        self.append_pref_list_item(source_id, key, item).await
    }

    async fn remove_pref_list_item(&self, source_id: i64, key: &str, item: &str) -> Result<()> {
        self.remove_pref_list_item(source_id, key, item).await
    }

    async fn toggle_pref_select_item(
        &self,
        source_id: i64,
        key: &str,
        item: String,
        selected: bool,
    ) -> Result<()> {
        self.toggle_pref_select_item(source_id, key, item, selected)
            .await
    }

    async fn get_source_url(&self, id: i64, manga_id: &str) -> Result<String> {
        self.get_source_url(id, manga_id).await
    }

    async fn get_chapter_list_paged(
        &self,
        id: i64,
        manga_id: &str,
        page: i32,
        page_size: i32,
        sort: Option<String>,
    ) -> Result<String> {
        self.get_chapter_list_paged(id, manga_id, page, page_size, sort)
            .await
    }

    async fn get_chapter_sort_list(&self, id: i64) -> Result<Vec<SortOption>> {
        self.get_chapter_sort_list(id).await
    }

    async fn set_source_download_concurrency(
        &self,
        id: i64,
        concurrency: Option<i64>,
    ) -> Result<()> {
        self.set_source_download_concurrency(id, concurrency).await
    }

    async fn set_source_browser_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        self.set_source_browser_enabled(id, enabled).await
    }

    async fn delete_source(&self, id: i64, user_id: UserId) -> Result<()> {
        self.delete_source(id, user_id).await
    }

    async fn list_active_source_ids(&self) -> Result<Vec<i64>> {
        self.list_active_source_ids().await
    }

    async fn get_source_pref_schema(
        &self,
        source_id: i64,
    ) -> Result<Vec<kani_core::PreferenceSpec>> {
        self.get_source_pref_schema(source_id).await
    }
}

#[async_trait::async_trait]
/// Chapter download submission, cancellation, history, and local-file boundary.
pub trait DownloadDomain: Send + Sync {
    async fn download_chapter(&self, chapter_id: ChapterId) -> Result<uuid::Uuid>;
    async fn retry_chapter_download(&self, chapter_id: ChapterId) -> Result<uuid::Uuid>;
    async fn delete_downloaded(&self, chapter_id: ChapterId) -> Result<()>;
    async fn cancel_download(&self, chapter_id: ChapterId) -> Result<()>;
    async fn cancel_all_global_downloads(&self) -> Result<()>;
    async fn get_download_history(&self, limit: i64) -> Result<Vec<serde_json::Value>>;
    async fn get_manga_download_status(&self, manga_id: MangaId) -> Result<serde_json::Value>;
}

#[async_trait::async_trait]
impl DownloadDomain for AppService {
    async fn download_chapter(&self, chapter_id: ChapterId) -> Result<uuid::Uuid> {
        self.download_chapter(chapter_id).await
    }

    async fn retry_chapter_download(&self, chapter_id: ChapterId) -> Result<uuid::Uuid> {
        self.retry_chapter_download(chapter_id).await
    }

    async fn delete_downloaded(&self, chapter_id: ChapterId) -> Result<()> {
        self.delete_downloaded(chapter_id).await
    }

    async fn cancel_download(&self, chapter_id: ChapterId) -> Result<()> {
        self.cancel_download(chapter_id).await
    }

    async fn cancel_all_global_downloads(&self) -> Result<()> {
        self.cancel_all_global_downloads().await
    }

    async fn get_download_history(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        self.get_download_history(limit).await
    }

    async fn get_manga_download_status(&self, manga_id: MangaId) -> Result<serde_json::Value> {
        self.get_manga_download_status(manga_id).await
    }
}

pub use crate::jobs::manager::{JobListFilter, JobListPage, JobStatus, JobSummary};

#[async_trait::async_trait]
/// Background-job query and control boundary.
pub trait JobDomain: Send + Sync {
    async fn list_jobs(&self, filter: JobListFilter) -> Result<JobListPage>;
    async fn get_job_status(&self, id: uuid::Uuid) -> Result<JobStatus>;
    async fn cancel_job(&self, id: uuid::Uuid) -> Result<()>;
    async fn pause_job(&self, id: uuid::Uuid) -> Result<()>;
    async fn resume_job(&self, id: uuid::Uuid) -> Result<()>;
    fn active_job_summaries(&self) -> Vec<serde_json::Value>;
}

#[async_trait::async_trait]
impl JobDomain for AppService {
    async fn list_jobs(&self, filter: JobListFilter) -> Result<JobListPage> {
        self.job_manager.list_jobs(filter).await
    }

    async fn get_job_status(&self, id: uuid::Uuid) -> Result<JobStatus> {
        self.job_manager.status(id).await
    }

    async fn cancel_job(&self, id: uuid::Uuid) -> Result<()> {
        self.job_manager.cancel(id).await
    }

    async fn pause_job(&self, id: uuid::Uuid) -> Result<()> {
        self.job_manager.pause(id).await
    }

    async fn resume_job(&self, id: uuid::Uuid) -> Result<()> {
        self.job_manager.resume(id).await
    }

    fn active_job_summaries(&self) -> Vec<serde_json::Value> {
        self.job_manager.active_job_summaries()
    }
}

#[async_trait::async_trait]
/// Chapter listing, progress, annotation, and page-manifest boundary.
pub trait ChapterDomain: Send + Sync {
    async fn get_chapter_page_manifest(
        &self,
        chapter_id: ChapterId,
        user_id: UserId,
    ) -> Result<ChapterPageManifest>;
    async fn set_chapter_progress(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        page: i64,
    ) -> Result<()>;
    async fn get_bookmarks(&self, user_id: UserId, chapter_id: ChapterId) -> Result<Vec<i64>>;
    async fn toggle_bookmark(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        page_index: i64,
    ) -> Result<bool>;
    async fn get_chapter_note(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> Result<Option<String>>;
    async fn set_chapter_note(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        note: &str,
    ) -> Result<()>;
    async fn get_manga_chapter_notes_with_text(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<Vec<(ChapterId, f64, String)>>;
    async fn set_chapter_read_status(
        &self,
        user_id: UserId,
        chapter_ids: Vec<ChapterId>,
        is_read: bool,
    ) -> Result<()>;
    async fn get_continue_reading_chapter(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<Option<ContinueReadingChapter>>;
    async fn get_chapters_up_to(
        &self,
        manga_id: MangaId,
        chapter_number: f64,
    ) -> Result<Vec<ChapterId>>;
}

#[async_trait::async_trait]
impl ChapterDomain for AppService {
    async fn get_chapter_page_manifest(
        &self,
        chapter_id: ChapterId,
        user_id: UserId,
    ) -> Result<ChapterPageManifest> {
        self.get_chapter_page_manifest(chapter_id, user_id).await
    }

    async fn set_chapter_progress(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        page: i64,
    ) -> Result<()> {
        self.set_chapter_progress(user_id, chapter_id, page).await
    }

    async fn get_bookmarks(&self, user_id: UserId, chapter_id: ChapterId) -> Result<Vec<i64>> {
        self.get_bookmarks(user_id, chapter_id).await
    }

    async fn toggle_bookmark(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        page_index: i64,
    ) -> Result<bool> {
        self.toggle_bookmark(user_id, chapter_id, page_index).await
    }

    async fn get_chapter_note(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> Result<Option<String>> {
        self.get_chapter_note(user_id, chapter_id).await
    }

    async fn set_chapter_note(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        note: &str,
    ) -> Result<()> {
        self.set_chapter_note(user_id, chapter_id, note).await
    }

    async fn get_manga_chapter_notes_with_text(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<Vec<(ChapterId, f64, String)>> {
        self.get_manga_chapter_notes_with_text(user_id, manga_id)
            .await
    }

    async fn set_chapter_read_status(
        &self,
        user_id: UserId,
        chapter_ids: Vec<ChapterId>,
        is_read: bool,
    ) -> Result<()> {
        self.set_chapter_read_status(user_id, chapter_ids, is_read)
            .await
    }

    async fn get_continue_reading_chapter(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<Option<ContinueReadingChapter>> {
        self.get_continue_reading_chapter(user_id, manga_id).await
    }

    async fn get_chapters_up_to(
        &self,
        manga_id: MangaId,
        chapter_number: f64,
    ) -> Result<Vec<ChapterId>> {
        self.get_chapters_up_to(manga_id, chapter_number).await
    }
}

#[async_trait::async_trait]
/// Library listing, scanning, import, backup, and duplicate-management boundary.
pub trait LibraryDomain: Send + Sync {
    async fn scan_all_manga(&self) -> Result<uuid::Uuid>;
    async fn scan_manga_ids(&self, ids: Vec<MangaId>) -> Result<uuid::Uuid>;
    async fn get_library(&self, page: i32, order: i32) -> Result<Vec<Manga>>;
    async fn export_backup(
        &self,
        user_id: UserId,
        include_progress: bool,
        passphrase: Option<String>,
    ) -> Result<Vec<u8>>;
    async fn preview_backup(
        &self,
        data: &[u8],
        passphrase: Option<String>,
    ) -> Result<BackupPreview>;
    async fn restore_backup(
        &self,
        user_id: UserId,
        data: &[u8],
        opts: RestoreOptions,
        passphrase: Option<String>,
    ) -> Result<RestoreResult>;
    async fn preview_tachiyomi_backup(&self, data: &[u8]) -> Result<TachiyomiPreview>;
    async fn import_tachiyomi_backup(
        &self,
        user_id: UserId,
        data: &[u8],
        opts: TachiyomiImportOptions,
    ) -> Result<TachiyomiImportResult>;
    async fn list_pending_imports(&self, user_id: UserId) -> Result<Vec<PendingImportRow>>;
    async fn delete_pending_import(&self, user_id: UserId, id: i64) -> Result<()>;
    async fn resolve_pending_import(
        &self,
        user_id: UserId,
        id: i64,
        source_id: i64,
        source_manga_id: &str,
    ) -> Result<MangaId>;
    async fn list_orphaned_manga(&self) -> Result<Vec<OrphanedManga>>;
    async fn list_duplicates(&self) -> Result<Vec<DuplicatePair>>;
    async fn merge_duplicate(&self, keep_id: i64, discard_id: i64, user_id: UserId) -> Result<()>;
    async fn scan_duplicates(&self, user_id: UserId) -> Result<u32>;
    async fn dismiss_duplicate(&self, a: MangaId, b: MangaId) -> Result<()>;
}

#[async_trait::async_trait]
impl LibraryDomain for AppService {
    async fn scan_all_manga(&self) -> Result<uuid::Uuid> {
        self.scan_all_manga().await
    }

    async fn scan_manga_ids(&self, ids: Vec<MangaId>) -> Result<uuid::Uuid> {
        self.scan_manga_ids(ids).await
    }

    async fn get_library(&self, page: i32, order: i32) -> Result<Vec<Manga>> {
        self.get_library(page, order).await
    }

    async fn export_backup(
        &self,
        user_id: UserId,
        include_progress: bool,
        passphrase: Option<String>,
    ) -> Result<Vec<u8>> {
        self.export_backup(user_id, include_progress, passphrase)
            .await
    }

    async fn preview_backup(
        &self,
        data: &[u8],
        passphrase: Option<String>,
    ) -> Result<BackupPreview> {
        self.preview_backup(data, passphrase).await
    }

    async fn restore_backup(
        &self,
        user_id: UserId,
        data: &[u8],
        opts: RestoreOptions,
        passphrase: Option<String>,
    ) -> Result<RestoreResult> {
        self.restore_backup(user_id, data, opts, passphrase).await
    }

    async fn preview_tachiyomi_backup(&self, data: &[u8]) -> Result<TachiyomiPreview> {
        self.preview_tachiyomi_backup(data).await
    }

    async fn import_tachiyomi_backup(
        &self,
        user_id: UserId,
        data: &[u8],
        opts: TachiyomiImportOptions,
    ) -> Result<TachiyomiImportResult> {
        self.import_tachiyomi_backup(user_id, data, opts).await
    }

    async fn list_pending_imports(&self, user_id: UserId) -> Result<Vec<PendingImportRow>> {
        self.list_pending_imports(user_id).await
    }

    async fn delete_pending_import(&self, user_id: UserId, id: i64) -> Result<()> {
        self.delete_pending_import(user_id, id).await
    }

    async fn resolve_pending_import(
        &self,
        user_id: UserId,
        id: i64,
        source_id: i64,
        source_manga_id: &str,
    ) -> Result<MangaId> {
        self.resolve_pending_import(user_id, id, source_id, source_manga_id)
            .await
    }

    async fn list_orphaned_manga(&self) -> Result<Vec<OrphanedManga>> {
        self.list_orphaned_manga().await
    }

    async fn list_duplicates(&self) -> Result<Vec<DuplicatePair>> {
        self.list_duplicates().await
    }

    async fn merge_duplicate(&self, keep_id: i64, discard_id: i64, user_id: UserId) -> Result<()> {
        self.merge_duplicate(keep_id, discard_id, user_id).await
    }

    async fn scan_duplicates(&self, user_id: UserId) -> Result<u32> {
        self.scan_duplicates(user_id).await
    }

    async fn dismiss_duplicate(&self, a: MangaId, b: MangaId) -> Result<()> {
        self.dismiss_duplicate(a, b).await
    }
}

#[async_trait::async_trait]
/// Per-manga metadata, rules, migration, upgrades, and trash boundary.
pub trait MangaDomain: Send + Sync {
    async fn get_manga_by_id(&self, id: MangaId) -> Result<Manga>;
    async fn delete_manga(&self, id: MangaId, user_id: UserId) -> Result<()>;
    async fn upload_manga_cover(
        &self,
        manga_id: MangaId,
        bytes: Vec<u8>,
        content_type: &str,
        user_id: UserId,
    ) -> Result<()>;
    async fn clear_manga_cover_override(&self, manga_id: MangaId, user_id: UserId) -> Result<()>;
    #[allow(clippy::too_many_arguments)]
    async fn get_local_chapters(
        &self,
        manga_id: MangaId,
        page: i32,
        page_size: i32,
        sort_order: ChapterSortOrder,
        user_id: UserId,
        filter_downloaded: Option<bool>,
        filter_unread: Option<bool>,
        filter_scanlator: Option<String>,
        filter_orphaned: Option<bool>,
    ) -> Result<(Vec<Chapter>, bool, Option<u32>, u32)>;
    #[allow(clippy::too_many_arguments)]
    async fn get_chapter_ids(
        &self,
        manga_id: MangaId,
        user_id: UserId,
        sort_order: ChapterSortOrder,
        filter_downloaded: Option<bool>,
        filter_unread: Option<bool>,
        filter_scanlator: Option<String>,
        preferred_only: bool,
        filter_orphaned: Option<bool>,
    ) -> Result<Vec<ChapterId>>;
    async fn download_all_chapters(&self, manga_id: MangaId) -> Result<uuid::Uuid>;
    async fn queue_manga_scan(&self, manga_id: MangaId, trigger: String) -> Result<uuid::Uuid>;
    async fn cancel_all_downloads(&self, manga_id: MangaId) -> Result<()>;
    async fn refresh_manga_with_options(
        &self,
        manga_id: MangaId,
        opts: RefreshOptions,
    ) -> Result<()>;
    async fn scan_for_new_chapters(&self, manga_id: MangaId) -> Result<Vec<i64>>;
    async fn toggle_auto_download(&self, manga_id: MangaId, enabled: bool) -> Result<()>;
    async fn toggle_auto_scan_manga(&self, manga_id: MangaId, enabled: bool) -> Result<()>;
    async fn toggle_download_all_preferred(&self, manga_id: MangaId, enabled: bool) -> Result<()>;
    async fn update_manga_notes(&self, manga_id: MangaId, notes: Option<String>) -> Result<()>;
    async fn update_local_metadata(
        &self,
        manga_id: MangaId,
        update: LocalMetadataUpdate,
        user_id: UserId,
    ) -> Result<()>;
    async fn mark_manga_seen(&self, user_id: UserId, manga_id: MangaId) -> Result<()>;
    async fn preview_migration(
        &self,
        manga_id: MangaId,
        target_source_id: i64,
        target_source_manga_id: String,
    ) -> Result<MigrationPreview>;
    /// Queues the migration; the caller polls the returned job.
    async fn submit_migration(
        &self,
        manga_id: MangaId,
        target_source_id: i64,
        target_source_manga_id: String,
        keep_orphaned_downloads: bool,
    ) -> Result<crate::jobs::JobId>;
    async fn match_migration_targets(
        &self,
        target_source_id: i64,
        queries: Vec<crate::service::migration::MigrationMatchQuery>,
    ) -> Result<Vec<crate::service::migration::MigrationMatch>>;
    async fn submit_bulk_migration(
        &self,
        target_source_id: i64,
        items: Vec<crate::service::migration::BulkMigrationItem>,
        keep_orphaned_downloads: bool,
    ) -> Result<Vec<crate::service::migration::BulkMigrationSubmission>>;
    async fn migration_statuses(&self, job_ids: Vec<crate::jobs::JobId>) -> Result<Vec<JobStatus>>;
    async fn get_download_rules(&self, manga_id: MangaId) -> Result<Vec<DownloadRule>>;
    async fn add_download_rule(&self, manga_id: MangaId, kind: DownloadRuleKind) -> Result<i64>;
    async fn delete_download_rule(&self, rule_id: i64) -> Result<()>;
    async fn update_download_rule(&self, rule_id: i64, kind: DownloadRuleKind) -> Result<()>;
    async fn reorder_download_rules(&self, manga_id: MangaId, ordered_ids: Vec<i64>) -> Result<()>;
    async fn preview_download_rules(
        &self,
        manga_id: MangaId,
        kinds: Vec<DownloadRuleKind>,
    ) -> Result<(usize, usize)>;
    async fn trash_manga(&self, id: MangaId, user_id: UserId) -> Result<uuid::Uuid>;
    async fn untrash_manga(&self, id: MangaId, user_id: UserId) -> Result<()>;
    async fn untrash_by_token(&self, token: uuid::Uuid, user_id: UserId) -> Result<()>;
    async fn queue_manga_refresh(
        &self,
        manga_id: MangaId,
        opts: crate::models::RefreshOptions,
    ) -> Result<uuid::Uuid>;
    async fn queue_import_relink(&self, manga_id: MangaId) -> Result<uuid::Uuid>;
    async fn list_trash(&self) -> Result<Vec<Manga>>;
    async fn purge_all_trash(&self) -> Result<u64>;
}

#[async_trait::async_trait]
impl MangaDomain for AppService {
    async fn get_manga_by_id(&self, id: MangaId) -> Result<Manga> {
        self.get_manga_by_id(id).await
    }

    async fn delete_manga(&self, id: MangaId, user_id: UserId) -> Result<()> {
        self.delete_manga(id, user_id).await
    }

    async fn upload_manga_cover(
        &self,
        manga_id: MangaId,
        bytes: Vec<u8>,
        content_type: &str,
        user_id: UserId,
    ) -> Result<()> {
        self.upload_manga_cover(manga_id, bytes, content_type, user_id)
            .await
    }

    async fn clear_manga_cover_override(&self, manga_id: MangaId, user_id: UserId) -> Result<()> {
        self.clear_manga_cover_override(manga_id, user_id).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn get_local_chapters(
        &self,
        manga_id: MangaId,
        page: i32,
        page_size: i32,
        sort_order: ChapterSortOrder,
        user_id: UserId,
        filter_downloaded: Option<bool>,
        filter_unread: Option<bool>,
        filter_scanlator: Option<String>,
        filter_orphaned: Option<bool>,
    ) -> Result<(Vec<Chapter>, bool, Option<u32>, u32)> {
        self.get_local_chapters(
            manga_id,
            page,
            page_size,
            sort_order,
            user_id,
            filter_downloaded,
            filter_unread,
            filter_scanlator,
            filter_orphaned,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn get_chapter_ids(
        &self,
        manga_id: MangaId,
        user_id: UserId,
        sort_order: ChapterSortOrder,
        filter_downloaded: Option<bool>,
        filter_unread: Option<bool>,
        filter_scanlator: Option<String>,
        preferred_only: bool,
        filter_orphaned: Option<bool>,
    ) -> Result<Vec<ChapterId>> {
        self.get_chapter_ids(
            manga_id,
            user_id,
            sort_order,
            filter_downloaded,
            filter_unread,
            filter_scanlator,
            preferred_only,
            filter_orphaned,
        )
        .await
    }

    async fn download_all_chapters(&self, manga_id: MangaId) -> Result<uuid::Uuid> {
        self.download_all_chapters(manga_id).await
    }

    async fn queue_manga_scan(&self, manga_id: MangaId, trigger: String) -> Result<uuid::Uuid> {
        let row = sqlx::query!("SELECT source_id, name FROM manga WHERE id = ?", manga_id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| {
                crate::error::ServiceError::NotFound(format!("Manga {manga_id} not found"))
            })?;
        let job =
            crate::jobs::download::SourceScanJob::new(manga_id.0, row.name, row.source_id, trigger);
        self.job_manager
            .submit(job)
            .await
            .map_err(|e| crate::error::ServiceError::Internal(e.to_string()))
    }

    async fn cancel_all_downloads(&self, manga_id: MangaId) -> Result<()> {
        self.cancel_all_downloads(manga_id).await
    }

    async fn refresh_manga_with_options(
        &self,
        manga_id: MangaId,
        opts: RefreshOptions,
    ) -> Result<()> {
        self.refresh_manga_with_options(manga_id, opts).await
    }

    async fn scan_for_new_chapters(&self, manga_id: MangaId) -> Result<Vec<i64>> {
        self.scan_for_new_chapters(manga_id).await
    }

    async fn toggle_auto_download(&self, manga_id: MangaId, enabled: bool) -> Result<()> {
        self.toggle_auto_download(manga_id, enabled).await
    }

    async fn toggle_auto_scan_manga(&self, manga_id: MangaId, enabled: bool) -> Result<()> {
        self.toggle_auto_scan_manga(manga_id, enabled).await
    }

    async fn toggle_download_all_preferred(&self, manga_id: MangaId, enabled: bool) -> Result<()> {
        self.toggle_download_all_preferred(manga_id, enabled).await
    }

    async fn update_manga_notes(&self, manga_id: MangaId, notes: Option<String>) -> Result<()> {
        self.update_manga_notes(manga_id, notes).await
    }

    async fn update_local_metadata(
        &self,
        manga_id: MangaId,
        update: LocalMetadataUpdate,
        user_id: UserId,
    ) -> Result<()> {
        self.update_local_metadata(manga_id, update, user_id).await
    }

    async fn mark_manga_seen(&self, user_id: UserId, manga_id: MangaId) -> Result<()> {
        self.mark_manga_seen(user_id, manga_id).await
    }

    async fn preview_migration(
        &self,
        manga_id: MangaId,
        target_source_id: i64,
        target_source_manga_id: String,
    ) -> Result<MigrationPreview> {
        self.preview_migration(manga_id, target_source_id, target_source_manga_id)
            .await
    }

    async fn submit_migration(
        &self,
        manga_id: MangaId,
        target_source_id: i64,
        target_source_manga_id: String,
        keep_orphaned_downloads: bool,
    ) -> Result<crate::jobs::JobId> {
        self.submit_migration(
            manga_id,
            target_source_id,
            target_source_manga_id,
            keep_orphaned_downloads,
        )
        .await
    }

    async fn match_migration_targets(
        &self,
        target_source_id: i64,
        queries: Vec<crate::service::migration::MigrationMatchQuery>,
    ) -> Result<Vec<crate::service::migration::MigrationMatch>> {
        self.match_migration_targets(target_source_id, queries)
            .await
    }

    async fn submit_bulk_migration(
        &self,
        target_source_id: i64,
        items: Vec<crate::service::migration::BulkMigrationItem>,
        keep_orphaned_downloads: bool,
    ) -> Result<Vec<crate::service::migration::BulkMigrationSubmission>> {
        self.submit_bulk_migration(target_source_id, items, keep_orphaned_downloads)
            .await
    }

    async fn migration_statuses(&self, job_ids: Vec<crate::jobs::JobId>) -> Result<Vec<JobStatus>> {
        self.migration_statuses(job_ids).await
    }

    async fn get_download_rules(&self, manga_id: MangaId) -> Result<Vec<DownloadRule>> {
        self.get_download_rules(manga_id).await
    }

    async fn add_download_rule(&self, manga_id: MangaId, kind: DownloadRuleKind) -> Result<i64> {
        self.add_download_rule(manga_id, kind).await
    }

    async fn delete_download_rule(&self, rule_id: i64) -> Result<()> {
        self.delete_download_rule(rule_id).await
    }

    async fn update_download_rule(&self, rule_id: i64, kind: DownloadRuleKind) -> Result<()> {
        self.update_download_rule(rule_id, kind).await
    }

    async fn reorder_download_rules(&self, manga_id: MangaId, ordered_ids: Vec<i64>) -> Result<()> {
        self.reorder_download_rules(manga_id, ordered_ids).await
    }

    async fn preview_download_rules(
        &self,
        manga_id: MangaId,
        kinds: Vec<DownloadRuleKind>,
    ) -> Result<(usize, usize)> {
        self.preview_download_rules(manga_id, kinds).await
    }

    async fn trash_manga(&self, id: MangaId, user_id: UserId) -> Result<uuid::Uuid> {
        self.trash_manga(id, user_id).await?;
        let token = uuid::Uuid::new_v4();
        self.undo_tokens
            .insert(token, (id, std::time::Instant::now()));
        Ok(token)
    }

    async fn untrash_manga(&self, id: MangaId, user_id: UserId) -> Result<()> {
        self.untrash_manga(id, user_id).await
    }

    async fn untrash_by_token(&self, token: uuid::Uuid, user_id: UserId) -> Result<()> {
        let entry = self.undo_tokens.remove(&token).ok_or_else(|| {
            crate::error::ServiceError::NotFound("Undo token not found or expired".into())
        })?;
        let (manga_id, issued_at) = entry.1;
        if issued_at.elapsed() > std::time::Duration::from_secs(10) {
            return Err(crate::error::ServiceError::NotFound(
                "Undo token expired".into(),
            ));
        }
        self.untrash_manga(manga_id, user_id).await
    }

    async fn queue_import_relink(&self, manga_id: MangaId) -> Result<uuid::Uuid> {
        let row = sqlx::query!("SELECT source_id FROM manga WHERE id = ?", manga_id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| {
                crate::error::ServiceError::NotFound(format!("Manga {manga_id} not found"))
            })?;
        sqlx::query!(
            "INSERT OR REPLACE INTO manga_import_links (manga_id, status) VALUES (?, 'pending')",
            manga_id
        )
        .execute(&self.db)
        .await?;
        self.queue_import_resolve(Some(row.source_id)).await
    }

    async fn queue_manga_refresh(
        &self,
        manga_id: MangaId,
        opts: crate::models::RefreshOptions,
    ) -> Result<uuid::Uuid> {
        let row = sqlx::query!("SELECT name FROM manga WHERE id = ?", manga_id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| {
                crate::error::ServiceError::NotFound(format!("Manga {manga_id} not found"))
            })?;
        let job = crate::jobs::refresh::RefreshMangaJob::new(manga_id.0, row.name, opts);
        self.job_manager
            .submit(job)
            .await
            .map_err(|e| crate::error::ServiceError::Internal(e.to_string()))
    }

    async fn list_trash(&self) -> Result<Vec<Manga>> {
        self.list_trash().await
    }

    async fn purge_all_trash(&self) -> Result<u64> {
        self.purge_all_trash().await
    }
}

#[async_trait::async_trait]
/// Tracker configuration, mapping, search, and synchronization boundary.
pub trait TrackerDomain: Send + Sync {
    async fn list_trackers_status(&self, user_id: UserId) -> Result<Vec<TrackerStatusItem>>;
    async fn get_tracker_auth_url(&self, tracker_id: i64, redirect_uri: &str) -> Result<String>;
    async fn complete_tracker_oauth(
        &self,
        user_id: UserId,
        tracker_id: i64,
        code: &str,
        state: &str,
    ) -> Result<()>;
    async fn unlink_tracker(&self, user_id: UserId, tracker_id: i64) -> Result<()>;
    async fn search_tracker_manga(
        &self,
        user_id: UserId,
        tracker_id: i64,
        query: &str,
    ) -> Result<Vec<TrackerMangaResult>>;
    async fn get_tracker_mappings(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<Vec<TrackerMappingItem>>;
    async fn get_tracker_config(&self, tracker_id: i64) -> Result<Option<(String, bool)>>;
    async fn set_tracker_config(
        &self,
        tracker_id: i64,
        client_id: &str,
        client_secret: Option<&str>,
    ) -> Result<()>;
    async fn delete_tracker_config(&self, tracker_id: i64) -> Result<()>;
    async fn set_tracker_mapping(
        &self,
        user_id: UserId,
        tracker_id: i64,
        manga_id: MangaId,
        tracker_manga_id: &str,
    ) -> Result<()>;
    async fn delete_tracker_mapping(
        &self,
        user_id: UserId,
        tracker_id: i64,
        manga_id: MangaId,
    ) -> Result<()>;
    async fn sync_all_trackers(&self, user_id: UserId) -> Result<()>;
    async fn sync_manga_trackers(&self, user_id: UserId, manga_id: MangaId) -> Result<()>;
    async fn get_manga_tracking(&self, user_id: UserId, manga_id: MangaId)
    -> Result<MangaTracking>;
    async fn set_manga_status(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        status: MangaTrackingStatus,
    ) -> Result<()>;
    async fn set_manga_score(&self, user_id: UserId, manga_id: MangaId, score: f64) -> Result<()>;
    async fn set_manga_tracking_enabled(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        enabled: bool,
    ) -> Result<()>;
    async fn set_manga_notify(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        notify: bool,
    ) -> Result<()>;
    async fn set_reading_direction(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        direction: &str,
    ) -> Result<()>;
    async fn set_reader_prefs(&self, user_id: UserId, manga_id: MangaId, prefs: &str)
    -> Result<()>;
}

#[async_trait::async_trait]
impl TrackerDomain for AppService {
    async fn list_trackers_status(&self, user_id: UserId) -> Result<Vec<TrackerStatusItem>> {
        self.list_trackers_status(user_id).await
    }

    async fn get_tracker_auth_url(&self, tracker_id: i64, redirect_uri: &str) -> Result<String> {
        self.get_tracker_auth_url(tracker_id, redirect_uri).await
    }

    async fn complete_tracker_oauth(
        &self,
        user_id: UserId,
        tracker_id: i64,
        code: &str,
        state: &str,
    ) -> Result<()> {
        self.complete_tracker_oauth(user_id, tracker_id, code, state)
            .await
    }

    async fn unlink_tracker(&self, user_id: UserId, tracker_id: i64) -> Result<()> {
        self.unlink_tracker(user_id, tracker_id).await
    }

    async fn search_tracker_manga(
        &self,
        user_id: UserId,
        tracker_id: i64,
        query: &str,
    ) -> Result<Vec<TrackerMangaResult>> {
        self.search_tracker_manga(user_id, tracker_id, query).await
    }

    async fn get_tracker_mappings(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<Vec<TrackerMappingItem>> {
        self.get_tracker_mappings(user_id, manga_id).await
    }

    async fn get_tracker_config(&self, tracker_id: i64) -> Result<Option<(String, bool)>> {
        self.get_tracker_config(tracker_id).await
    }

    async fn set_tracker_config(
        &self,
        tracker_id: i64,
        client_id: &str,
        client_secret: Option<&str>,
    ) -> Result<()> {
        self.set_tracker_config(tracker_id, client_id, client_secret)
            .await
    }

    async fn delete_tracker_config(&self, tracker_id: i64) -> Result<()> {
        self.delete_tracker_config(tracker_id).await
    }

    async fn set_tracker_mapping(
        &self,
        user_id: UserId,
        tracker_id: i64,
        manga_id: MangaId,
        tracker_manga_id: &str,
    ) -> Result<()> {
        self.set_tracker_mapping(user_id, tracker_id, manga_id, tracker_manga_id)
            .await
    }

    async fn delete_tracker_mapping(
        &self,
        user_id: UserId,
        tracker_id: i64,
        manga_id: MangaId,
    ) -> Result<()> {
        self.delete_tracker_mapping(user_id, tracker_id, manga_id)
            .await
    }

    async fn sync_all_trackers(&self, user_id: UserId) -> Result<()> {
        self.sync_all_trackers(user_id).await
    }

    async fn sync_manga_trackers(&self, user_id: UserId, manga_id: MangaId) -> Result<()> {
        self.sync_manga_trackers(user_id, manga_id).await
    }

    async fn get_manga_tracking(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<MangaTracking> {
        self.get_manga_tracking(user_id, manga_id).await
    }

    async fn set_manga_status(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        status: MangaTrackingStatus,
    ) -> Result<()> {
        self.set_manga_status(user_id, manga_id, status).await
    }

    async fn set_manga_score(&self, user_id: UserId, manga_id: MangaId, score: f64) -> Result<()> {
        self.set_manga_score(user_id, manga_id, score).await
    }

    async fn set_manga_tracking_enabled(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        enabled: bool,
    ) -> Result<()> {
        self.set_manga_tracking_enabled(user_id, manga_id, enabled)
            .await
    }

    async fn set_manga_notify(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        notify: bool,
    ) -> Result<()> {
        self.set_manga_notify(user_id, manga_id, notify).await
    }

    async fn set_reading_direction(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        direction: &str,
    ) -> Result<()> {
        self.set_reading_direction(user_id, manga_id, direction)
            .await
    }

    async fn set_reader_prefs(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        prefs: &str,
    ) -> Result<()> {
        self.set_reader_prefs(user_id, manga_id, prefs).await
    }
}

#[async_trait::async_trait]
/// Category administration and manga-membership boundary.
pub trait CategoryDomain: Send + Sync {
    async fn list_categories(&self) -> Result<Vec<Category>>;
    async fn create_category(&self, name: &str, sort_order: i64) -> Result<i64>;
    async fn reorder_categories(&self, ordered_ids: Vec<i64>) -> Result<()>;
    async fn rename_category(&self, id: i64, name: &str) -> Result<()>;
    async fn delete_category(&self, id: i64) -> Result<()>;
    async fn get_manga_categories(&self, manga_id: MangaId) -> Result<Vec<Category>>;
    async fn set_manga_categories(&self, manga_id: MangaId, category_ids: Vec<i64>) -> Result<()>;
}

#[async_trait::async_trait]
impl CategoryDomain for AppService {
    async fn list_categories(&self) -> Result<Vec<Category>> {
        self.list_categories().await
    }
    async fn create_category(&self, name: &str, sort_order: i64) -> Result<i64> {
        self.create_category(name, sort_order).await
    }
    async fn reorder_categories(&self, ordered_ids: Vec<i64>) -> Result<()> {
        self.reorder_categories(ordered_ids).await
    }
    async fn rename_category(&self, id: i64, name: &str) -> Result<()> {
        self.rename_category(id, name).await
    }
    async fn delete_category(&self, id: i64) -> Result<()> {
        self.delete_category(id).await
    }
    async fn get_manga_categories(&self, manga_id: MangaId) -> Result<Vec<Category>> {
        self.get_manga_categories(manga_id).await
    }
    async fn set_manga_categories(&self, manga_id: MangaId, category_ids: Vec<i64>) -> Result<()> {
        self.set_manga_categories(manga_id, category_ids).await
    }
}

#[async_trait::async_trait]
/// Global and per-manga scanlator preference boundary.
pub trait ScanlatorDomain: Send + Sync {
    async fn get_scanlator_prefs(&self, manga_id: MangaId) -> Result<Vec<ScanlatorPreference>>;
    async fn set_scanlator_pref(
        &self,
        manga_id: MangaId,
        scanlator: &str,
        priority: i64,
        blocked: bool,
    ) -> Result<()>;
    async fn delete_scanlator_pref(&self, id: i64) -> Result<()>;
    async fn set_scanlator_mode(&self, manga_id: MangaId, mode: &str) -> Result<()>;
    async fn get_chapter_scanlators(&self, manga_id: MangaId) -> Result<Vec<String>>;
    async fn get_chapter_languages(&self, manga_id: MangaId) -> Result<Vec<String>>;
}

#[async_trait::async_trait]
impl ScanlatorDomain for AppService {
    async fn get_scanlator_prefs(&self, manga_id: MangaId) -> Result<Vec<ScanlatorPreference>> {
        self.get_scanlator_prefs(manga_id).await
    }
    async fn set_scanlator_pref(
        &self,
        manga_id: MangaId,
        scanlator: &str,
        priority: i64,
        blocked: bool,
    ) -> Result<()> {
        self.set_scanlator_pref(manga_id, scanlator, priority, blocked)
            .await
    }
    async fn delete_scanlator_pref(&self, id: i64) -> Result<()> {
        self.delete_scanlator_pref(id).await
    }
    async fn set_scanlator_mode(&self, manga_id: MangaId, mode: &str) -> Result<()> {
        self.set_scanlator_mode(manga_id, mode).await
    }
    async fn get_chapter_scanlators(&self, manga_id: MangaId) -> Result<Vec<String>> {
        self.get_chapter_scanlators(manga_id).await
    }
    async fn get_chapter_languages(&self, manga_id: MangaId) -> Result<Vec<String>> {
        self.get_chapter_languages(manga_id).await
    }
}

#[async_trait::async_trait]
/// Application settings read and update boundary.
pub trait SettingsDomain: Send + Sync {
    async fn get_settings(&self) -> AppSettings;
    async fn update_settings(&self, update: SettingsUpdate, user_id: UserId) -> Result<()>;
    async fn start_refresh_all(&self) -> Result<()>;
    async fn is_refreshing(&self) -> bool;
}

#[async_trait::async_trait]
impl SettingsDomain for AppService {
    async fn get_settings(&self) -> AppSettings {
        self.get_settings().await
    }
    async fn update_settings(&self, update: SettingsUpdate, user_id: UserId) -> Result<()> {
        self.update_settings(update, user_id).await
    }
    async fn start_refresh_all(&self) -> Result<()> {
        self.start_refresh_all().await
    }
    async fn is_refreshing(&self) -> bool {
        self.is_refreshing().await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use std::sync::Arc;

    use super::*;

    struct FixedSources {
        list: Vec<Source>,
    }

    #[async_trait::async_trait]
    impl SourceDomain for FixedSources {
        async fn list_sources(&self) -> Result<Vec<Source>> {
            Ok(self.list.clone())
        }
        async fn get_source(&self, _id: i64) -> Result<Source> {
            unimplemented!()
        }
        async fn update_source(
            &self,
            _id: i64,
            _name: Option<String>,
            _version: Option<String>,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn get_source_health(&self) -> Result<Vec<SourceHealthRow>> {
            unimplemented!()
        }
        async fn get_metadata(&self, _id: i64) -> Result<String> {
            unimplemented!()
        }
        async fn toggle_source_enabled(&self, _id: i64, _enabled: bool) -> Result<()> {
            unimplemented!()
        }
        async fn toggle_source_favourite(&self, _id: i64, _favourited: bool) -> Result<()> {
            unimplemented!()
        }
        async fn get_filter_list(&self, _id: i64) -> Result<kani_core::WitFilterList> {
            unimplemented!()
        }
        async fn get_all_preferences(&self, _source_id: i64) -> Result<Vec<(String, String)>> {
            unimplemented!()
        }
        async fn set_preference(&self, _source_id: i64, _key: &str, _value: &str) -> Result<()> {
            unimplemented!()
        }
        async fn append_pref_list_item(
            &self,
            _source_id: i64,
            _key: &str,
            _item: String,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn remove_pref_list_item(
            &self,
            _source_id: i64,
            _key: &str,
            _item: &str,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn toggle_pref_select_item(
            &self,
            _source_id: i64,
            _key: &str,
            _item: String,
            _selected: bool,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn get_source_url(&self, _id: i64, _manga_id: &str) -> Result<String> {
            unimplemented!()
        }
        async fn get_chapter_list_paged(
            &self,
            _id: i64,
            _manga_id: &str,
            _page: i32,
            _page_size: i32,
            _sort: Option<String>,
        ) -> Result<String> {
            unimplemented!()
        }
        async fn get_chapter_sort_list(&self, _id: i64) -> Result<Vec<SortOption>> {
            unimplemented!()
        }
        async fn set_source_download_concurrency(
            &self,
            _id: i64,
            _concurrency: Option<i64>,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn set_source_browser_enabled(&self, _id: i64, _enabled: bool) -> Result<()> {
            unimplemented!()
        }
        async fn delete_source(&self, _id: i64, _user_id: UserId) -> Result<()> {
            unimplemented!()
        }
        async fn list_active_source_ids(&self) -> Result<Vec<i64>> {
            unimplemented!()
        }
        async fn get_source_pref_schema(
            &self,
            _source_id: i64,
        ) -> Result<Vec<kani_core::PreferenceSpec>> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn list_sources_via_mock_no_appservice() {
        let source = Source {
            id: 1,
            name: "test-source".into(),
            version: "0.1".into(),
            base_url: "https://example.com".into(),
            enabled: true,
            favourited: false,
            unrestricted_http: false,
            browser_enabled: true,
            download_concurrency: None,
            circuit_state: None,
            icon: None,
            description: None,
            languages: None,
            schema_version: 1,
        };
        let svc: Arc<dyn SourceDomain> = Arc::new(FixedSources { list: vec![source] });
        let result = svc.list_sources().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "test-source");
    }

    struct FixedDownloads {
        history: Vec<serde_json::Value>,
    }

    #[async_trait::async_trait]
    impl DownloadDomain for FixedDownloads {
        async fn get_download_history(&self, _limit: i64) -> Result<Vec<serde_json::Value>> {
            Ok(self.history.clone())
        }
        async fn download_chapter(&self, _: ChapterId) -> Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn retry_chapter_download(&self, _: ChapterId) -> Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn delete_downloaded(&self, _: ChapterId) -> Result<()> {
            unimplemented!()
        }
        async fn cancel_download(&self, _: ChapterId) -> Result<()> {
            unimplemented!()
        }
        async fn cancel_all_global_downloads(&self) -> Result<()> {
            unimplemented!()
        }
        async fn get_manga_download_status(&self, _: MangaId) -> Result<serde_json::Value> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn download_history_via_mock_no_appservice() {
        let entry = serde_json::json!({ "id": 1, "chapter_number": 1.0 });
        let svc: Arc<dyn DownloadDomain> = Arc::new(FixedDownloads {
            history: vec![entry],
        });
        let result = svc.get_download_history(10).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["id"], 1);
    }

    struct FixedChapters;

    #[async_trait::async_trait]
    impl ChapterDomain for FixedChapters {
        async fn get_bookmarks(
            &self,
            _user_id: UserId,
            _chapter_id: ChapterId,
        ) -> Result<Vec<i64>> {
            Ok(vec![3, 7])
        }
        async fn get_chapter_page_manifest(
            &self,
            _chapter_id: ChapterId,
            _user_id: UserId,
        ) -> Result<ChapterPageManifest> {
            unimplemented!()
        }
        async fn set_chapter_progress(
            &self,
            _user_id: UserId,
            _chapter_id: ChapterId,
            _page: i64,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn toggle_bookmark(
            &self,
            _user_id: UserId,
            _chapter_id: ChapterId,
            _page_index: i64,
        ) -> Result<bool> {
            unimplemented!()
        }
        async fn get_chapter_note(
            &self,
            _user_id: UserId,
            _chapter_id: ChapterId,
        ) -> Result<Option<String>> {
            unimplemented!()
        }
        async fn set_chapter_note(
            &self,
            _user_id: UserId,
            _chapter_id: ChapterId,
            _note: &str,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn get_manga_chapter_notes_with_text(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
        ) -> Result<Vec<(ChapterId, f64, String)>> {
            unimplemented!()
        }
        async fn set_chapter_read_status(
            &self,
            _user_id: UserId,
            _chapter_ids: Vec<ChapterId>,
            _is_read: bool,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn get_continue_reading_chapter(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
        ) -> Result<Option<ContinueReadingChapter>> {
            unimplemented!()
        }
        async fn get_chapters_up_to(
            &self,
            _manga_id: MangaId,
            _chapter_number: f64,
        ) -> Result<Vec<ChapterId>> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn bookmarks_via_mock_no_appservice() {
        let svc: Arc<dyn ChapterDomain> = Arc::new(FixedChapters);
        let pages = svc.get_bookmarks(UserId(1), ChapterId(42)).await.unwrap();
        assert_eq!(pages, vec![3, 7]);
    }

    struct FixedLibrary;

    #[async_trait::async_trait]
    impl LibraryDomain for FixedLibrary {
        async fn list_orphaned_manga(&self) -> Result<Vec<OrphanedManga>> {
            Ok(vec![OrphanedManga {
                id: 99,
                name: "Orphaned Title".into(),
                cover_url: None,
                local_cover_path: None,
                source_name: "deleted-source".into(),
            }])
        }
        async fn scan_all_manga(&self) -> Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn scan_manga_ids(&self, _ids: Vec<MangaId>) -> Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn get_library(&self, _page: i32, _order: i32) -> Result<Vec<Manga>> {
            unimplemented!()
        }
        async fn export_backup(
            &self,
            _user_id: UserId,
            _include_progress: bool,
            _passphrase: Option<String>,
        ) -> Result<Vec<u8>> {
            unimplemented!()
        }
        async fn preview_backup(
            &self,
            _data: &[u8],
            _passphrase: Option<String>,
        ) -> Result<BackupPreview> {
            unimplemented!()
        }
        async fn restore_backup(
            &self,
            _user_id: UserId,
            _data: &[u8],
            _opts: RestoreOptions,
            _passphrase: Option<String>,
        ) -> Result<RestoreResult> {
            unimplemented!()
        }
        async fn preview_tachiyomi_backup(&self, _data: &[u8]) -> Result<TachiyomiPreview> {
            unimplemented!()
        }
        async fn import_tachiyomi_backup(
            &self,
            _user_id: UserId,
            _data: &[u8],
            _opts: TachiyomiImportOptions,
        ) -> Result<TachiyomiImportResult> {
            unimplemented!()
        }
        async fn list_pending_imports(&self, _user_id: UserId) -> Result<Vec<PendingImportRow>> {
            unimplemented!()
        }
        async fn delete_pending_import(&self, _user_id: UserId, _id: i64) -> Result<()> {
            unimplemented!()
        }
        async fn resolve_pending_import(
            &self,
            _user_id: UserId,
            _id: i64,
            _source_id: i64,
            _source_manga_id: &str,
        ) -> Result<MangaId> {
            unimplemented!()
        }
        async fn list_duplicates(&self) -> Result<Vec<DuplicatePair>> {
            unimplemented!()
        }
        async fn merge_duplicate(
            &self,
            _keep_id: i64,
            _discard_id: i64,
            _user_id: UserId,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn scan_duplicates(&self, _user_id: UserId) -> Result<u32> {
            unimplemented!()
        }
        async fn dismiss_duplicate(&self, _a: MangaId, _b: MangaId) -> Result<()> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn orphaned_via_mock_no_appservice() {
        let svc: Arc<dyn LibraryDomain> = Arc::new(FixedLibrary);
        let result = svc.list_orphaned_manga().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Orphaned Title");
    }

    struct FixedManga;

    #[async_trait::async_trait]
    impl MangaDomain for FixedManga {
        async fn get_download_rules(&self, _manga_id: MangaId) -> Result<Vec<DownloadRule>> {
            Ok(vec![])
        }
        async fn get_manga_by_id(&self, _id: MangaId) -> Result<Manga> {
            unimplemented!()
        }
        async fn delete_manga(&self, _id: MangaId, _user_id: UserId) -> Result<()> {
            unimplemented!()
        }
        async fn upload_manga_cover(
            &self,
            _manga_id: MangaId,
            _bytes: Vec<u8>,
            _content_type: &str,
            _user_id: UserId,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn clear_manga_cover_override(
            &self,
            _manga_id: MangaId,
            _user_id: UserId,
        ) -> Result<()> {
            unimplemented!()
        }
        #[allow(clippy::too_many_arguments)]
        async fn get_local_chapters(
            &self,
            _manga_id: MangaId,
            _page: i32,
            _page_size: i32,
            _sort_order: ChapterSortOrder,
            _user_id: UserId,
            _filter_downloaded: Option<bool>,
            _filter_unread: Option<bool>,
            _filter_scanlator: Option<String>,
            _filter_orphaned: Option<bool>,
        ) -> Result<(Vec<Chapter>, bool, Option<u32>, u32)> {
            unimplemented!()
        }
        #[allow(clippy::too_many_arguments)]
        async fn get_chapter_ids(
            &self,
            _manga_id: MangaId,
            _user_id: UserId,
            _sort_order: ChapterSortOrder,
            _filter_downloaded: Option<bool>,
            _filter_unread: Option<bool>,
            _filter_scanlator: Option<String>,
            _preferred_only: bool,
            _filter_orphaned: Option<bool>,
        ) -> Result<Vec<ChapterId>> {
            unimplemented!()
        }
        async fn download_all_chapters(&self, _manga_id: MangaId) -> Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn queue_manga_scan(
            &self,
            _manga_id: MangaId,
            _trigger: String,
        ) -> Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn cancel_all_downloads(&self, _manga_id: MangaId) -> Result<()> {
            unimplemented!()
        }
        async fn refresh_manga_with_options(
            &self,
            _manga_id: MangaId,
            _opts: RefreshOptions,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn scan_for_new_chapters(&self, _manga_id: MangaId) -> Result<Vec<i64>> {
            unimplemented!()
        }
        async fn toggle_auto_download(&self, _manga_id: MangaId, _enabled: bool) -> Result<()> {
            unimplemented!()
        }
        async fn toggle_auto_scan_manga(&self, _manga_id: MangaId, _enabled: bool) -> Result<()> {
            unimplemented!()
        }
        async fn toggle_download_all_preferred(
            &self,
            _manga_id: MangaId,
            _enabled: bool,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn update_manga_notes(
            &self,
            _manga_id: MangaId,
            _notes: Option<String>,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn update_local_metadata(
            &self,
            _manga_id: MangaId,
            _update: LocalMetadataUpdate,
            _user_id: UserId,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn mark_manga_seen(&self, _user_id: UserId, _manga_id: MangaId) -> Result<()> {
            unimplemented!()
        }
        async fn preview_migration(
            &self,
            _manga_id: MangaId,
            _target_source_id: i64,
            _target_source_manga_id: String,
        ) -> Result<MigrationPreview> {
            unimplemented!()
        }
        async fn submit_migration(
            &self,
            _manga_id: MangaId,
            _target_source_id: i64,
            _target_source_manga_id: String,
            _keep_orphaned_downloads: bool,
        ) -> Result<crate::jobs::JobId> {
            unimplemented!()
        }
        async fn match_migration_targets(
            &self,
            _target_source_id: i64,
            _queries: Vec<crate::service::migration::MigrationMatchQuery>,
        ) -> Result<Vec<crate::service::migration::MigrationMatch>> {
            unimplemented!()
        }
        async fn submit_bulk_migration(
            &self,
            _target_source_id: i64,
            _items: Vec<crate::service::migration::BulkMigrationItem>,
            _keep_orphaned_downloads: bool,
        ) -> Result<Vec<crate::service::migration::BulkMigrationSubmission>> {
            unimplemented!()
        }
        async fn migration_statuses(
            &self,
            _job_ids: Vec<crate::jobs::JobId>,
        ) -> Result<Vec<JobStatus>> {
            unimplemented!()
        }
        async fn add_download_rule(
            &self,
            _manga_id: MangaId,
            _kind: DownloadRuleKind,
        ) -> Result<i64> {
            unimplemented!()
        }
        async fn delete_download_rule(&self, _rule_id: i64) -> Result<()> {
            unimplemented!()
        }
        async fn update_download_rule(&self, _rule_id: i64, _kind: DownloadRuleKind) -> Result<()> {
            unimplemented!()
        }
        async fn reorder_download_rules(
            &self,
            _manga_id: MangaId,
            _ordered_ids: Vec<i64>,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn preview_download_rules(
            &self,
            _manga_id: MangaId,
            _kinds: Vec<DownloadRuleKind>,
        ) -> Result<(usize, usize)> {
            unimplemented!()
        }
        async fn trash_manga(&self, _id: MangaId, _user_id: UserId) -> Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn untrash_manga(&self, _id: MangaId, _user_id: UserId) -> Result<()> {
            unimplemented!()
        }
        async fn untrash_by_token(&self, _token: uuid::Uuid, _user_id: UserId) -> Result<()> {
            unimplemented!()
        }
        async fn queue_import_relink(&self, _: MangaId) -> Result<uuid::Uuid> {
            Ok(uuid::Uuid::nil())
        }

        async fn queue_manga_refresh(
            &self,
            _manga_id: MangaId,
            _opts: crate::models::RefreshOptions,
        ) -> Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn list_trash(&self) -> Result<Vec<Manga>> {
            unimplemented!()
        }
        async fn purge_all_trash(&self) -> Result<u64> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn download_rules_via_mock_no_appservice() {
        let svc: Arc<dyn MangaDomain> = Arc::new(FixedManga);
        let result = svc.get_download_rules(MangaId(1)).await.unwrap();
        assert!(result.is_empty());
    }

    struct FixedTrackers;

    #[async_trait::async_trait]
    impl TrackerDomain for FixedTrackers {
        async fn list_trackers_status(&self, _user_id: UserId) -> Result<Vec<TrackerStatusItem>> {
            Ok(vec![TrackerStatusItem {
                id: 1,
                name: "stub".into(),
                configured: false,
                linked: false,
                needs_reauth: false,
            }])
        }
        async fn get_tracker_auth_url(
            &self,
            _tracker_id: i64,
            _redirect_uri: &str,
        ) -> Result<String> {
            unimplemented!()
        }
        async fn complete_tracker_oauth(
            &self,
            _user_id: UserId,
            _tracker_id: i64,
            _code: &str,
            _state: &str,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn unlink_tracker(&self, _user_id: UserId, _tracker_id: i64) -> Result<()> {
            unimplemented!()
        }
        async fn search_tracker_manga(
            &self,
            _user_id: UserId,
            _tracker_id: i64,
            _query: &str,
        ) -> Result<Vec<TrackerMangaResult>> {
            unimplemented!()
        }
        async fn get_tracker_mappings(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
        ) -> Result<Vec<TrackerMappingItem>> {
            unimplemented!()
        }
        async fn get_tracker_config(&self, _tracker_id: i64) -> Result<Option<(String, bool)>> {
            unimplemented!()
        }
        async fn set_tracker_config(
            &self,
            _tracker_id: i64,
            _client_id: &str,
            _client_secret: Option<&str>,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn delete_tracker_config(&self, _tracker_id: i64) -> Result<()> {
            unimplemented!()
        }
        async fn set_tracker_mapping(
            &self,
            _user_id: UserId,
            _tracker_id: i64,
            _manga_id: MangaId,
            _tracker_manga_id: &str,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn delete_tracker_mapping(
            &self,
            _user_id: UserId,
            _tracker_id: i64,
            _manga_id: MangaId,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn sync_all_trackers(&self, _user_id: UserId) -> Result<()> {
            unimplemented!()
        }
        async fn sync_manga_trackers(&self, _user_id: UserId, _manga_id: MangaId) -> Result<()> {
            unimplemented!()
        }
        async fn get_manga_tracking(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
        ) -> Result<MangaTracking> {
            unimplemented!()
        }
        async fn set_manga_status(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _status: MangaTrackingStatus,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn set_manga_score(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _score: f64,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn set_manga_tracking_enabled(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _enabled: bool,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn set_manga_notify(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _notify: bool,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn set_reading_direction(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _direction: &str,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn set_reader_prefs(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _prefs: &str,
        ) -> Result<()> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn list_trackers_via_mock_no_appservice() {
        let svc: Arc<dyn TrackerDomain> = Arc::new(FixedTrackers);
        let result = svc.list_trackers_status(UserId(1)).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "stub");
    }

    struct FixedCategories;

    #[async_trait::async_trait]
    impl CategoryDomain for FixedCategories {
        async fn list_categories(&self) -> Result<Vec<Category>> {
            Ok(vec![Category {
                id: 1,
                name: "Favorites".into(),
                sort_order: 0,
            }])
        }
        async fn create_category(&self, _name: &str, _sort_order: i64) -> Result<i64> {
            unimplemented!()
        }
        async fn reorder_categories(&self, _ordered_ids: Vec<i64>) -> Result<()> {
            unimplemented!()
        }
        async fn rename_category(&self, _id: i64, _name: &str) -> Result<()> {
            unimplemented!()
        }
        async fn delete_category(&self, _id: i64) -> Result<()> {
            unimplemented!()
        }
        async fn get_manga_categories(&self, _manga_id: MangaId) -> Result<Vec<Category>> {
            unimplemented!()
        }
        async fn set_manga_categories(
            &self,
            _manga_id: MangaId,
            _category_ids: Vec<i64>,
        ) -> Result<()> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn categories_via_mock_no_appservice() {
        let svc: Arc<dyn CategoryDomain> = Arc::new(FixedCategories);
        let result = svc.list_categories().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Favorites");
    }

    struct FixedScanlators;

    #[async_trait::async_trait]
    impl ScanlatorDomain for FixedScanlators {
        async fn get_scanlator_prefs(
            &self,
            _manga_id: MangaId,
        ) -> Result<Vec<ScanlatorPreference>> {
            Ok(vec![])
        }
        async fn set_scanlator_pref(
            &self,
            _manga_id: MangaId,
            _scanlator: &str,
            _priority: i64,
            _blocked: bool,
        ) -> Result<()> {
            unimplemented!()
        }
        async fn delete_scanlator_pref(&self, _id: i64) -> Result<()> {
            unimplemented!()
        }
        async fn set_scanlator_mode(&self, _manga_id: MangaId, _mode: &str) -> Result<()> {
            unimplemented!()
        }
        async fn get_chapter_scanlators(&self, _manga_id: MangaId) -> Result<Vec<String>> {
            unimplemented!()
        }
        async fn get_chapter_languages(&self, _manga_id: MangaId) -> Result<Vec<String>> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn scanlators_via_mock_no_appservice() {
        let svc: Arc<dyn ScanlatorDomain> = Arc::new(FixedScanlators);
        let result = svc.get_scanlator_prefs(MangaId(1)).await.unwrap();
        assert!(result.is_empty());
    }

    struct FixedSettings;

    #[async_trait::async_trait]
    impl SettingsDomain for FixedSettings {
        async fn get_settings(&self) -> AppSettings {
            unimplemented!()
        }
        async fn update_settings(&self, _update: SettingsUpdate, _user_id: UserId) -> Result<()> {
            unimplemented!()
        }
        async fn start_refresh_all(&self) -> Result<()> {
            unimplemented!()
        }
        async fn is_refreshing(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn settings_via_mock_no_appservice() {
        let svc: Arc<dyn SettingsDomain> = Arc::new(FixedSettings);
        assert!(!svc.is_refreshing().await);
    }
}
