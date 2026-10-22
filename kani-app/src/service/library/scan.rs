use super::super::*;
use crate::ids::MangaId;
use futures::stream::{FuturesUnordered, StreamExt};

/// Consecutive all-known listing pages tolerated before a scan concludes it has
/// caught up. More than one, because hitting a page of already-held chapters is
/// routine at a pagination boundary and says nothing about what follows.
const DEFAULT_BARREN_PAGE_TOLERANCE: usize = 3;

impl AppService {
    /// `force` skips the fuzzy duplicate check (used after the user confirms "add anyway").
    pub async fn save_to_library(
        &self,
        source_id: i64,
        manga_id: &str,
        force: bool,
    ) -> Result<MangaId> {
        let backend = self
            .sources
            .get_backend(source_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Source {source_id} not found")))?;

        let result = backend.get_manga_details(manga_id).await?;

        let manga = convert_to_shared_manga_info(result);

        if !force {
            let hits = crate::service::dedup::find_similar_manga(
                &self.db,
                &manga.title,
                &manga.authors,
                None,
            )
            .await?;
            if !hits.is_empty() {
                return Err(ServiceError::PossibleDuplicate(hits));
            }
        }

        let mut tx = self.db.begin().await?;
        let status: i64 = manga.status.into();

        let insert_result = sqlx::query!(
            "INSERT OR IGNORE INTO manga (source_manga_id, source_id, name, cover_url, description, status) \
             VALUES (?, ?, ?, ?, ?, ?)",
            manga.id,
            source_id,
            manga.title,
            manga.cover_url,
            manga.description,
            status
        )
        .execute(&mut *tx)
        .await?;

        let we_inserted = insert_result.rows_affected() == 1;

        let manga_row_id = MangaId(
            sqlx::query_scalar!(
                "SELECT id FROM manga WHERE source_manga_id = ? AND source_id = ?",
                manga.id,
                source_id
            )
            .fetch_one(&mut *tx)
            .await?,
        );

        if we_inserted {
            Self::sync_manga_metadata(&mut tx, manga_row_id, &manga).await?;
        }

        tx.commit().await?;

        if we_inserted {
            self.after_manga_added(
                manga_row_id,
                source_id,
                manga_id,
                &manga.title,
                manga.cover_url.as_deref(),
                true,
            )
            .await;
        }

        Ok(manga_row_id)
    }

    /// Everything a newly created manga row needs beyond the row itself:
    /// search indexing, a local cover, its chapter list, duplicate detection
    /// and the webhook.
    ///
    /// Shared with the importers so a manga that arrives from a backup is not
    /// half-added — unsearchable, coverless, and with no chapters to read.
    /// A failed chapter fetch is logged, not surfaced: the manga is still
    /// added and the next scan fills it in.
    pub(crate) async fn after_manga_added(
        &self,
        manga_row_id: MangaId,
        source_id: i64,
        source_manga_id: &str,
        title: &str,
        cover_url: Option<&str>,
        fetch_chapters: bool,
    ) {
        self.update_manga_fts(manga_row_id)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("FTS update failed for manga {manga_row_id}: {e}");
            });
        self.cache.invalidate_library();

        if let Some(url) = cover_url {
            let base_url =
                sqlx::query_scalar!("SELECT base_url FROM sources WHERE id = ?", source_id)
                    .fetch_optional(&self.db_read)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default();

            if let Err(e) = self
                .download_and_store_cover(manga_row_id, url, &base_url)
                .await
            {
                tracing::warn!(
                    "Failed to download cover for manga {}: {} — library entry still saved, scheduling retry",
                    manga_row_id,
                    e
                );
                self.schedule_cover_retry(manga_row_id).await;
            }
        }

        let has_next_page = if fetch_chapters {
            self.fetch_and_store_chapter_page(source_id, source_manga_id, manga_row_id, 1)
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to fetch initial chapters: {}", e);
                    false
                })
        } else {
            false
        };

        if has_next_page {
            let bg_self = self.clone();
            let bg_manga_id = source_manga_id.to_string();
            tokio::spawn(async move {
                bg_self
                    .fetch_and_store_remaining_chapters(source_id, bg_manga_id, manga_row_id, 2)
                    .await;
            });
        }

        let pool = self.db.clone();
        tokio::spawn(async move {
            if let Err(e) =
                crate::service::dedup::record_duplicates_for_manga(&pool, manga_row_id).await
            {
                tracing::warn!("Duplicate recording failed for manga {manga_row_id}: {e}");
            }
        });

        self.fire_webhooks(crate::service::webhooks::WebhookPayload::MangaAdded {
            manga_id: manga_row_id,
            manga_name: title.to_string(),
            source_id,
        })
        .await;
    }

    pub(super) async fn sync_manga_people(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        manga_row_id: MangaId,
        manga: &wit_types::MangaInfo,
    ) -> Result<()> {
        for author in &manga.authors {
            sqlx::query!("INSERT OR IGNORE INTO people (name) VALUES (?)", author)
                .execute(&mut **tx)
                .await?;
            sqlx::query!(
                "INSERT OR IGNORE INTO manga_people (manga_id, role, person_id) \
                SELECT ?, 'author', id FROM people WHERE name = ?",
                manga_row_id,
                author
            )
            .execute(&mut **tx)
            .await?;
        }

        for artist in &manga.artists {
            sqlx::query!("INSERT OR IGNORE INTO people (name) VALUES (?)", artist)
                .execute(&mut **tx)
                .await?;
            sqlx::query!(
                "INSERT OR IGNORE INTO manga_people (manga_id, role, person_id) \
                SELECT ?, 'artist', id FROM people WHERE name = ?",
                manga_row_id,
                artist
            )
            .execute(&mut **tx)
            .await?;
        }

        Ok(())
    }

    pub(super) async fn sync_manga_tags(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        manga_row_id: MangaId,
        manga: &wit_types::MangaInfo,
    ) -> Result<()> {
        for tag in &manga.tags {
            sqlx::query!("INSERT OR IGNORE INTO tags (name) VALUES (?)", tag)
                .execute(&mut **tx)
                .await?;
            sqlx::query!(
                "INSERT OR IGNORE INTO manga_tags (manga_id, tag_id) \
                SELECT ?, id FROM tags WHERE name = ?",
                manga_row_id,
                tag
            )
            .execute(&mut **tx)
            .await?;
        }

        Ok(())
    }

    pub(in crate::service) async fn sync_manga_metadata(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        manga_row_id: MangaId,
        manga: &wit_types::MangaInfo,
    ) -> Result<()> {
        Self::sync_manga_people(tx, manga_row_id, manga).await?;
        Self::sync_manga_tags(tx, manga_row_id, manga).await?;
        Ok(())
    }

    pub async fn refresh_manga(&self, manga_row_id: MangaId) -> Result<()> {
        self.refresh_manga_with_options(manga_row_id, crate::models::RefreshOptions::default())
            .await
    }

    pub async fn refresh_manga_with_options(
        &self,
        manga_row_id: MangaId,
        opts: crate::models::RefreshOptions,
    ) -> Result<()> {
        let ids = sqlx::query!(
            "SELECT source_id, source_manga_id, s.base_url as base_url, m.cover_overridden
            FROM manga m
            JOIN sources s ON m.source_id = s.id
            WHERE m.id = ?",
            manga_row_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Manga {manga_row_id} not found")))?;

        // Read current scalar fields so we can keep unselected ones unchanged.
        let current = sqlx::query_as!(
            crate::models::Manga,
            "SELECT * FROM manga WHERE id = ?",
            manga_row_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Manga {manga_row_id} not found")))?;

        let manga_info_raw = self
            .get_manga_details(ids.source_id, &ids.source_manga_id)
            .await?;
        let manga_info: wit_types::MangaInfo = serde_json::from_str(&manga_info_raw)
            .map_err(|e| ServiceError::Internal(format!("Failed to parse manga: {}", e)))?;

        let mut tx = self.db.begin().await?;

        let new_name: String = if opts.fields.title {
            manga_info.title.clone()
        } else {
            current.name.clone()
        };
        let new_cover_url: Option<String> = if opts.fields.cover {
            manga_info.cover_url.clone()
        } else {
            current.cover_url.clone()
        };
        let new_description: Option<String> = if opts.fields.description {
            manga_info.description.clone()
        } else {
            current.description.clone()
        };
        let new_status: i64 = if opts.fields.status {
            manga_info.status as i64
        } else {
            i64::from(current.status)
        };

        sqlx::query!(
            "UPDATE manga SET name = ?, cover_url = ?, description = ?, status = ? WHERE id = ?",
            new_name,
            new_cover_url,
            new_description,
            new_status,
            manga_row_id
        )
        .execute(&mut *tx)
        .await?;

        if opts.clear_overrides {
            if opts.fields.title {
                sqlx::query!(
                    "UPDATE manga SET local_name = NULL WHERE id = ?",
                    manga_row_id
                )
                .execute(&mut *tx)
                .await?;
            }
            if opts.fields.description {
                sqlx::query!(
                    "UPDATE manga SET local_description = NULL WHERE id = ?",
                    manga_row_id
                )
                .execute(&mut *tx)
                .await?;
            }
            if opts.fields.status {
                sqlx::query!(
                    "UPDATE manga SET local_status = NULL WHERE id = ?",
                    manga_row_id
                )
                .execute(&mut *tx)
                .await?;
            }
            if opts.fields.cover {
                sqlx::query!(
                    "UPDATE manga SET local_cover_path = NULL, cover_overridden = FALSE WHERE id = ?",
                    manga_row_id
                )
                .execute(&mut *tx)
                .await?;
            }
            if opts.fields.people {
                sqlx::query!(
                    "DELETE FROM manga_local_authors WHERE manga_id = ?",
                    manga_row_id
                )
                .execute(&mut *tx)
                .await?;
            }
            if opts.fields.tags {
                sqlx::query!(
                    "DELETE FROM manga_local_tags WHERE manga_id = ?",
                    manga_row_id
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        if opts.fields.people {
            sqlx::query!("DELETE FROM manga_people WHERE manga_id = ?", manga_row_id)
                .execute(&mut *tx)
                .await?;
            Self::sync_manga_people(&mut tx, manga_row_id, &manga_info).await?;
        }

        if opts.fields.tags {
            sqlx::query!("DELETE FROM manga_tags WHERE manga_id = ?", manga_row_id)
                .execute(&mut *tx)
                .await?;
            Self::sync_manga_tags(&mut tx, manga_row_id, &manga_info).await?;
        }

        tx.commit().await?;

        self.update_manga_fts(manga_row_id)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("FTS update failed for manga {manga_row_id}: {e}");
            });
        self.cache.invalidate_library();

        let cover_overridden_now = if opts.clear_overrides && opts.fields.cover {
            false
        } else {
            ids.cover_overridden
        };
        if opts.fields.cover
            && !cover_overridden_now
            && let Some(ref url) = manga_info.cover_url
            && let Err(e) = self
                .download_and_store_cover(manga_row_id, url, &ids.base_url)
                .await
        {
            tracing::warn!(
                "Failed to refresh cover for manga {}: {}, scheduling retry",
                manga_row_id,
                e
            );
            self.schedule_cover_retry(manga_row_id).await;
        }

        if opts.fetch_chapters {
            let has_next_page = self
                .fetch_and_store_chapter_page(ids.source_id, &ids.source_manga_id, manga_row_id, 1)
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to fetch initial chapters during refresh: {}", e);
                    false
                });

            self.cache
                .invalidate_chapter_list_for_manga(ids.source_id, &ids.source_manga_id)
                .await;

            if has_next_page {
                let bg_self = self.clone();
                let bg_manga_id = ids.source_manga_id.clone();
                tokio::spawn(async move {
                    bg_self
                        .fetch_and_store_remaining_chapters(
                            ids.source_id,
                            bg_manga_id,
                            manga_row_id,
                            2,
                        )
                        .await;
                });
            }
        }

        // Upgrade detection runs on freshly-upserted chapters. It is
        // metadata-only and must never fail a refresh, so a problem here is
        // logged rather than propagated.
        match self.evaluate_upgrades(manga_row_id).await {
            Ok(found) if !found.is_empty() => {
                let _ = self
                    .refresh_tx
                    .send(crate::events::AppEvent::UpgradesFound {
                        manga_id: manga_row_id.0,
                        count: found.len() as u64,
                    });
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("Upgrade evaluation failed for {manga_row_id}: {e}"),
        }

        Ok(())
    }

    pub(super) async fn insert_chapters_batch(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        manga_row_id: MangaId,
        chapters: &[wit_types::ChapterInfo],
    ) -> Result<Vec<i64>> {
        let mut ids = Vec::new();
        for chunk in chapters.chunks(100) {
            let mut qb = sqlx::QueryBuilder::new(
                "INSERT OR IGNORE INTO chapters \
                (manga_id, source_chapter_id, name, chapter_number, language, volume, scanlator, uploaded_at, source_page_count, discovered_at) ",
            );
            qb.push_values(chunk, |mut b, ch| {
                b.push_bind(manga_row_id)
                    .push_bind(ch.id.clone())
                    .push_bind(ch.title.clone())
                    .push_bind(ch.number)
                    .push_bind(ch.language.clone())
                    .push_bind(ch.volume)
                    .push_bind(ch.scanlator.clone())
                    .push_bind(ch.date_uploaded)
                    .push_bind(ch.page_count.map(i64::from));
                b.push("CURRENT_TIMESTAMP");
            });
            qb.push(" RETURNING id");
            let mut rows: Vec<i64> = qb.build_query_scalar().fetch_all(&mut **tx).await?;
            ids.append(&mut rows);
        }
        Ok(ids)
    }

    async fn refresh_source_page_counts(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        manga_row_id: MangaId,
        chapters: &[wit_types::ChapterInfo],
    ) -> Result<()> {
        for ch in chapters {
            let Some(count) = ch.page_count.map(i64::from) else {
                continue;
            };
            let source_chapter_id = &ch.id;
            sqlx::query!(
                "UPDATE chapters SET source_page_count = ? \
                 WHERE manga_id = ? AND source_chapter_id = ? \
                 AND (source_page_count IS NULL OR source_page_count != ?)",
                count,
                manga_row_id,
                source_chapter_id,
                count
            )
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }

    /// Fetches chapters from the source and stores them without broadcasting any SSE events.
    /// Used during bulk import to avoid spamming `NewChapters` notifications.
    /// Returns the IDs of newly inserted chapters.
    pub async fn fetch_and_store_chapters_silent(&self, manga_row_id: MangaId) -> Result<Vec<i64>> {
        self.fetch_and_store_chapters_impl(manga_row_id, false)
            .await
    }

    /// Core chapter fetch/store loop shared by the silent (bulk import) and streaming
    /// (scan jobs) callers. `emit_progress` gates `ChapterListPartial`/`Complete`/`Error`
    /// broadcasts so bulk import keeps its documented zero-SSE-events guarantee.
    async fn fetch_and_store_chapters_impl(
        &self,
        manga_row_id: MangaId,
        emit_progress: bool,
    ) -> Result<Vec<i64>> {
        let ids = sqlx::query!(
            "SELECT source_id, source_manga_id FROM manga WHERE id = ?",
            manga_row_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Manga {manga_row_id} not found")))?;

        let backend = self
            .sources
            .get_backend(ids.source_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Source {} not found", ids.source_id)))?;

        // Collect pages before taking the single write connection; see the engineering
        // constraints register.
        let mut known_ids: std::collections::HashSet<String> = sqlx::query_scalar!(
            "SELECT source_chapter_id FROM chapters WHERE manga_id = ?",
            manga_row_id
        )
        .fetch_all(&self.db_read)
        .await?
        .into_iter()
        .collect();

        let mut collected: Vec<wit_types::ChapterInfo> = Vec::new();
        let mut total_received = 0usize;
        let mut page = 1;
        let mut barren_pages = 0usize;
        let barren_tolerance =
            usize::try_from(self.settings.read().await.scan_barren_page_tolerance)
                .unwrap_or(DEFAULT_BARREN_PAGE_TOLERANCE)
                .max(1);

        loop {
            let started = std::time::Instant::now();
            let attempt = backend
                .get_chapter_list(&ids.source_manga_id, page, None, None)
                .await;
            self.record_source_call(ids.source_id, started, &attempt)
                .await;
            let res = match attempt {
                Ok(res) => res,
                Err(e) => {
                    if emit_progress {
                        let _ = self.refresh_tx.send(AppEvent::ChapterListError {
                            manga_id: manga_row_id,
                            error: e.to_string(),
                        });
                    }
                    return Err(ServiceError::Core(e));
                }
            };
            let json = serde_json::to_string(&res)
                .map_err(|e| ServiceError::Core(kani_core::Error::Json(e)))?;
            let chapter_list: wit_types::ChapterList =
                serde_json::from_str(&json).map_err(|e| {
                    ServiceError::Internal(format!("Failed to parse chapter list: {}", e))
                })?;

            if chapter_list.chapters.is_empty() {
                break;
            }

            let new_on_page = chapter_list
                .chapters
                .iter()
                .filter(|ch| known_ids.insert(ch.id.clone()))
                .count();

            total_received += chapter_list.chapters.len();
            collected.extend(chapter_list.chapters.iter().cloned());

            if emit_progress {
                let _ = self.refresh_tx.send(AppEvent::ChapterListPartial {
                    manga_id: manga_row_id,
                    received: total_received,
                });
            }

            // Stop only after the configured run of barren pages; see the engineering
            // constraints register.
            if new_on_page == 0 {
                barren_pages += 1;
            } else {
                barren_pages = 0;
            }
            if barren_pages >= barren_tolerance || !chapter_list.has_next_page {
                break;
            }

            page += 1;
        }

        // One short checkout of the single write connection, with no network in
        // scope. Still all-or-nothing: a page failure above returns before this
        // runs, so a partial listing is never half-stored.
        let mut tx = self.db.begin().await?;
        let new_chapter_ids = self
            .insert_chapters_batch(&mut tx, manga_row_id, &collected)
            .await?;

        // Refresh known rows because batch insertion intentionally ignores them.
        self.refresh_source_page_counts(&mut tx, manga_row_id, &collected)
            .await?;
        tx.commit().await?;

        if !new_chapter_ids.is_empty() {
            self.cache
                .invalidate_chapter_list_for_manga(ids.source_id, &ids.source_manga_id)
                .await;
        }

        if emit_progress {
            let _ = self.refresh_tx.send(AppEvent::ChapterListComplete {
                manga_id: manga_row_id,
                total: total_received,
            });
        }

        Ok(new_chapter_ids)
    }

    pub async fn scan_for_new_chapters(&self, manga_row_id: MangaId) -> Result<Vec<i64>> {
        let new_chapter_ids = self
            .fetch_and_store_chapters_impl(manga_row_id, true)
            .await?;

        if !new_chapter_ids.is_empty() {
            let manga_name =
                sqlx::query_scalar!("SELECT name FROM manga WHERE id = ?", manga_row_id)
                    .fetch_optional(&self.db_read)
                    .await
                    .unwrap_or(None)
                    .unwrap_or_default();

            struct ChRow {
                volume: Option<i64>,
                chapter_number: f64,
                name: Option<String>,
            }
            let ids_json = serde_json::to_string(&new_chapter_ids).unwrap_or_default();
            let chapter_rows: Vec<ChRow> = sqlx::query_as!(
                ChRow,
                "SELECT volume, chapter_number, name FROM chapters WHERE id IN (SELECT value FROM json_each(?))",
                ids_json
            )
            .fetch_all(&self.db_read)
            .await
            .unwrap_or_default();
            let chapter_names: Vec<String> = chapter_rows
                .into_iter()
                .map(|r| crate::service::chapter_name(r.volume, r.chapter_number, r.name))
                .collect();

            let _ = self.refresh_tx.send(AppEvent::NewChapters {
                manga_id: manga_row_id,
                manga_name,
                count: new_chapter_ids.len(),
                chapter_ids: new_chapter_ids.clone(),
                chapter_names,
            });
        }

        Ok(new_chapter_ids)
    }

    pub async fn start_refresh_all(&self) -> Result<()> {
        let mut task_guard = self.refresh_task.lock().await;

        if let Some(handle) = &*task_guard
            && !handle.is_finished()
        {
            return Err(ServiceError::Internal("Refresh already in progress".into()));
        }

        let ids: Vec<(MangaId, String)> = sqlx::query!(
            "SELECT id, name FROM manga WHERE deleted_at IS NULL AND is_orphaned = FALSE ORDER BY id"
        )
        .fetch_all(&self.db_read)
        .await?
        .into_iter()
        .map(|r| (r.id.into(), r.name))
        .collect();

        let total = ids.len();
        let state = self.clone();

        let handle = tokio::spawn(async move {
            let manga_ids: Vec<MangaId> = ids.iter().map(|(id, _)| *id).collect();
            let _ = state
                .refresh_tx
                .send(AppEvent::Refresh(RefreshProgressEvent::Started {
                    total,
                    manga_ids,
                }));

            let mut futures = FuturesUnordered::new();
            for (id, name) in ids {
                let s = state.clone();
                let n = name.clone();
                futures.push(async move {
                    let success = s.refresh_manga(id).await.is_ok();
                    (id, n, success)
                });
            }

            let mut completed = 0usize;
            let mut failed = 0usize;

            while let Some((manga_id, manga_name, success)) = futures.next().await {
                completed += 1;
                if !success {
                    failed += 1;
                }

                let _ = state.refresh_tx.send(AppEvent::Refresh(
                    RefreshProgressEvent::MangaRefreshed {
                        manga_id,
                        manga_name,
                        completed,
                        total,
                        success,
                        // start_refresh_all does metadata refresh, not chapter scanning,
                        // so new_chapters is not meaningful here.
                        new_chapters: 0,
                    },
                ));
            }

            let _ = state
                .refresh_tx
                .send(AppEvent::Refresh(RefreshProgressEvent::Completed {
                    total,
                    failed,
                }));

            *state.refresh_task.lock().await = None;
        })
        .abort_handle();

        *task_guard = Some(handle);
        Ok(())
    }

    pub async fn is_refreshing(&self) -> bool {
        self.refresh_task
            .lock()
            .await
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    }

    /// Aborts the currently running on-demand refresh task, if any.
    pub async fn abort_refresh(&self) {
        if let Some(handle) = self.refresh_task.lock().await.take() {
            handle.abort();
        }
    }

    pub(crate) async fn fetch_and_store_chapter_page(
        &self,
        source_id: i64,
        manga_id: &str,
        manga_row_id: MangaId,
        page: i32,
    ) -> Result<bool> {
        let backend = self
            .sources
            .get_backend(source_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Source {} not found", source_id)))?;
        let started = std::time::Instant::now();
        let attempt = backend.get_chapter_list(manga_id, page, None, None).await;
        self.record_source_call(source_id, started, &attempt).await;
        let res = attempt?;
        let json = serde_json::to_string(&res)
            .map_err(|e| ServiceError::Core(kani_core::Error::Json(e)))?;
        let chapter_list: wit_types::ChapterList = serde_json::from_str(&json)
            .map_err(|e| ServiceError::Internal(format!("Failed to parse chapter list: {}", e)))?;

        if chapter_list.chapters.is_empty() {
            return Ok(false);
        }

        for chunk in chapter_list.chapters.chunks(100) {
            let mut query_builder = sqlx::QueryBuilder::new(
                "INSERT OR IGNORE INTO chapters (manga_id, source_chapter_id, name, chapter_number, language, volume, scanlator, uploaded_at, discovered_at) ",
            );

            query_builder.push_values(chunk, |mut b, chapter| {
                b.push_bind(manga_row_id)
                    .push_bind(chapter.id.clone())
                    .push_bind(chapter.title.clone())
                    .push_bind(chapter.number)
                    .push_bind(chapter.language.clone())
                    .push_bind(chapter.volume)
                    .push_bind(chapter.scanlator.clone())
                    .push_bind(chapter.date_uploaded);
                b.push("NULL");
            });

            query_builder.build().execute(&self.db).await?;
        }

        Ok(chapter_list.has_next_page)
    }

    /// Queues a background scan for every manga in the library.
    /// Returns immediately with the count of manga queued.
    pub async fn scan_all_manga(&self) -> Result<uuid::Uuid> {
        let ids: Vec<MangaId> = sqlx::query!(
            "SELECT id FROM manga WHERE deleted_at IS NULL AND is_orphaned = FALSE ORDER BY id"
        )
        .fetch_all(&self.db_read)
        .await?
        .into_iter()
        .map(|r| r.id.into())
        .collect();
        self.scan_manga_ids(ids).await
    }

    /// Scans a specific list of manga IDs for new chapters, emitting SSE progress
    /// events compatible with the scan-all event stream. The caller receives
    /// `Started`, per-manga `MangaRefreshed`, and `Completed` events.
    /// Returns the ID of the submitted `LibraryScanJob`.
    pub async fn scan_manga_ids(&self, ids: Vec<MangaId>) -> Result<uuid::Uuid> {
        let raw_ids: Vec<i64> = ids.iter().map(|id| id.0).collect();
        let job = crate::jobs::download::LibraryScanJob::new(raw_ids, "manual".to_string());
        self.job_manager
            .submit(job)
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))
    }

    pub(crate) async fn fetch_and_store_remaining_chapters(
        &self,
        source_id: i64,
        manga_id: String,
        manga_row_id: MangaId,
        start_page: i32,
    ) {
        let mut page = start_page;
        loop {
            match self
                .fetch_and_store_chapter_page(source_id, &manga_id, manga_row_id, page)
                .await
            {
                Ok(has_next_page) => {
                    if !has_next_page {
                        break;
                    }
                    page += 1;
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to fetch chapter page {} for manga {}: {:?}",
                        page,
                        manga_id,
                        e
                    );
                    break;
                }
            }
        }
    }
}
