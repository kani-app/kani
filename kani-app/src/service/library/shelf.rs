use super::super::*;
use crate::ids::{MangaId, UserId};

impl AppService {
    /// Returns the continue-reading shelf: manga the user has started that still have
    /// unread chapters, ordered by most-recently-read first.
    pub async fn get_continue_reading_shelf(
        &self,
        user_id: UserId,
        limit: i64,
    ) -> Result<Vec<crate::models::ContinueReadingItem>> {
        // Read chapters are matched by subquery rather than joined in: joining
        // them lets the planner drive the query from user_chapter_tracking, and
        // the unread check below is then evaluated once per read chapter.
        let mangas = sqlx::query!(
            r#"
            SELECT m.id, m.source_id, COALESCE(m.local_name, m.name) as "name!: String", m.cover_url, m.local_cover_path, m.cover_hash, s.base_url
            FROM manga m
            JOIN sources s ON s.id = m.source_id
            WHERE m.deleted_at IS NULL
              AND EXISTS (
                  SELECT 1 FROM chapters rc
                  JOIN user_chapter_tracking ruct ON ruct.chapter_id = rc.id
                  WHERE rc.manga_id = m.id AND ruct.user_id = ?1 AND ruct.is_read = true
              )
              AND EXISTS (
                  SELECT 1 FROM chapters c2
                  WHERE c2.manga_id = m.id
                    AND NOT EXISTS (
                        SELECT 1 FROM chapters c3
                        JOIN user_chapter_tracking uct2 ON uct2.chapter_id = c3.id
                        WHERE c3.manga_id = m.id
                          AND c3.chapter_number = c2.chapter_number
                          AND uct2.user_id = ?1
                          AND uct2.is_read = true
                    )
              )
            ORDER BY (
                SELECT MAX(uct.last_read_at) FROM user_chapter_tracking uct
                JOIN chapters c ON c.id = uct.chapter_id
                WHERE c.manga_id = m.id AND uct.user_id = ?1 AND uct.is_read = true
            ) DESC
            LIMIT ?2
            "#,
            user_id,
            limit,
        )
        .fetch_all(&self.db_read)
        .await?;

        let mut items = Vec::new();
        for row in mangas {
            let Ok(Some(next)) = self
                .get_continue_reading_chapter(user_id, MangaId(row.id))
                .await
            else {
                continue;
            };
            items.push(crate::models::ContinueReadingItem {
                manga_id: MangaId(row.id),
                source_id: row.source_id,
                manga_name: row.name,
                cover_url: row.cover_url,
                local_cover_path: row.local_cover_path,
                cover_hash: row.cover_hash,
                base_url: row.base_url,
                chapter_id: crate::ids::ChapterId(next.chapter_id),
                chapter_number: next.chapter_number,
                last_page: next.last_page,
                page_count: next.page_count,
            });
        }
        Ok(items)
    }
}
