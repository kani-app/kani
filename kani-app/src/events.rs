//! Application events broadcast to SSE subscribers after service-layer state changes.

use crate::ids::MangaId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
/// Wire-level event envelope consumed by the web SSE stream and frontend cache handlers.
pub enum AppEvent {
    NewChapters {
        manga_id: MangaId,
        manga_name: String,
        count: usize,
        chapter_ids: Vec<i64>,
        chapter_names: Vec<String>,
    },
    ImportStarted {
        origin: String,
        total: u32,
    },
    ImportProgress {
        origin: String,
        completed: u32,
        total: u32,
        title: String,
    },
    ImportCompleted {
        origin: String,
        imported: u32,
        skipped: u32,
        pending: u32,
    },
    PathMigrationStarted {
        field: String,
        total_bytes: u64,
    },
    PathMigrationProgress {
        field: String,
        bytes_copied: u64,
        total_bytes: u64,
    },
    PathMigrationCompleted {
        field: String,
        new_path: String,
    },
    PathMigrationFailed {
        field: String,
        error: String,
    },
    UpgradesFound {
        manga_id: i64,
        count: u64,
    },
    ChapterListPartial {
        manga_id: MangaId,
        received: usize,
    },
    ChapterListComplete {
        manga_id: MangaId,
        total: usize,
    },
    ChapterListError {
        manga_id: MangaId,
        error: String,
    },
    JobStarted {
        job_id: uuid::Uuid,
        job_type: String,
        description: String,
    },
    JobProgress {
        job_id: uuid::Uuid,
        job_type: String,
        current: u64,
        total: u64,
        message: String,
    },
    JobCompleted {
        job_id: uuid::Uuid,
        job_type: String,
        description: String,
    },
    JobFailed {
        job_id: uuid::Uuid,
        job_type: String,
        message: String,
        retryable: bool,
    },
    JobCancelled {
        job_id: uuid::Uuid,
        job_type: String,
    },
    SourceInstalled {
        source_id: i64,
        source_name: String,
        from_repo: String,
    },
    RepoRefreshed {
        repo_id: i64,
        repo_name: String,
    },
    UpdateAvailable {
        source_id: i64,
        source_name: String,
        installed_version: String,
        available_version: String,
        repo_id: i64,
    },
    SourceUpdating {
        source_id: i64,
        source_name: String,
    },
    LibraryInvalidated,
    CircuitOpen {
        host: String,
        failure_count: u32,
    },
    #[serde(untagged)]
    Refresh(RefreshProgressEvent),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
/// Progress lifecycle for a multi-manga metadata refresh.
pub enum RefreshProgressEvent {
    Started {
        total: usize,
        manga_ids: Vec<MangaId>,
    },
    MangaRefreshed {
        manga_id: MangaId,
        manga_name: String,
        completed: usize,
        total: usize,
        success: bool,
        new_chapters: u32,
    },
    Completed {
        total: usize,
        failed: usize,
    },
}
