use crate::error::{Result, ServiceError};
use crate::ids::{MangaId, UserId};
use crate::models::{OrphanedManga, PendingImportRow};
use crate::service::AppService;

impl AppService {
    pub async fn list_pending_imports(&self, user_id: UserId) -> Result<Vec<PendingImportRow>> {
        let rows = sqlx::query_as!(
            PendingImportRow,
            r#"SELECT pi.id, pi.origin, pi.title, pi.source_hint AS "source_hint?: crate::ids::SourceId", pi.source_manga_id,
                      pi.description, pi.cover_url, pi.authors, pi.tags, pi.status,
                      pi.tracking, pi.chapter_progress, pi.possible_duplicate_of,
                      m.name AS possible_duplicate_title,
                      pi.duplicate_similarity,
                      strftime('%Y-%m-%dT%H:%M:%SZ', pi.created_at) AS created_at
               FROM pending_imports pi
               LEFT JOIN manga m ON m.id = pi.possible_duplicate_of
               WHERE pi.user_id = ? AND pi.resolved = FALSE
               ORDER BY pi.created_at DESC"#,
            user_id
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows)
    }

    pub async fn delete_pending_import(&self, user_id: UserId, id: i64) -> Result<()> {
        let affected = sqlx::query!(
            "DELETE FROM pending_imports WHERE id = ? AND user_id = ?",
            id,
            user_id
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(ServiceError::NotFound("Pending import not found".into()));
        }
        Ok(())
    }

    /// Resolve a pending import: the user has found the manga on `source_id`
    /// with the given `source_manga_id`. Inserts the manga and applies stored
    /// tracking. Returns the new manga's DB id.
    pub async fn resolve_pending_import(
        &self,
        user_id: UserId,
        id: i64,
        source_id: i64,
        source_manga_id: &str,
    ) -> Result<MangaId> {
        let row = sqlx::query!(
            "SELECT title, tracking, chapter_progress FROM pending_imports \
             WHERE id = ? AND user_id = ? AND resolved = FALSE",
            id,
            user_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound("Pending import not found".into()))?;

        let source_exists = sqlx::query_scalar!(
            "SELECT id FROM sources WHERE id = ? AND deleted_at IS NULL",
            source_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .is_some();

        if !source_exists {
            return Err(ServiceError::NotFound("Source not found".into()));
        }

        let mut tx = self.db.begin().await?;

        let manga_id = MangaId(
            sqlx::query_scalar!(
                "INSERT INTO manga (source_id, source_manga_id, name) \
             VALUES (?, ?, ?) RETURNING id",
                source_id,
                source_manga_id,
                row.title
            )
            .fetch_one(&mut *tx)
            .await?,
        );

        if let Some(tracking_json) = &row.tracking {
            #[derive(serde::Deserialize)]
            struct TrackingData {
                status: i64,
                score: Option<f64>,
            }
            if let Ok(t) = serde_json::from_str::<TrackingData>(tracking_json) {
                sqlx::query!(
                    "INSERT OR REPLACE INTO user_manga_tracking (user_id, manga_id, status, score) \
                     VALUES (?, ?, ?, ?)",
                    user_id,
                    manga_id,
                    t.status,
                    t.score
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        sqlx::query!(
            "UPDATE pending_imports SET resolved = TRUE WHERE id = ?",
            id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let pool = self.db.clone();
        tokio::spawn(async move {
            if let Err(e) =
                crate::service::dedup::record_duplicates_for_manga(&pool, manga_id).await
            {
                tracing::warn!("Duplicate recording failed for manga {manga_id}: {e}");
            }
        });

        Ok(manga_id)
    }

    pub async fn list_orphaned_manga(&self) -> Result<Vec<OrphanedManga>> {
        let rows = sqlx::query_as!(
            OrphanedManga,
            r#"SELECT m.id, m.name, m.cover_url, m.local_cover_path,
                      COALESCE(s.name, 'Unknown') AS "source_name!: String"
               FROM manga m
               LEFT JOIN sources s ON s.id = m.source_id
               WHERE m.is_orphaned = TRUE
               ORDER BY m.name"#
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows)
    }
}
