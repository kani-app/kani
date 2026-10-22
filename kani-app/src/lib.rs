//! Kani application layer — business logic and orchestration.
//!
//! This crate sits between the HTTP layer (`kani-web`) and the core WASM/download
//! infrastructure (`kani-core`). It owns all database-backed orchestration that is
//! not specific to any particular interface (web, CLI, TUI, …).

pub mod cache;
pub mod error;
pub mod events;
pub mod ids;
pub mod images;
pub mod install_gating;
pub mod jobs;
pub mod models;
pub mod permissions;
pub mod service;
pub mod source;
pub mod tuning;

pub use error::ServiceError;
pub use models::{
    AuditEntry, ChapterPageManifest, ChapterRow, DailyActivity, GenreCount, LibraryManga,
    LocalMangaDetails, Manga, MangaReadCount, OrphanedManga, PageInfo, PendingImportRow,
    ReadingStats,
};
pub use service::backup::{BackupPreview, RestoreOptions, RestoreResult};
pub use service::dedup::{DuplicatePair, SimilarMangaHit};
pub use service::import::tachiyomi::{
    TachiyomiImportOptions, TachiyomiImportResult, TachiyomiPreview,
};
pub use service::{AppService, chapter_name};
