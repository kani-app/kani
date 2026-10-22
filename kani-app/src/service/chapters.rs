use super::*;
use crate::ids::{ChapterId, MangaId, UserId};

/// Ceiling on chapter-listing pagination. A long series runs to a few dozen
/// pages; a source claiming more than this is malfunctioning, and following it
/// grows memory and the database without bound.
const MAX_CHAPTER_LIST_PAGES: usize = 500;

/// Resolved metadata for a downloaded chapter, including its on-disk CBZ path.
/// Returned by [`AppService::chapter_cbz_path`]; all callers get everything
/// they need from one query instead of issuing follow-up round-trips.
pub struct ChapterCbzInfo {
    pub path: std::path::PathBuf,
    pub chapter_title: String,
    pub manga_id: MangaId,
    pub manga_name: String,
    pub chapter_number: f64,
    pub scanlator: Option<String>,
    pub source_name: String,
}

impl AppService {
    /// Returns a paginated chapter list for a manga.
    /// Returns (chapters, has_next_page, total_pages, total_count).
    #[allow(clippy::too_many_arguments)]
    pub async fn get_local_chapters(
        &self,
        manga_id: MangaId,
        page: i32,
        page_size: i32,
        sort_order: kani_shared::types::ChapterSortOrder,
        user_id: UserId,
        filter_downloaded: Option<bool>,
        filter_unread: Option<bool>,
        filter_scanlator: Option<String>,
        filter_orphaned: Option<bool>,
    ) -> Result<(Vec<kani_shared::types::Chapter>, bool, Option<u32>, u32)> {
        let limit = (page_size as i64) + 1;
        let offset = ((page - 1).max(0) as i64) * (page_size as i64);

        let mut extra = String::new();
        match filter_downloaded {
            Some(true) => extra.push_str(" AND c.download_status = 2"),
            Some(false) => extra.push_str(" AND c.download_status != 2"),
            None => {}
        }
        if filter_unread == Some(true) {
            extra.push_str(" AND (uct.is_read IS NULL OR uct.is_read = 0)");
        }
        // Chapters kept by a migration's "keep downloaded chapters" are hidden
        // unless asked for: they no longer exist on the series' current source,
        // so they are an archive rather than part of the listing.
        extra.push_str(if filter_orphaned == Some(true) {
            " AND c.is_orphaned = true"
        } else {
            " AND c.is_orphaned = false"
        });

        let sql = format!(
            r#"SELECT c.id, c.source_chapter_id, c.name, c.chapter_number, c.language,
                      c.volume, c.scanlator, c.uploaded_at, c.download_status, c.is_orphaned,
                      c.page_count, c.download_error, c.upgrade_available,
                      uct.is_read, uct.last_page_read
               FROM chapters c
               LEFT JOIN user_chapter_tracking uct
                   ON uct.chapter_id = c.id AND uct.user_id = ?
               WHERE c.manga_id = ?{extra}
                 AND (? IS NULL OR c.scanlator = ?)
               ORDER BY {}, COALESCE((
                   SELECT sp.priority FROM scanlator_preferences sp
                   WHERE (sp.manga_id = c.manga_id OR sp.manga_id IS NULL)
                     AND (c.scanlator = sp.scanlator
                          OR (c.scanlator IS NULL AND sp.scanlator = 'Unknown'))
                   ORDER BY sp.manga_id IS NULL LIMIT 1), -1) DESC
               LIMIT ? OFFSET ?"#,
            sort_order.to_sql_order()
        );

        let scanlator_for_count = filter_scanlator.clone();
        let mut rows = sqlx::query_as::<_, crate::models::ChapterRow>(&sql)
            .bind(user_id)
            .bind(manga_id)
            .bind(filter_scanlator.clone())
            .bind(filter_scanlator)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db_read)
            .await?;

        let has_next_page = rows.len() == limit as usize;
        if has_next_page {
            rows.pop();
        }

        let chapters = rows
            .into_iter()
            .map(|c| kani_shared::types::Chapter {
                id: c.id.to_string(),
                title: c.name,
                number: c.chapter_number,
                volume: c.volume,
                language: c.language,
                scanlator: c.scanlator,
                date_uploaded: c.uploaded_at,
                download_status: c.download_status,
                is_orphaned: c.is_orphaned,
                page_count: c.page_count,
                is_read: c.is_read.unwrap_or(false),
                last_page_read: c.last_page_read,
                download_error: c.download_error.and_then(|s| serde_json::from_str(&s).ok()),
                upgrade_available: c
                    .upgrade_available
                    .and_then(|s| serde_json::from_str(&s).ok()),
            })
            .collect();

        let count_sql = format!(
            "SELECT COUNT(*) FROM chapters c \
             LEFT JOIN user_chapter_tracking uct ON uct.chapter_id = c.id AND uct.user_id = ? \
             WHERE c.manga_id = ?{extra} AND (? IS NULL OR c.scanlator = ?)"
        );
        let total_count: i64 = sqlx::query_scalar(&count_sql)
            .bind(user_id)
            .bind(manga_id)
            .bind(scanlator_for_count.clone())
            .bind(scanlator_for_count)
            .fetch_one(&self.db_read)
            .await?;
        let ps = page_size as i64;
        let total_pages = Some(((total_count + ps - 1) / ps).max(0) as u32);

        Ok((
            chapters,
            has_next_page,
            total_pages,
            total_count.max(0) as u32,
        ))
    }

    /// Returns all chapter IDs for a manga matching the given filters (no pagination).
    /// When `preferred_only` is true, applies scanlator preferences and download rules
    /// to return one preferred version per chapter number (undownloaded chapters only).
    #[allow(clippy::too_many_arguments)]
    pub async fn get_chapter_ids(
        &self,
        manga_id: MangaId,
        user_id: UserId,
        sort_order: kani_shared::types::ChapterSortOrder,
        filter_downloaded: Option<bool>,
        filter_unread: Option<bool>,
        filter_scanlator: Option<String>,
        preferred_only: bool,
        filter_orphaned: Option<bool>,
    ) -> Result<Vec<ChapterId>> {
        // When preferred_only is set, restrict to undownloaded chapters and
        // apply filter_chapters_by_rules afterwards.
        let effective_filter_downloaded = if preferred_only {
            Some(false)
        } else {
            filter_downloaded
        };

        let mut extra = String::new();
        match effective_filter_downloaded {
            Some(true) => extra.push_str(" AND c.download_status = 2"),
            Some(false) => extra.push_str(" AND c.download_status != 2"),
            None => {}
        }
        if filter_unread == Some(true) {
            extra.push_str(" AND (uct.is_read IS NULL OR uct.is_read = 0)");
        }
        // Opt-in, like the listing. Bulk download and preferred-version
        // selection pass None, so they never reach for a chapter the current
        // source does not carry; only "select orphaned" asks for them.
        extra.push_str(if filter_orphaned == Some(true) {
            " AND c.is_orphaned = true"
        } else {
            " AND c.is_orphaned = false"
        });

        let sql = format!(
            r#"SELECT c.id
               FROM chapters c
               LEFT JOIN user_chapter_tracking uct
                   ON uct.chapter_id = c.id AND uct.user_id = ?
               WHERE c.manga_id = ?{extra}
                 AND (? IS NULL OR c.scanlator = ?)
               ORDER BY {}, COALESCE((
                   SELECT sp.priority FROM scanlator_preferences sp
                   WHERE (sp.manga_id = c.manga_id OR sp.manga_id IS NULL)
                     AND (c.scanlator = sp.scanlator
                          OR (c.scanlator IS NULL AND sp.scanlator = 'Unknown'))
                   ORDER BY sp.manga_id IS NULL LIMIT 1), -1) DESC"#,
            sort_order.to_sql_order()
        );

        let ids: Vec<ChapterId> = sqlx::query_scalar::<_, i64>(&sql)
            .bind(user_id)
            .bind(manga_id)
            .bind(filter_scanlator.clone())
            .bind(filter_scanlator)
            .fetch_all(&self.db_read)
            .await?
            .into_iter()
            .map(ChapterId)
            .collect();

        if preferred_only {
            Ok(self.filter_chapters_by_rules(manga_id, ids).await)
        } else {
            Ok(ids)
        }
    }

    /// Resolves the on-disk CBZ path for a downloaded chapter together with all
    /// associated metadata in a single query. Returns an error if the chapter is
    /// not in downloaded state.
    pub async fn chapter_cbz_path(&self, chapter_id: ChapterId) -> Result<ChapterCbzInfo> {
        let rec = sqlx::query!(
            "SELECT c.download_status, c.volume, c.chapter_number, c.name, c.scanlator,
                    c.file_path,
                    m.id as manga_id, m.name as manga_name,
                    s.name as source_name
             FROM chapters c
             JOIN manga m ON c.manga_id = m.id
             JOIN sources s ON s.id = m.source_id
             WHERE c.id = ?",
            chapter_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Chapter {chapter_id} not found")))?;

        if rec.download_status != 2 {
            return Err(ServiceError::NotFound(format!(
                "Chapter {chapter_id} is not downloaded"
            )));
        }

        let chapter_title = chapter_name(rec.volume, rec.chapter_number, rec.name);
        let library_path = self.settings.read().await.library_path.clone();

        // A stored path wins over title derivation: renaming a manga must not
        // orphan its files. Rows predating the backfill fall through below.
        if let Some(rel) = rec.file_path.as_deref().filter(|p| !p.is_empty()) {
            let resolved =
                kani_core::utilities::assert_within_root(&library_path, &library_path.join(rel))
                    .map_err(|e| ServiceError::Internal(format!("Path traversal blocked: {e}")))?;

            return Ok(ChapterCbzInfo {
                path: resolved,
                chapter_title,
                manga_id: MangaId(rec.manga_id),
                manga_name: rec.manga_name,
                chapter_number: rec.chapter_number,
                scanlator: rec.scanlator,
                source_name: rec.source_name,
            });
        }

        let safe_manga_dir = format!(
            "{} - {}",
            kani_core::utilities::sanitize_filename(&rec.manga_name),
            rec.manga_id,
        );
        let safe_cbz = kani_core::utilities::assert_within_root(
            &library_path,
            &library_path.join(safe_manga_dir).join(format!(
                "{}.cbz",
                kani_core::utilities::sanitize_filename(&chapter_title)
            )),
        )
        .map_err(|e| ServiceError::Internal(format!("Path traversal blocked: {e}")))?;

        Ok(ChapterCbzInfo {
            path: safe_cbz,
            chapter_title,
            manga_id: MangaId(rec.manga_id),
            manga_name: rec.manga_name,
            chapter_number: rec.chapter_number,
            scanlator: rec.scanlator,
            source_name: rec.source_name,
        })
    }

    /// Returns the page manifest for a downloaded chapter, including adjacent
    /// chapter IDs for navigation.
    pub async fn get_chapter_page_manifest(
        &self,
        chapter_id: ChapterId,
        user_id: UserId,
    ) -> Result<crate::models::ChapterPageManifest> {
        let info = self.chapter_cbz_path(chapter_id).await?;
        let cbz_path = info.path;
        let chapter_title = info.chapter_title;
        let manga_id = info.manga_id;
        let manga_title = info.manga_name;
        let chapter_number = info.chapter_number;
        let scanlator = info.scanlator;
        let source_name = info.source_name;

        let path_clone = cbz_path.clone();
        // Read page list and double-page flags in a single blocking task so
        // we open the CBZ archive only once (the `zip` crate is sync).
        let (names, double_page_flags, spread_analysed) = tokio::task::spawn_blocking(move || {
            let names = kani_core::cbz::list_cbz_pages(&path_clone)?;
            let (flags, analysed) = kani_core::cbz::read_double_page_flags(&path_clone);
            Ok::<_, kani_core::error::Error>((names, flags, analysed))
        })
        .await
        .map_err(|e| ServiceError::Internal(format!("Task join error: {e}")))?
        .map_err(ServiceError::Core)?;

        let pages = names
            .into_iter()
            .enumerate()
            .map(|(index, filename)| crate::models::PageInfo {
                double_page: double_page_flags.contains(&index),
                index,
                filename,
            })
            .collect::<Vec<_>>();
        let page_count = pages.len();

        let db_clone = self.db.clone();
        let pc_i64 = page_count as i64;

        tokio::spawn(async move {
            let result = sqlx::query!(
                "UPDATE chapters SET page_count = ? WHERE id = ? AND page_count IS NULL",
                pc_i64,
                chapter_id
            )
            .execute(&db_clone)
            .await;

            if let Err(e) = result {
                tracing::warn!(
                    "Opportunistic page_count update failed for chapter {}: {}",
                    chapter_id,
                    e
                );
            }
        });

        let prev_chapter_id = self
            .adjacent_chapter_id(manga_id, chapter_number, false)
            .await?;
        let next_chapter_id = self
            .adjacent_chapter_id(manga_id, chapter_number, true)
            .await?;

        let last_page_read = self
            .get_chapter_progress(user_id, chapter_id)
            .await?
            .map(|(page, _)| page);

        let alt_rows = sqlx::query!(
            r#"SELECT id as "id: i64", scanlator, volume as "volume: i64"
               FROM chapters
               WHERE manga_id = ? AND chapter_number = ? AND id != ? AND download_status = 2
               ORDER BY scanlator, volume"#,
            manga_id,
            chapter_number,
            chapter_id,
        )
        .fetch_all(&self.db_read)
        .await?;

        let scanlator_alternatives = alt_rows
            .into_iter()
            .map(|r| crate::models::ScanlatorAlt {
                chapter_id: ChapterId(r.id),
                scanlator: r.scanlator,
                volume: r.volume,
            })
            .collect();

        Ok(crate::models::ChapterPageManifest {
            chapter_id,
            chapter_title,
            chapter_number,
            scanlator,
            source_name,
            manga_id,
            manga_title,
            page_count,
            pages,
            prev_chapter_id,
            next_chapter_id,
            last_page_read,
            spread_analysed,
            scanlator_alternatives,
        })
    }

    /// Returns the id of the nearest chapter before (`next=false`) or after
    /// (`next=true`) `chapter_number`, respecting scanlator preferences.
    /// Does not filter by download status — the reader handles downloading.
    async fn adjacent_chapter_id(
        &self,
        manga_id: MangaId,
        chapter_number: f64,
        next: bool,
    ) -> Result<Option<ChapterId>> {
        let scanlator_mode = self.get_scanlator_mode(manga_id).await?;
        let scanlator_filter = match scanlator_mode.as_str() {
            "whitelist" => {
                " AND EXISTS (SELECT 1 FROM scanlator_preferences sp WHERE sp.manga_id = c.manga_id AND sp.scanlator = c.scanlator)"
            }
            _ => {
                " AND NOT EXISTS (SELECT 1 FROM scanlator_preferences sp \
                   WHERE (sp.manga_id = c.manga_id OR sp.manga_id IS NULL) \
                     AND sp.scanlator = c.scanlator AND sp.blocked = 1)"
            }
        };
        let (cmp, order) = if next { (">", "ASC") } else { ("<", "DESC") };
        let sql = format!(
            "SELECT c.id FROM chapters c
             WHERE c.manga_id = ? AND c.chapter_number {cmp} ?{scanlator_filter}
             ORDER BY c.chapter_number {order}, COALESCE((
                 SELECT sp.priority FROM scanlator_preferences sp
                 WHERE (sp.manga_id = c.manga_id OR sp.manga_id IS NULL)
                   AND sp.scanlator = c.scanlator
                 ORDER BY sp.manga_id IS NULL LIMIT 1), -1) DESC LIMIT 1"
        );
        let id: Option<i64> = sqlx::query_scalar(&sql)
            .bind(manga_id)
            .bind(chapter_number)
            .fetch_optional(&self.db_read)
            .await?;
        Ok(id.map(ChapterId))
    }

    /// Reads a single page image from a downloaded chapter's CBZ archive.
    ///
    /// Returns the raw bytes and lowercase file extension (e.g. `"jpg"`).
    pub async fn read_chapter_page(
        &self,
        chapter_id: ChapterId,
        page_num: usize,
    ) -> Result<(Vec<u8>, String)> {
        let info = self.chapter_cbz_path(chapter_id).await?;
        tokio::task::spawn_blocking(move || kani_core::cbz::read_cbz_page(&info.path, page_num))
            .await
            .map_err(|e| ServiceError::Internal(format!("Task join error: {e}")))?
            .map_err(ServiceError::Core)
    }

    /// Every chapter the source lists, plus whether the page ceiling cut the
    /// listing short. A truncated listing must never be read as "these are all
    /// the chapters that exist" — callers that delete on absence have to know.
    pub(super) async fn fetch_all_chapter_pages_checked(
        &self,
        source_id: i64,
        source_manga_id: &str,
    ) -> Result<(Vec<wit_types::ChapterInfo>, bool)> {
        let backend = self
            .sources
            .get_backend(source_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Source {source_id} not found")))?;

        let mut all: Vec<wit_types::ChapterInfo> = Vec::new();
        let mut page = 1i32;
        loop {
            let res = backend
                .get_chapter_list(source_manga_id, page, None, None)
                .await?;
            let json = serde_json::to_string(&res)
                .map_err(|e| ServiceError::Core(kani_core::Error::Json(e)))?;
            let list: wit_types::ChapterList = serde_json::from_str(&json).map_err(|e| {
                ServiceError::Internal(format!("Failed to parse chapter list: {e}"))
            })?;
            let empty = list.chapters.is_empty();
            let more = list.has_next_page;
            all.extend(list.chapters);
            // A source that always answers `has_next_page: true` would spin here forever,
            // growing the vector without bound. This is reachable from a REST handler
            // (`preview_migration`), so the ceiling is not hypothetical.
            if empty || !more || page as usize >= MAX_CHAPTER_LIST_PAGES {
                let truncated = !empty && more && page as usize >= MAX_CHAPTER_LIST_PAGES;
                if truncated {
                    tracing::warn!(
                        "Chapter listing for source {source_id}/{source_manga_id} hit the \
                         {MAX_CHAPTER_LIST_PAGES}-page ceiling and is incomplete"
                    );
                }
                return Ok((all, truncated));
            }
            page += 1;
        }
    }
}
