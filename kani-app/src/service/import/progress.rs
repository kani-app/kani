//! Applies the read progress a backup carried once its chapters exist.
//!
//! An imported manga has no chapter rows at import time — its id is not one the
//! source accepts until the resolve job repairs it — so progress is parked and
//! replayed when a chapter list finally lands.

use crate::error::Result;
use crate::ids::{MangaId, UserId};
use crate::service::AppService;

/// How far apart two chapter numbers may be and still be the same chapter.
///
/// Mihon stores the number as a 32-bit float, so `1.1` reaches us as
/// `1.100000023841858` and never equals the `1.1` a source parsed.
const NUMBER_EPSILON: f64 = 0.001;

/// One chapter's progress as the backup recorded it.
pub struct ImportedProgress {
    pub source_chapter_id: String,
    pub chapter_number: f64,
    pub is_read: bool,
    pub last_page_read: i64,
}

impl AppService {
    /// Parks `entries` until the manga has chapters to match them against.
    pub async fn store_import_progress(
        &self,
        manga_id: MangaId,
        user_id: UserId,
        entries: &[ImportedProgress],
    ) -> Result<()> {
        for e in entries {
            sqlx::query!(
                "INSERT OR REPLACE INTO manga_import_progress \
                 (manga_id, user_id, source_chapter_id, chapter_number, is_read, last_page_read) \
                 VALUES (?, ?, ?, ?, ?, ?)",
                manga_id,
                user_id,
                e.source_chapter_id,
                e.chapter_number,
                e.is_read,
                e.last_page_read
            )
            .execute(&self.db)
            .await?;
        }
        Ok(())
    }

    /// Replays parked progress for `manga_id`, returning how many entries could
    /// not be matched to a chapter.
    ///
    /// Matched entries are consumed; unmatched ones are dropped, because the
    /// chapter list they were waiting for has now arrived and a second pass over
    /// the same list would reject them again.
    pub async fn apply_stored_import_progress(&self, manga_id: MangaId) -> Result<u32> {
        let parked = sqlx::query!(
            "SELECT user_id, source_chapter_id, chapter_number, is_read, last_page_read \
             FROM manga_import_progress WHERE manga_id = ?",
            manga_id
        )
        .fetch_all(&self.db_read)
        .await?;

        if parked.is_empty() {
            return Ok(0);
        }

        let mut unmatched = 0;
        for row in &parked {
            let applied = self
                .apply_one_progress(
                    manga_id,
                    UserId(row.user_id),
                    &row.source_chapter_id,
                    row.chapter_number,
                    row.is_read,
                    row.last_page_read,
                )
                .await?;
            if !applied {
                unmatched += 1;
            }
        }

        sqlx::query!(
            "DELETE FROM manga_import_progress WHERE manga_id = ?",
            manga_id
        )
        .execute(&self.db)
        .await?;

        Ok(unmatched)
    }

    /// Marks the chapters this entry refers to, reporting whether any matched.
    ///
    /// The backup's own chapter id is tried first so a source whose ids happen to
    /// agree keeps an exact match; otherwise the chapter number identifies it,
    /// and every scanlator's copy of that number is marked — the same rule the
    /// continue-reading shelf already reads by.
    pub(crate) async fn apply_one_progress(
        &self,
        manga_id: MangaId,
        user_id: UserId,
        source_chapter_id: &str,
        chapter_number: f64,
        is_read: bool,
        last_page_read: i64,
    ) -> Result<bool> {
        let mut ids: Vec<i64> = sqlx::query_scalar!(
            "SELECT id FROM chapters WHERE manga_id = ? AND source_chapter_id = ?",
            manga_id,
            source_chapter_id
        )
        .fetch_all(&self.db_read)
        .await?;

        if ids.is_empty() && chapter_number > 0.0 {
            ids = sqlx::query_scalar!(
                "SELECT id FROM chapters WHERE manga_id = ? AND ABS(chapter_number - ?) < ?",
                manga_id,
                chapter_number,
                NUMBER_EPSILON
            )
            .fetch_all(&self.db_read)
            .await?;
        }

        if ids.is_empty() {
            return Ok(false);
        }

        for id in ids {
            // Updated rather than replaced: REPLACE would drop `last_read_at`,
            // which orders the continue-reading shelf.
            sqlx::query!(
                "INSERT INTO user_chapter_tracking \
                 (user_id, chapter_id, is_read, last_page_read) VALUES (?, ?, ?, ?) \
                 ON CONFLICT(user_id, chapter_id) DO UPDATE SET \
                 is_read = excluded.is_read, last_page_read = excluded.last_page_read",
                user_id,
                id,
                is_read,
                last_page_read
            )
            .execute(&self.db)
            .await?;
        }
        Ok(true)
    }
}
