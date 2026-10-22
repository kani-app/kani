use super::*;
use crate::ids::{ChapterId, MangaId, UserId};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct ReadProgressBuffer(std::sync::Arc<std::sync::Mutex<HashMap<(i64, i64), i64>>>);

impl ReadProgressBuffer {
    pub fn record(&self, user_id: i64, chapter_id: i64, page: i64) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.insert((user_id, chapter_id), page);
    }

    pub fn remove(&self, user_id: i64, chapter_id: i64) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(&(user_id, chapter_id));
    }

    fn drain(&self) -> HashMap<(i64, i64), i64> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *map)
    }
}

impl AppService {
    /// Manga this user has muted new-chapter notifications for.
    ///
    /// Returns only exceptions because notification is enabled by default.
    pub async fn muted_manga_ids(&self, user_id: UserId) -> Result<Vec<i64>> {
        Ok(sqlx::query_scalar!(
            "SELECT manga_id FROM user_manga_tracking \
             WHERE user_id = ? AND notify_new_chapters = FALSE",
            user_id
        )
        .fetch_all(&self.db_read)
        .await?)
    }

    pub async fn set_chapter_progress(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        page: i64,
    ) -> Result<()> {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chapters WHERE id = ?)")
            .bind(chapter_id)
            .fetch_one(&self.db_read)
            .await?;
        if !exists {
            return Err(crate::error::ServiceError::NotFound(format!(
                "Chapter {chapter_id} not found"
            )));
        }
        self.progress_buffer.record(user_id.0, chapter_id.0, page);
        Ok(())
    }

    pub async fn flush_progress_buffer(&self) {
        let entries = self.progress_buffer.drain();
        if entries.is_empty() {
            return;
        }
        let mut tx = match self.db.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::warn!("Progress flush begin failed: {e}");
                return;
            }
        };
        let mut affected_users: HashSet<i64> = HashSet::new();
        for ((user_id, chapter_id), page) in &entries {
            let result = sqlx::query(
                r#"INSERT INTO user_chapter_tracking (user_id, chapter_id, last_page_read, is_read, last_read_at)
                   VALUES (?1, ?2, ?3,
                       COALESCE(?3 >= ((SELECT page_count FROM chapters WHERE id = ?2) - 1), false),
                       datetime('now'))
                   ON CONFLICT (user_id, chapter_id) DO UPDATE SET
                       last_page_read = excluded.last_page_read,
                       is_read = user_chapter_tracking.is_read OR excluded.is_read,
                       last_read_at = datetime('now')"#,
            )
            .bind(user_id)
            .bind(chapter_id)
            .bind(page)
            .execute(&mut *tx)
            .await;
            match result {
                Ok(_) => {
                    affected_users.insert(*user_id);
                }
                Err(e) => {
                    tracing::warn!("Progress flush: user={user_id} chapter={chapter_id}: {e}");
                }
            }
        }
        if let Err(e) = tx.commit().await {
            tracing::warn!("Progress flush commit failed: {e}");
            return;
        }
        for user_id in affected_users {
            self.cache.invalidate_stats(UserId(user_id));
        }
    }

    pub fn spawn_progress_flush(&self) {
        let svc = self.clone();
        let token = self.shutdown_token.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        svc.flush_progress_buffer().await;
                        break;
                    }
                    _ = interval.tick() => {
                        svc.flush_progress_buffer().await;
                    }
                }
            }
        });
    }

    pub async fn set_chapter_read_status(
        &self,
        user_id: UserId,
        chapter_ids: Vec<ChapterId>,
        is_read: bool,
    ) -> Result<()> {
        let json_chapter_ids = serde_json::to_string(&chapter_ids)
            .map_err(|e| ServiceError::Internal(e.to_string()))?;

        sqlx::query!(
            r#"
            INSERT INTO user_chapter_tracking (user_id, chapter_id, last_page_read, is_read, last_read_at)
            SELECT ?1, CAST(value AS INTEGER), 0, ?3,
                   CASE WHEN ?3 THEN datetime('now') ELSE NULL END
            FROM json_each(?2)
            WHERE 1=1
            ON CONFLICT (user_id, chapter_id) DO UPDATE SET
                is_read = excluded.is_read,
                last_page_read = CASE WHEN excluded.is_read THEN user_chapter_tracking.last_page_read ELSE 0 END,
                last_read_at = CASE WHEN excluded.is_read THEN datetime('now') ELSE user_chapter_tracking.last_read_at END
            "#,
            user_id,
            json_chapter_ids,
            is_read
        )
        .execute(&self.db)
        .await?;

        for chapter_id in &chapter_ids {
            self.progress_buffer.remove(user_id.0, chapter_id.0);
        }
        self.cache.invalidate_stats(user_id);
        Ok(())
    }

    pub async fn set_manga_status(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        status: kani_shared::types::MangaTrackingStatus,
    ) -> Result<()> {
        let status_int = status as i64;

        sqlx::query!(
            r#"
            INSERT INTO user_manga_tracking (user_id, manga_id, status) 
            VALUES (?1, ?2, ?3)
            ON CONFLICT (user_id, manga_id) DO UPDATE SET 
                status = excluded.status
            "#,
            user_id,
            manga_id,
            status_int
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }

    pub async fn get_chapter_progress(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> Result<Option<(i64, bool)>> {
        let row = sqlx::query!(
            r#"
            SELECT last_page_read, is_read as "is_read: bool"
            FROM user_chapter_tracking
            WHERE user_id = ? AND chapter_id = ?
            "#,
            user_id,
            chapter_id
        )
        .fetch_optional(&self.db_read)
        .await?;

        Ok(row.map(|r| (r.last_page_read, r.is_read)))
    }

    /// Like [`Self::get_chapter_progress`] but also returns the last-read timestamp
    /// formatted as RFC 3339 (for the OPDS-PSE `pse:lastReadDate` attribute).
    pub(crate) async fn get_chapter_progress_full(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> Result<Option<(i64, bool, Option<String>)>> {
        let row = sqlx::query!(
            r#"
            SELECT last_page_read,
                   is_read as "is_read: bool",
                   strftime('%Y-%m-%dT%H:%M:%SZ', last_read_at) as "last_read_rfc3339?: String"
            FROM user_chapter_tracking
            WHERE user_id = ? AND chapter_id = ?
            "#,
            user_id,
            chapter_id
        )
        .fetch_optional(&self.db_read)
        .await?;

        Ok(row.map(|r| (r.last_page_read, r.is_read, r.last_read_rfc3339)))
    }

    pub async fn set_manga_tracking_enabled(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        enabled: bool,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO user_manga_tracking (user_id, manga_id, tracking_enabled)
            VALUES (?1, ?2, ?3)
            ON CONFLICT (user_id, manga_id) DO UPDATE SET
                tracking_enabled = excluded.tracking_enabled
            "#,
            user_id,
            manga_id,
            enabled
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn set_manga_notify(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        notify: bool,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO user_manga_tracking (user_id, manga_id, notify_new_chapters)
            VALUES (?1, ?2, ?3)
            ON CONFLICT (user_id, manga_id) DO UPDATE SET
                notify_new_chapters = excluded.notify_new_chapters
            "#,
            user_id,
            manga_id,
            notify
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn set_reading_direction(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        direction: &str,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO user_manga_tracking (user_id, manga_id, reading_direction)
            VALUES (?1, ?2, ?3)
            ON CONFLICT (user_id, manga_id) DO UPDATE SET
                reading_direction = excluded.reading_direction
            "#,
            user_id,
            manga_id,
            direction
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn set_reader_prefs(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        prefs: &str,
    ) -> Result<()> {
        // Reject anything that isn't a JSON object so the column stays well-formed.
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(prefs).map_err(
            |_| crate::error::ServiceError::Validation("reader_prefs must be a JSON object".into()),
        )?;
        sqlx::query!(
            r#"
            INSERT INTO user_manga_tracking (user_id, manga_id, reader_prefs)
            VALUES (?1, ?2, ?3)
            ON CONFLICT (user_id, manga_id) DO UPDATE SET
                reader_prefs = excluded.reader_prefs
            "#,
            user_id,
            manga_id,
            prefs
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn get_manga_tracking(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<kani_shared::types::MangaTracking> {
        let tracking = sqlx::query!(
            r#"
            SELECT status, score,
                   tracking_enabled as "tracking_enabled: bool",
                   COALESCE(notify_new_chapters, TRUE) as "notify_new_chapters: bool",
                   COALESCE(reading_direction, 'rtl') as "reading_direction: String",
                   reader_prefs
            FROM user_manga_tracking
            WHERE user_id = ? AND manga_id = ?
            "#,
            user_id,
            manga_id
        )
        .fetch_optional(&self.db_read)
        .await?;

        // Count distinct chapter numbers read (coalesces multiple scanlator versions).
        let chapters_read = sqlx::query_scalar!(
            r#"
            SELECT COUNT(DISTINCT c.chapter_number) as "count: i64"
            FROM user_chapter_tracking uct
            JOIN chapters c ON c.id = uct.chapter_id
            WHERE uct.user_id = ? AND c.manga_id = ? AND uct.is_read = true
            "#,
            user_id,
            manga_id
        )
        .fetch_one(&self.db_read)
        .await?;

        // Count distinct chapter numbers (not rows) so scanlator dupes don't inflate total.
        let total_chapters = sqlx::query_scalar!(
            r#"SELECT COUNT(DISTINCT chapter_number) as "count: i64" FROM chapters WHERE manga_id = ?"#,
            manga_id
        )
        .fetch_one(&self.db_read)
        .await?;

        let (status, score, tracking_enabled, notify_new_chapters, reading_direction, reader_prefs) =
            match tracking {
                Some(row) => {
                    let status = match row.status {
                        0 => Some(kani_shared::types::MangaTrackingStatus::Reading),
                        1 => Some(kani_shared::types::MangaTrackingStatus::OnHold),
                        2 => Some(kani_shared::types::MangaTrackingStatus::Dropped),
                        3 => Some(kani_shared::types::MangaTrackingStatus::PlanToRead),
                        4 => Some(kani_shared::types::MangaTrackingStatus::Completed),
                        5 => Some(kani_shared::types::MangaTrackingStatus::Rereading),
                        _ => None,
                    };
                    (
                        status,
                        row.score,
                        row.tracking_enabled,
                        row.notify_new_chapters.unwrap_or(true),
                        row.reading_direction,
                        row.reader_prefs,
                    )
                }
                None => {
                    let default = self.settings.read().await.default_tracking_enabled;
                    (None, None, default, true, None, None)
                }
            };

        Ok(kani_shared::types::MangaTracking {
            status,
            score,
            chapters_read,
            total_chapters,
            tracking_enabled,
            notify_new_chapters,
            reading_direction: reading_direction.unwrap_or_else(|| "rtl".to_string()),
            reader_prefs,
        })
    }

    pub async fn set_manga_score(
        &self,
        user_id: UserId,
        manga_id: MangaId,
        score: f64,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO user_manga_tracking (user_id, manga_id, score)
            VALUES (?1, ?2, ?3)
            ON CONFLICT (user_id, manga_id) DO UPDATE SET
                score = excluded.score
            "#,
            user_id,
            manga_id,
            score
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Find the next chapter to read for a manga:
    ///    1. The lowest in-progress chapter (started but not finished).
    ///    2. Otherwise, the lowest unread chapter number, picking the preferred scanlator version.
    ///
    /// Returns `None` when all chapters are read or there are no chapters.
    pub async fn get_continue_reading_chapter(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<Option<kani_shared::types::ContinueReadingChapter>> {
        let in_progress = sqlx::query!(
            r#"
            SELECT c.id as "id: i64", c.chapter_number as "chapter_number: f64",
                   uct.last_page_read as "last_page_read: i64",
                   COALESCE(c.page_count, 0) as "page_count!: i64"
            FROM chapters c
            JOIN manga m ON m.id = c.manga_id
            JOIN user_chapter_tracking uct ON uct.chapter_id = c.id
            LEFT JOIN scanlator_preferences sp
                ON sp.id = (SELECT sp2.id FROM scanlator_preferences sp2
                            WHERE (sp2.manga_id = c.manga_id OR sp2.manga_id IS NULL)
                              AND sp2.scanlator = c.scanlator
                            ORDER BY sp2.manga_id IS NULL LIMIT 1)
            WHERE c.manga_id = ? AND uct.user_id = ?
              AND uct.is_read = false AND uct.last_page_read > 0
              AND c.download_status = 2
              AND (
                  (m.scanlator_mode = 'priority' AND (sp.blocked IS NULL OR sp.blocked = 0)) OR
                  (m.scanlator_mode = 'whitelist' AND sp.id IS NOT NULL)
              )
            ORDER BY c.chapter_number ASC
            LIMIT 1
            "#,
            manga_id,
            user_id,
        )
        .fetch_optional(&self.db_read)
        .await?;

        if let Some(row) = in_progress {
            return Ok(Some(kani_shared::types::ContinueReadingChapter {
                chapter_id: row.id,
                chapter_number: row.chapter_number,
                last_page: row.last_page_read,
                page_count: row.page_count,
            }));
        }

        let next_number = sqlx::query_scalar!(
            r#"
            SELECT MIN(c.chapter_number) as "num: f64"
            FROM chapters c
            JOIN manga m ON m.id = c.manga_id
            LEFT JOIN scanlator_preferences sp 
                ON sp.id = (SELECT sp2.id FROM scanlator_preferences sp2
                            WHERE (sp2.manga_id = c.manga_id OR sp2.manga_id IS NULL)
                              AND sp2.scanlator = c.scanlator
                            ORDER BY sp2.manga_id IS NULL LIMIT 1)
            WHERE c.manga_id = ?
              AND c.download_status = 2
              AND (
                  (m.scanlator_mode = 'priority' AND (sp.blocked IS NULL OR sp.blocked = 0)) OR
                  (m.scanlator_mode = 'whitelist' AND sp.id IS NOT NULL)
              )
              AND NOT EXISTS (
                  SELECT 1 FROM chapters c2
                  JOIN user_chapter_tracking uct2 ON uct2.chapter_id = c2.id
                  WHERE c2.manga_id = c.manga_id
                    AND c2.chapter_number = c.chapter_number
                    AND uct2.user_id = ?
                    AND uct2.is_read = true
              )
            "#,
            manga_id,
            user_id,
        )
        .fetch_one(&self.db_read)
        .await?;

        let Some(chapter_number) = next_number else {
            return Ok(None);
        };

        let chapter = sqlx::query!(
            r#"
            SELECT c.id as "id: i64", COALESCE(c.page_count, 0) as "page_count!: i64"
            FROM chapters c
            JOIN manga m ON m.id = c.manga_id
            LEFT JOIN scanlator_preferences sp
                ON sp.id = (SELECT sp2.id FROM scanlator_preferences sp2
                            WHERE (sp2.manga_id = c.manga_id OR sp2.manga_id IS NULL)
                              AND sp2.scanlator = c.scanlator
                            ORDER BY sp2.manga_id IS NULL LIMIT 1)
            WHERE c.manga_id = ? AND c.chapter_number = ?
              AND c.download_status = 2
              AND (
                  (m.scanlator_mode = 'priority' AND (sp.blocked IS NULL OR sp.blocked = 0)) OR
                  (m.scanlator_mode = 'whitelist' AND sp.id IS NOT NULL)
              )
            ORDER BY COALESCE(sp.priority, -1) DESC
            LIMIT 1
            "#,
            manga_id,
            chapter_number,
        )
        .fetch_optional(&self.db_read)
        .await?;

        Ok(chapter.map(|r| kani_shared::types::ContinueReadingChapter {
            chapter_id: r.id,
            chapter_number,
            last_page: 0,
            page_count: r.page_count,
        }))
    }

    /// Returns all chapter IDs for a manga where chapter_number <= the given number.
    /// Used by "mark as read/unread up to here".
    pub async fn get_chapters_up_to(
        &self,
        manga_id: MangaId,
        chapter_number: f64,
    ) -> Result<Vec<ChapterId>> {
        let ids: Vec<i64> = sqlx::query_scalar!(
            r#"SELECT id as "id: i64" FROM chapters WHERE manga_id = ? AND chapter_number <= ?"#,
            manga_id,
            chapter_number,
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(ids.into_iter().map(ChapterId).collect())
    }

    pub async fn get_bookmarks(&self, user_id: UserId, chapter_id: ChapterId) -> Result<Vec<i64>> {
        let pages = sqlx::query_scalar!(
            r#"SELECT page_index as "page_index: i64" FROM user_page_bookmarks
               WHERE user_id = ? AND chapter_id = ?
               ORDER BY page_index"#,
            user_id,
            chapter_id,
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(pages)
    }

    pub async fn toggle_bookmark(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        page_index: i64,
    ) -> Result<bool> {
        let deleted = sqlx::query!(
            "DELETE FROM user_page_bookmarks WHERE user_id = ? AND chapter_id = ? AND page_index = ?",
            user_id, chapter_id, page_index,
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if deleted > 0 {
            return Ok(false);
        }

        sqlx::query!(
            "INSERT INTO user_page_bookmarks (user_id, chapter_id, page_index) VALUES (?, ?, ?)",
            user_id,
            chapter_id,
            page_index,
        )
        .execute(&self.db)
        .await?;
        Ok(true)
    }

    pub async fn get_chapter_note(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> Result<Option<String>> {
        let note = sqlx::query_scalar!(
            r#"SELECT note FROM user_chapter_notes WHERE user_id = ? AND chapter_id = ?"#,
            user_id,
            chapter_id,
        )
        .fetch_optional(&self.db_read)
        .await?;
        Ok(note)
    }

    /// Returns chapter notes with text for a given user + manga, ordered by chapter number.
    pub async fn get_manga_chapter_notes_with_text(
        &self,
        user_id: UserId,
        manga_id: MangaId,
    ) -> Result<Vec<(ChapterId, f64, String)>> {
        let rows = sqlx::query!(
            r#"SELECT ucn.chapter_id as "chapter_id: i64",
                      c.chapter_number as "chapter_number: f64",
                      ucn.note
               FROM user_chapter_notes ucn
               JOIN chapters c ON c.id = ucn.chapter_id
               WHERE ucn.user_id = ? AND c.manga_id = ? AND ucn.note != ''
               ORDER BY c.chapter_number ASC"#,
            user_id,
            manga_id,
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| (ChapterId(r.chapter_id), r.chapter_number, r.note))
            .collect())
    }

    pub async fn set_chapter_note(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        note: &str,
    ) -> Result<()> {
        sqlx::query!(
            r#"INSERT INTO user_chapter_notes (user_id, chapter_id, note, updated_at)
               VALUES (?, ?, ?, datetime('now'))
               ON CONFLICT (user_id, chapter_id) DO UPDATE SET
                   note = excluded.note,
                   updated_at = excluded.updated_at"#,
            user_id,
            chapter_id,
            note,
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }
}
