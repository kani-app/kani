use super::*;
use crate::ids::MangaId;

impl AppService {
    /// Submit a background job to generate cover thumbnails, deduplicating against any
    /// pending/running thumbnail job already queued for the same manga.
    pub async fn spawn_thumbnail_generation(&self, manga_id: MangaId) {
        if self.thumbnail_job_active(manga_id.0).await {
            return;
        }
        let job = crate::jobs::thumbnail::ThumbnailGenerationJob::new(manga_id.0);
        if let Err(e) = self.job_manager.submit(job).await {
            tracing::warn!("Failed to submit thumbnail job for manga {manga_id}: {e}");
        }
    }

    async fn thumbnail_job_active(&self, manga_id: i64) -> bool {
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM jobs WHERE job_type = 'thumbnail_generation' \
             AND status IN ('pending', 'running') \
             AND json_extract(params_json, '$.manga_id') = ?",
            manga_id
        )
        .fetch_one(&self.db_read)
        .await
        .map(|c| c > 0)
        .unwrap_or(false)
    }

    pub async fn generate_and_store_thumbnails(&self, manga_id: MangaId) -> Result<()> {
        let library_path = self.settings.read().await.library_path.clone();

        let rel: Option<String> =
            sqlx::query_scalar("SELECT local_cover_path FROM manga WHERE id = ?")
                .bind(manga_id)
                .fetch_optional(&self.db_read)
                .await?
                .flatten();

        let rel =
            rel.ok_or_else(|| ServiceError::NotFound(format!("No cover for manga {manga_id}")))?;

        let cover_path = library_path.join(&rel);
        kani_core::utilities::assert_within_root(&library_path, &cover_path)
            .map_err(|e| ServiceError::Internal(format!("Cover path traversal: {e}")))?;

        let source_bytes = tokio::fs::read(&cover_path)
            .await
            .map_err(|e| ServiceError::Internal(format!("Read cover failed: {e}")))?;

        let formats =
            crate::images::parse_thumbnail_formats(&self.settings.read().await.thumbnail_formats);
        let lib = library_path.clone();
        let mid = manga_id.0;

        let (hash, entries) = tokio::task::spawn_blocking(move || {
            crate::images::generate_thumbnails_sync(&source_bytes, mid, &lib, &formats)
        })
        .await
        .map_err(|e| ServiceError::Internal(format!("Thumbnail task panicked: {e}")))?
        .map_err(ServiceError::Internal)?;

        for entry in &entries {
            sqlx::query(
                "INSERT OR REPLACE INTO cover_thumbnails \
                 (manga_id, size, format, path, file_size, created_at) \
                 VALUES (?, ?, ?, ?, ?, datetime('now'))",
            )
            .bind(manga_id)
            .bind(entry.size)
            .bind(&entry.format)
            .bind(&entry.path)
            .bind(entry.file_size)
            .execute(&self.db)
            .await?;
        }

        sqlx::query("UPDATE manga SET cover_hash = ? WHERE id = ?")
            .bind(&hash)
            .bind(manga_id)
            .execute(&self.db)
            .await?;

        Ok(())
    }

    pub async fn clear_thumbnails(&self, manga_id: MangaId) {
        let library_path = self.settings.read().await.library_path.clone();

        let rows: Vec<String> =
            sqlx::query_scalar("SELECT path FROM cover_thumbnails WHERE manga_id = ?")
                .bind(manga_id)
                .fetch_all(&self.db_read)
                .await
                .unwrap_or_default();

        for rel in rows {
            let full = library_path.join(&rel);
            if kani_core::utilities::assert_within_root(&library_path, &full).is_ok() {
                let _ = tokio::fs::remove_file(&full).await;
            }
        }

        let thumb_dir = library_path.join("covers").join(manga_id.0.to_string());
        let _ = tokio::fs::remove_dir(&thumb_dir).await;

        if let Err(e) = sqlx::query("DELETE FROM cover_thumbnails WHERE manga_id = ?")
            .bind(manga_id)
            .execute(&self.db)
            .await
        {
            tracing::warn!("clear_thumbnails: DB delete failed for manga {manga_id}: {e}");
        }

        if let Err(e) = sqlx::query("UPDATE manga SET cover_hash = NULL WHERE id = ?")
            .bind(manga_id)
            .execute(&self.db)
            .await
        {
            tracing::warn!("clear_thumbnails: cover_hash clear failed for manga {manga_id}: {e}");
        }
    }

    pub async fn get_thumbnail_for_size(
        &self,
        manga_id: MangaId,
        size: &str,
    ) -> Result<Option<(std::path::PathBuf, String, String)>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            path: String,
            format: String,
        }

        let row: Option<Row> = sqlx::query_as(
            "SELECT path, format FROM cover_thumbnails WHERE manga_id = ? AND size = ? \
             ORDER BY CASE format WHEN 'webp' THEN 0 ELSE 1 END LIMIT 1",
        )
        .bind(manga_id)
        .bind(size)
        .fetch_optional(&self.db_read)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let library_path = self.settings.read().await.library_path.clone();
        let full = library_path.join(&row.path);
        kani_core::utilities::assert_within_root(&library_path, &full)
            .map_err(|e| ServiceError::Internal(format!("Thumbnail path traversal: {e}")))?;

        let cover_hash: Option<String> =
            sqlx::query_scalar("SELECT cover_hash FROM manga WHERE id = ?")
                .bind(manga_id)
                .fetch_optional(&self.db_read)
                .await?
                .flatten();

        Ok(Some((full, row.format, cover_hash.unwrap_or_default())))
    }
}
