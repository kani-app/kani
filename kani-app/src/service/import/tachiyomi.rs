use std::io::Read as _;

use flate2::read::GzDecoder;
use prost::Message as _;
use serde::{Deserialize, Serialize};

use crate::error::{Result, ServiceError};
use crate::events::AppEvent;
use crate::ids::{MangaId, SourceId, UserId};
use crate::service::AppService;
use crate::service::backup::BackupManga as KaniBackupManga;
use crate::service::backup::{BackupChapterProgress, BackupMangaTracking};
use crate::service::import::progress::ImportedProgress;

use super::tachiyomi_sources::{tachiyomi_source_to_kani_name, tachiyomi_sync_id_to_tracker_name};

include!(concat!(env!("OUT_DIR"), "/tachiyomi.rs"));

#[derive(Debug, Serialize)]
pub struct TachiyomiPreview {
    pub total_manga: u32,
    pub category_count: u32,
    pub has_tracking: bool,
    pub has_chapter_progress: bool,
    /// Chapters the backup carries. Zero alongside a non-zero manga count means
    /// the file did not decode the way this proto expects, which is otherwise
    /// indistinguishable from a library nobody has read.
    pub chapter_count: u32,
    /// Chapters carrying a read flag or a page position.
    pub progress_entry_count: u32,
    /// Tracker links the backup carries. Reading status is derived from these,
    /// so a library with no tracker linked has none to import.
    pub tracking_entry_count: u32,
    pub sources: Vec<TachiyomiSourceSummary>,
    pub pending_import_estimate: u32,
}

#[derive(Debug, Serialize)]
pub struct TachiyomiSourceSummary {
    pub source_id: i64,
    pub source_name: String,
    pub manga_count: u32,
    pub found: bool,
}

#[derive(Debug, Deserialize)]
pub struct TachiyomiImportOptions {
    pub import_manga: bool,
    pub import_categories: bool,
    pub import_tracking: bool,
    pub import_chapter_progress: bool,
}

impl Default for TachiyomiImportOptions {
    fn default() -> Self {
        Self {
            import_manga: true,
            import_categories: true,
            import_tracking: true,
            import_chapter_progress: false,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TachiyomiImportResult {
    pub imported_manga: u32,
    /// Read-progress entries whose chapter was not found in the library.
    pub unmatched_progress: u32,
    pub skipped_manga: u32,
    pub trashed_manga: u32,
    pub imported_categories: u32,
    pub pending_imports_added: u32,
    pub possible_duplicates: u32,
    pub warnings: Vec<String>,
}

/// The name a backup gives each source it references, by Mihon source id.
fn backup_source_names(backup: &Backup) -> std::collections::HashMap<i64, String> {
    backup
        .backup_sources
        .iter()
        .filter(|s| !s.name.trim().is_empty())
        .map(|s| (s.source_id, s.name.clone()))
        .collect()
}

/// Resolve a Mihon/Tachiyomi source ID to a Kani source.
///
/// Tries `sources.mihon_source_id` (declared by an extension), then the
/// hardcoded map in `tachiyomi_sources.rs`, then the name the backup itself
/// carries — an installed source of the same name is the same source, which
/// spares the table an entry per extension that ever ships.
///
/// Returns `(Option<kani_source_id>, Option<display_name>)`; the name is
/// returned even when nothing matched, so warnings can say what was missing.
async fn resolve_kani_source(
    db: &sqlx::SqlitePool,
    mihon_id: i64,
    backup_name: Option<&str>,
) -> Result<(Option<i64>, Option<String>)> {
    let by_mihon = sqlx::query!(
        "SELECT id, name FROM sources WHERE mihon_source_id = ? AND deleted_at IS NULL",
        mihon_id
    )
    .fetch_optional(db)
    .await?;

    if let Some(row) = by_mihon {
        return Ok((Some(row.id), Some(row.name)));
    }

    let mapped = tachiyomi_source_to_kani_name(mihon_id);
    if let Some(name) = mapped {
        let id = sqlx::query_scalar!(
            "SELECT id FROM sources WHERE name = ? AND deleted_at IS NULL",
            name
        )
        .fetch_optional(db)
        .await?;
        if let Some(id) = id {
            return Ok((Some(id), Some(name.to_string())));
        }
    }

    let backup_name = backup_name.map(str::trim).filter(|n| !n.is_empty());
    if let Some(name) = backup_name {
        let by_name: Option<(i64, String)> = sqlx::query_as(
            "SELECT id, name FROM sources WHERE name = ? COLLATE NOCASE AND deleted_at IS NULL",
        )
        .bind(name)
        .fetch_optional(db)
        .await?;
        if let Some((id, installed_name)) = by_name {
            return Ok((Some(id), Some(installed_name)));
        }
    }

    // Nothing installed matches; name it as helpfully as we can so the warning
    // says which source is missing rather than "Unknown".
    Ok((
        None,
        mapped
            .map(str::to_string)
            .or_else(|| backup_name.map(str::to_string)),
    ))
}

fn decode_backup(data: &[u8]) -> Result<Backup> {
    let mut gz = GzDecoder::new(data);
    let mut buf = Vec::new();
    gz.read_to_end(&mut buf)
        .map_err(|e| ServiceError::Validation(format!("Failed to decompress .tachibk: {e}")))?;

    Backup::decode(buf.as_slice())
        .map_err(|e| ServiceError::Validation(format!("Failed to decode Tachiyomi proto: {e}")))
}

/// Map Tachiyomi manga publication status (BackupManga.status) to Kani's manga.status.
fn map_publication_status(tachi_status: i32) -> i64 {
    const TACHI_ONGOING: i32 = 1;
    const TACHI_COMPLETED: i32 = 2;
    const KANI_UNKNOWN: i64 = 0;
    const KANI_ONGOING: i64 = 1;
    const KANI_COMPLETED: i64 = 2;

    match tachi_status {
        TACHI_ONGOING => KANI_ONGOING,
        TACHI_COMPLETED => KANI_COMPLETED,
        _ => KANI_UNKNOWN,
    }
}

/// Map Tachiyomi tracker reading status (BackupTracking.status) to Kani's user_manga_tracking.status.
/// Tachiyomi: 1=Reading, 2=Completed, 3=OnHold, 4=Dropped, 5=PlanToRead, 6=Rereading
/// Kani:      0=Reading, 4=Completed, 1=OnHold, 2=Dropped, 3=PlanToRead, 5=Rereading
fn map_reading_status(tachi_status: i32) -> i64 {
    match tachi_status {
        1 => 0,
        2 => 4,
        3 => 1,
        4 => 2,
        5 => 3,
        6 => 5,
        _ => 0,
    }
}

impl AppService {
    pub async fn preview_tachiyomi_backup(&self, data: &[u8]) -> Result<TachiyomiPreview> {
        let backup = decode_backup(data)?;

        let mut source_counts: std::collections::HashMap<i64, u32> = Default::default();
        let mut pending_estimate: u32 = 0;
        let mut tracking_entry_count: u32 = 0;
        let mut chapter_count: u32 = 0;
        let mut progress_entry_count: u32 = 0;

        for m in &backup.backup_manga {
            *source_counts.entry(m.source).or_insert(0) += 1;
            tracking_entry_count += m.tracking.len() as u32;
            chapter_count += m.chapters.len() as u32;
            progress_entry_count += m
                .chapters
                .iter()
                .filter(|c| c.read || c.last_page_read > 0)
                .count() as u32;
        }
        let has_chapter_progress = progress_entry_count > 0;
        let has_tracking = tracking_entry_count > 0;

        let names = backup_source_names(&backup);
        let mut sources = Vec::new();
        for (&source_id, &count) in &source_counts {
            let (kani_db_id, display_name) = resolve_kani_source(
                &self.db,
                source_id,
                names.get(&source_id).map(String::as_str),
            )
            .await?;
            let found = kani_db_id.is_some();

            if !found {
                pending_estimate += count;
            }

            sources.push(TachiyomiSourceSummary {
                source_id,
                source_name: display_name.unwrap_or_else(|| "Unknown".to_string()),
                manga_count: count,
                found,
            });
        }

        sources.sort_by_key(|b| std::cmp::Reverse(b.manga_count));

        Ok(TachiyomiPreview {
            total_manga: backup.backup_manga.len() as u32,
            category_count: backup.backup_categories.len() as u32,
            has_tracking,
            has_chapter_progress,
            chapter_count,
            progress_entry_count,
            tracking_entry_count,
            sources,
            pending_import_estimate: pending_estimate,
        })
    }

    pub async fn import_tachiyomi_backup(
        &self,
        user_id: UserId,
        data: &[u8],
        opts: TachiyomiImportOptions,
    ) -> Result<TachiyomiImportResult> {
        let backup = decode_backup(data)?;

        let mut result = TachiyomiImportResult {
            imported_manga: 0,
            unmatched_progress: 0,
            skipped_manga: 0,
            trashed_manga: 0,
            imported_categories: 0,
            pending_imports_added: 0,
            possible_duplicates: 0,
            warnings: vec![],
        };

        let mut tachi_cat_map: std::collections::HashMap<i32, i64> = Default::default();

        if opts.import_categories {
            for (idx, cat) in backup.backup_categories.iter().enumerate() {
                sqlx::query!(
                    "INSERT OR IGNORE INTO categories (name, sort_order) VALUES (?, ?)",
                    cat.name,
                    cat.order
                )
                .execute(&self.db)
                .await?;
                result.imported_categories += 1;

                let cat_id: Option<i64> =
                    sqlx::query_scalar!("SELECT id FROM categories WHERE name = ?", cat.name)
                        .fetch_optional(&self.db_read)
                        .await?;

                if let Some(id) = cat_id {
                    tachi_cat_map.insert(idx as i32, id);
                }
            }
        }

        let mut new_manga_ids: Vec<i64> = Vec::new();
        // One job per source so each batch runs under that source's own
        // concurrency and rate limits.
        let mut resolve_sources: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let names = backup_source_names(&backup);

        let total_manga = if opts.import_manga {
            backup.backup_manga.len() as u32
        } else {
            0
        };
        if total_manga > 0 {
            let _ = self.refresh_tx.send(AppEvent::ImportStarted {
                origin: "tachiyomi".into(),
                total: total_manga,
            });
        }
        let mut title_index: std::collections::HashMap<
            i64,
            std::collections::HashMap<String, Vec<i64>>,
        > = Default::default();

        for (processed, m) in (1_u32..).zip(backup.backup_manga.iter()) {
            let (resolved_id, display_name) =
                resolve_kani_source(&self.db, m.source, names.get(&m.source).map(String::as_str))
                    .await?;

            let source_id = match resolved_id {
                Some(id) => id,
                None => {
                    if !opts.import_manga {
                        result.skipped_manga += 1;
                        continue;
                    }
                    let kani_hint = display_name.as_deref().unwrap_or("Unknown");
                    result.warnings.push(format!(
                        "Source '{}' (Tachiyomi ID {}) not installed — '{}' saved to pending imports",
                        kani_hint, m.source, m.title
                    ));
                    let proxy = self.make_tachi_backup_manga(m);
                    let source_hint =
                        display_name.unwrap_or_else(|| format!("Tachiyomi:{}", m.source));
                    self.save_pending_import_tachiyomi(user_id, &proxy, &source_hint, None, None)
                        .await?;
                    result.pending_imports_added += 1;
                    result.skipped_manga += 1;
                    let _ = self.refresh_tx.send(AppEvent::ImportProgress {
                        origin: "tachiyomi".into(),
                        completed: processed,
                        total: total_manga,
                        title: m.title.clone(),
                    });
                    continue;
                }
            };

            let mut existing_id: Option<i64> = sqlx::query_scalar!(
                "SELECT id FROM manga WHERE source_id = ? AND source_manga_id = ?",
                source_id,
                m.url
            )
            .fetch_optional(&self.db_read)
            .await?;

            if existing_id.is_none() {
                use std::collections::hash_map::Entry;
                let index = match title_index.entry(source_id) {
                    Entry::Occupied(e) => e.into_mut(),
                    Entry::Vacant(e) => e.insert(
                        crate::service::dedup::source_title_index(&self.db_read, source_id).await?,
                    ),
                };
                // An ambiguous title matches nothing: attaching a backup's
                // progress to the wrong series is worse than a duplicate.
                existing_id = index
                    .get(&crate::service::dedup::normalise_title(&m.title))
                    .filter(|ids| ids.len() == 1)
                    .map(|ids| ids[0]);
            }

            if let Some(id) = existing_id {
                let trashed: Option<i64> = sqlx::query_scalar!(
                    "SELECT id FROM manga WHERE id = ? AND deleted_at IS NOT NULL",
                    id
                )
                .fetch_optional(&self.db_read)
                .await?;
                if trashed.is_some() {
                    result.trashed_manga += 1;
                    let _ = self.refresh_tx.send(AppEvent::ImportProgress {
                        origin: "tachiyomi".into(),
                        completed: processed,
                        total: total_manga,
                        title: m.title.clone(),
                    });
                    continue;
                }
            }

            // Without `import_manga` the run may only touch what is already in
            // the library, so an entry with nothing to attach to is skipped
            // rather than added under another option's name.
            if existing_id.is_none() && !opts.import_manga {
                result.skipped_manga += 1;
                let _ = self.refresh_tx.send(AppEvent::ImportProgress {
                    origin: "tachiyomi".into(),
                    completed: processed,
                    total: total_manga,
                    title: m.title.clone(),
                });
                continue;
            }

            let (manga_id, is_new) = if let Some(id) = existing_id {
                (id, false)
            } else {
                let authors = if m.author.is_empty() {
                    vec![]
                } else {
                    vec![m.author.clone()]
                };
                let hits =
                    crate::service::dedup::find_similar_manga(&self.db, &m.title, &authors, None)
                        .await?;

                if !hits.is_empty() {
                    let proxy = self.make_tachi_backup_manga(m);
                    let source_hint = display_name.as_deref().unwrap_or("Unknown").to_string();
                    self.save_pending_import_tachiyomi(
                        user_id,
                        &proxy,
                        &source_hint,
                        Some(hits[0].id),
                        Some(hits[0].similarity),
                    )
                    .await?;
                    result.possible_duplicates += 1;
                    result.pending_imports_added += 1;
                    let _ = self.refresh_tx.send(AppEvent::ImportProgress {
                        origin: "tachiyomi".into(),
                        completed: processed,
                        total: total_manga,
                        title: m.title.clone(),
                    });
                    continue;
                }

                let mut tx = self.db.begin().await?;

                let status = map_publication_status(m.status);
                let description = if m.description.is_empty() {
                    None
                } else {
                    Some(&m.description)
                };
                let cover_url = if m.thumbnail_url.is_empty() {
                    None
                } else {
                    Some(&m.thumbnail_url)
                };

                let id = sqlx::query_scalar!(
                    "INSERT INTO manga (source_id, source_manga_id, name, description, cover_url, status) \
                     VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
                    source_id,
                    m.url,
                    m.title,
                    description,
                    cover_url,
                    status
                )
                .fetch_one(&mut *tx)
                .await?;

                sqlx::query!(
                    "INSERT OR REPLACE INTO manga_import_links (manga_id, status) \
                     VALUES (?, 'pending')",
                    id
                )
                .execute(&mut *tx)
                .await?;

                for &cat_idx in &m.categories {
                    if let Some(&cat_id) = tachi_cat_map.get(&cat_idx) {
                        sqlx::query!(
                            "INSERT OR IGNORE INTO manga_categories (manga_id, category_id) \
                             VALUES (?, ?)",
                            id,
                            cat_id
                        )
                        .execute(&mut *tx)
                        .await?;
                    }
                }

                for (name, role) in [(&m.author, "author"), (&m.artist, "artist")] {
                    if name.is_empty() {
                        continue;
                    }
                    sqlx::query!("INSERT OR IGNORE INTO people (name) VALUES (?)", name)
                        .execute(&mut *tx)
                        .await?;
                    sqlx::query!(
                        "INSERT OR IGNORE INTO manga_people (manga_id, role, person_id) \
                         SELECT ?, ?, id FROM people WHERE name = ?",
                        id,
                        role,
                        name
                    )
                    .execute(&mut *tx)
                    .await?;
                }

                for genre in &m.genre {
                    if genre.is_empty() {
                        continue;
                    }
                    sqlx::query!("INSERT OR IGNORE INTO tags (name) VALUES (?)", genre)
                        .execute(&mut *tx)
                        .await?;
                    sqlx::query!(
                        "INSERT OR IGNORE INTO manga_tags (manga_id, tag_id) \
                         SELECT ?, id FROM tags WHERE name = ?",
                        id,
                        genre
                    )
                    .execute(&mut *tx)
                    .await?;
                }

                tx.commit().await?;
                (id, true)
            };

            if opts.import_tracking
                && let Some(t) = m.tracking.first()
            {
                let kani_status = map_reading_status(t.status);
                let score: Option<f64> = if t.score > 0.0 {
                    Some(t.score as f64)
                } else {
                    None
                };
                // Updated rather than replaced: REPLACE would reset every column the
                // backup does not carry, including this user's reader settings.
                sqlx::query!(
                    "INSERT INTO user_manga_tracking \
                         (user_id, manga_id, status, score) VALUES (?, ?, ?, ?) \
                         ON CONFLICT(user_id, manga_id) DO UPDATE SET \
                         status = excluded.status, score = excluded.score",
                    user_id,
                    manga_id,
                    kani_status,
                    score
                )
                .execute(&self.db)
                .await?;
            }

            for t in &m.tracking {
                let Some(tracker_name) = tachiyomi_sync_id_to_tracker_name(t.sync_id) else {
                    continue;
                };

                let remote_id_val = if t.media_id != 0 {
                    t.media_id
                } else {
                    t.media_id_int as i64
                };

                let remote_id = if remote_id_val != 0 {
                    remote_id_val.to_string()
                } else {
                    // Fall back to last path segment of tracking_url (renamed from track_url)
                    t.tracking_url
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .filter(|s| s.chars().all(|c| c.is_ascii_digit()))
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                };

                if remote_id.is_empty() {
                    continue;
                }

                sqlx::query!(
                    "INSERT OR IGNORE INTO trackers (name) VALUES (?)",
                    tracker_name
                )
                .execute(&self.db)
                .await?;

                let tracker_id: Option<i64> =
                    sqlx::query_scalar!("SELECT id FROM trackers WHERE name = ?", tracker_name)
                        .fetch_optional(&self.db_read)
                        .await?
                        .flatten();

                if let Some(tid) = tracker_id {
                    sqlx::query!(
                        "INSERT OR IGNORE INTO tracker_manga_mappings \
                         (user_id, tracker_id, manga_id, tracker_manga_id) VALUES (?, ?, ?, ?)",
                        user_id,
                        tid,
                        manga_id,
                        remote_id
                    )
                    .execute(&self.db)
                    .await?;
                }
            }

            let reading_dir = if m.viewer_flags != 0 {
                match m.viewer_flags & 0x07 {
                    1 => Some("ltr"),
                    2 => Some("rtl"),
                    _ => None,
                }
            } else {
                match m.viewer {
                    1 => Some("ltr"),
                    2 => Some("rtl"),
                    _ => None,
                }
            };
            if let Some(dir) = reading_dir {
                sqlx::query!(
                    "INSERT INTO user_manga_tracking (user_id, manga_id, reading_direction) \
                     VALUES (?, ?, ?) \
                     ON CONFLICT (user_id, manga_id) DO UPDATE SET reading_direction = excluded.reading_direction",
                    user_id, manga_id, dir
                )
                .execute(&self.db)
                .await?;
            }

            // Search indexing and a local cover. Chapters wait for the resolve
            // job: the backup's id addresses nothing on this source, so asking
            // for them now only produces extraction errors.
            if is_new {
                let cover = (!m.thumbnail_url.is_empty()).then_some(m.thumbnail_url.as_str());
                self.after_manga_added(
                    MangaId(manga_id),
                    source_id,
                    &m.url,
                    &m.title,
                    cover,
                    false,
                )
                .await;
                resolve_sources.insert(source_id);
            }

            if opts.import_chapter_progress {
                let entries: Vec<ImportedProgress> = m
                    .chapters
                    .iter()
                    .filter(|ch| ch.read || ch.last_page_read != 0)
                    .map(|ch| ImportedProgress {
                        source_chapter_id: ch.url.clone(),
                        chapter_number: f64::from(ch.chapter_number),
                        is_read: ch.read,
                        last_page_read: i64::from(ch.last_page_read),
                    })
                    .collect();

                // A manga still carrying a link row is owed a chapter list, so
                // its progress waits for one instead of matching nothing. A
                // re-import is then the repair action for one that gave up.
                let awaiting_link: Option<String> = if is_new {
                    Some("pending".to_string())
                } else {
                    sqlx::query_scalar!(
                        "SELECT status FROM manga_import_links WHERE manga_id = ?",
                        manga_id
                    )
                    .fetch_optional(&self.db_read)
                    .await?
                };

                if let Some(status) = awaiting_link {
                    self.store_import_progress(MangaId(manga_id), user_id, &entries)
                        .await?;
                    if status != "pending" {
                        sqlx::query!(
                            "INSERT OR REPLACE INTO manga_import_links (manga_id, status) \
                             VALUES (?, 'pending')",
                            manga_id
                        )
                        .execute(&self.db)
                        .await?;
                        resolve_sources.insert(source_id);
                    }
                } else {
                    let mut unmatched = 0u32;
                    for e in &entries {
                        let applied = self
                            .apply_one_progress(
                                MangaId(manga_id),
                                user_id,
                                &e.source_chapter_id,
                                e.chapter_number,
                                e.is_read,
                                e.last_page_read,
                            )
                            .await?;
                        if !applied {
                            unmatched += 1;
                        }
                    }
                    if unmatched > 0 {
                        result.unmatched_progress += unmatched;
                        result.warnings.push(format!(
                            "Read progress for '{}': {unmatched} entries matched none of its chapters in the library.",
                            m.title
                        ));
                    }
                }
            }

            new_manga_ids.push(manga_id);
            result.imported_manga += 1;
            let _ = self.refresh_tx.send(AppEvent::ImportProgress {
                origin: "tachiyomi".into(),
                completed: processed,
                total: total_manga,
                title: m.title.clone(),
            });
        }

        if total_manga > 0 {
            let _ = self.refresh_tx.send(AppEvent::ImportCompleted {
                origin: "tachiyomi".into(),
                imported: result.imported_manga,
                skipped: result.skipped_manga,
                pending: result.pending_imports_added,
            });
        }

        if !new_manga_ids.is_empty() {
            let job = crate::jobs::import_dedup::ImportDedupJob::new(new_manga_ids);
            if let Err(e) = self.job_manager.submit(job).await {
                tracing::warn!("Failed to submit import dedup job: {e}");
            }
        }

        for source_id in resolve_sources {
            if let Err(e) = self.queue_import_resolve(Some(source_id)).await {
                tracing::warn!("Failed to submit import resolve job: {e}");
            }
        }

        self.cache.invalidate_stats(user_id);
        self.audit(
            Some(user_id),
            "import.tachiyomi",
            None,
            Some(serde_json::json!({
                "imported": result.imported_manga,
                "skipped": result.skipped_manga,
                "pending": result.pending_imports_added,
            })),
        )
        .await;

        Ok(result)
    }

    fn make_tachi_backup_manga(&self, m: &BackupManga) -> KaniBackupManga {
        let chapter_progress: Vec<BackupChapterProgress> = m
            .chapters
            .iter()
            .filter(|c| c.read || c.last_page_read > 0)
            .map(|c| BackupChapterProgress {
                source_chapter_id: c.url.clone(),
                is_read: c.read,
                last_page_read: c.last_page_read as i64,
            })
            .collect();

        let tracking = m.tracking.first().map(|t| BackupMangaTracking {
            status: map_reading_status(t.status),
            score: if t.score > 0.0 {
                Some(t.score as f64)
            } else {
                None
            },
        });

        KaniBackupManga {
            source_name: SourceId(String::new()),
            source_manga_id: m.url.clone(),
            name: m.title.clone(),
            status: Some(map_publication_status(m.status)),
            auto_download: false,
            auto_scan: false,
            scanlator_mode: String::new(),
            categories: vec![],
            tracking,
            download_rules: vec![],
            chapter_progress,
        }
    }

    async fn save_pending_import_tachiyomi(
        &self,
        user_id: UserId,
        m: &KaniBackupManga,
        source_hint: &str,
        duplicate_of: Option<MangaId>,
        similarity: Option<f64>,
    ) -> Result<()> {
        let tracking = m
            .tracking
            .as_ref()
            .and_then(|t| serde_json::to_string(t).ok());
        let chapter_progress = if m.chapter_progress.is_empty() {
            None
        } else {
            serde_json::to_string(&m.chapter_progress).ok()
        };

        sqlx::query!(
            "INSERT INTO pending_imports \
             (user_id, origin, title, source_hint, source_manga_id, tracking, chapter_progress, \
              possible_duplicate_of, duplicate_similarity) \
             VALUES (?, 'tachiyomi', ?, ?, ?, ?, ?, ?, ?)",
            user_id,
            m.name,
            source_hint,
            m.source_manga_id,
            tracking,
            chapter_progress,
            duplicate_of,
            similarity
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }
}
