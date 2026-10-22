use super::*;
use crate::ids::{ChapterId, MangaId};
use kani_shared::types::DownloadStatus;

impl AppService {
    pub async fn download_chapter(&self, chapter_id: ChapterId) -> Result<uuid::Uuid> {
        let claimed = sqlx::query!(
            "UPDATE chapters SET download_status = ? \
             WHERE id = ? AND download_status = ?",
            DownloadStatus::InProgress,
            chapter_id,
            DownloadStatus::Pending,
        )
        .execute(&self.db)
        .await?;

        if claimed.rows_affected() == 0 {
            return Err(ServiceError::Conflict(format!(
                "Chapter {chapter_id} is already downloaded or in progress."
            )));
        }

        self.enqueue_claimed_chapter(chapter_id).await
    }

    pub(crate) async fn enqueue_claimed_chapter(
        &self,
        chapter_id: ChapterId,
    ) -> Result<uuid::Uuid> {
        let result = async {
            let row = sqlx::query!(
                "SELECT c.manga_id, m.source_id, m.name as manga_title \
                 FROM chapters c JOIN manga m ON c.manga_id = m.id WHERE c.id = ?",
                chapter_id
            )
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Chapter {chapter_id} not found")))?;

            let job = crate::jobs::download::ChapterDownloadJob::new(
                chapter_id.0,
                row.manga_id,
                row.manga_title,
                row.source_id,
            );
            self.job_manager
                .submit(job)
                .await
                .map_err(|e| ServiceError::Internal(e.to_string()))
        }
        .await;

        if result.is_err() {
            let _ = sqlx::query!(
                "UPDATE chapters SET download_status = ? WHERE id = ?",
                DownloadStatus::Pending,
                chapter_id
            )
            .execute(&self.db)
            .await;
        }

        result
    }

    pub async fn retry_chapter_download(&self, chapter_id: ChapterId) -> Result<uuid::Uuid> {
        let err_json = sqlx::query_scalar!(
            "SELECT download_error FROM chapters WHERE id = ?",
            chapter_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Chapter {chapter_id} not found")))?;

        if let Some(raw) = err_json {
            let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
            if v.get("kind").and_then(|k| k.as_str()) == Some("not_found") {
                return Err(ServiceError::Conflict("chapter_missing_from_source".into()));
            }
        }

        self.download_chapter(chapter_id).await
    }

    pub async fn get_manga_download_status(&self, manga_id: MangaId) -> Result<serde_json::Value> {
        let rows = sqlx::query!(
            "SELECT download_status, download_error, id, name, chapter_number \
             FROM chapters WHERE manga_id = ? AND is_orphaned = 0",
            manga_id
        )
        .fetch_all(&self.db_read)
        .await?;

        let mut downloaded = 0i64;
        let mut in_progress = 0i64;
        let mut pending = 0i64;
        let mut failed_chapters = vec![];

        for r in rows {
            match r.download_status {
                2 => downloaded += 1,
                1 => in_progress += 1,
                _ => {
                    pending += 1;
                    if let Some(raw) = r.download_error {
                        let err_val: serde_json::Value =
                            serde_json::from_str(&raw).unwrap_or_default();
                        failed_chapters.push(serde_json::json!({
                            "id": r.id,
                            "name": r.name,
                            "chapterNumber": r.chapter_number,
                            "error": err_val,
                        }));
                    }
                }
            }
        }

        Ok(serde_json::json!({
            "total": downloaded + in_progress + pending,
            "downloaded": downloaded,
            "inProgress": in_progress,
            "pending": pending,
            "failedChapters": failed_chapters,
        }))
    }

    pub async fn download_all_chapters(&self, manga_id: MangaId) -> Result<uuid::Uuid> {
        let row = sqlx::query!(
            "SELECT source_id, name, download_all_preferred_only FROM manga WHERE id = ?",
            manga_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Manga {manga_id} not found")))?;

        let job = crate::jobs::download::MangaDownloadAllJob::new(
            manga_id.0,
            row.name,
            row.source_id,
            row.download_all_preferred_only,
        );
        self.job_manager
            .submit(job)
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))
    }

    pub async fn delete_downloaded(&self, chapter_id: ChapterId) -> Result<()> {
        let info = self.chapter_cbz_path(chapter_id).await?;

        let remove_ok = match tokio::fs::remove_file(&info.path).await {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(e) => {
                tracing::warn!("Failed to remove chapter file {:?}: {e}", info.path);
                false
            }
        };

        let is_orphaned: bool =
            sqlx::query_scalar!("SELECT is_orphaned FROM chapters WHERE id = ?", chapter_id)
                .fetch_one(&self.db_read)
                .await?;

        if is_orphaned {
            let _ = sqlx::query!("DELETE FROM chapters WHERE id = ?", chapter_id)
                .execute(&self.db)
                .await;
        } else if remove_ok {
            let _ = sqlx::query!(
                "UPDATE chapters SET download_status = ?, delete_status = NULL WHERE id = ?",
                DownloadStatus::Pending,
                chapter_id
            )
            .execute(&self.db)
            .await;
            // The file is gone: a surviving hash would make the chapter look
            // present to the scrub and to duplicate detection.
            let _ = self.clear_chapter_manifest(chapter_id).await;
        } else {
            let _ = sqlx::query!(
                "UPDATE chapters SET delete_status = 'pending_delete' WHERE id = ?",
                chapter_id
            )
            .execute(&self.db)
            .await;
        }

        Ok(())
    }

    pub async fn cancel_download(&self, chapter_id: ChapterId) -> Result<()> {
        self.cancel_chapter_jobs_where("$.chapter_id", chapter_id.0)
            .await
    }

    pub async fn cancel_all_downloads(&self, manga_id: MangaId) -> Result<()> {
        self.cancel_chapter_jobs_where("$.manga_id", manga_id.0)
            .await?;
        self.cancel_jobs_where_type_and_manga("manga_download_all", manga_id.0)
            .await
    }

    pub async fn cancel_all_global_downloads(&self) -> Result<()> {
        for type_str in &["chapter_download", "manga_download_all"] {
            let job_ids: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM jobs WHERE job_type = ? AND status IN ('pending', 'running')",
            )
            .bind(type_str)
            .fetch_all(&self.db_read)
            .await
            .unwrap_or_default();

            for id_str in job_ids {
                if let Ok(job_id) = uuid::Uuid::parse_str(&id_str) {
                    let _ = self.job_manager.cancel(job_id).await;
                }
            }
        }
        Ok(())
    }

    async fn cancel_chapter_jobs_where(&self, json_path: &str, value: i64) -> Result<()> {
        let job_ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM jobs WHERE job_type = 'chapter_download' AND status IN ('pending', 'running') AND json_extract(params_json, ?) = ?"
        )
        .bind(json_path)
        .bind(value)
        .fetch_all(&self.db_read)
        .await
        .unwrap_or_default();

        for id_str in job_ids {
            if let Ok(job_id) = uuid::Uuid::parse_str(&id_str) {
                let _ = self.job_manager.cancel(job_id).await;
            }
        }
        Ok(())
    }

    async fn cancel_jobs_where_type_and_manga(&self, job_type: &str, manga_id: i64) -> Result<()> {
        let job_ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM jobs WHERE job_type = ? AND status IN ('pending', 'running') AND json_extract(params_json, '$.manga_id') = ?"
        )
        .bind(job_type)
        .bind(manga_id)
        .fetch_all(&self.db_read)
        .await
        .unwrap_or_default();

        for id_str in job_ids {
            if let Ok(job_id) = uuid::Uuid::parse_str(&id_str) {
                let _ = self.job_manager.cancel(job_id).await;
            }
        }
        Ok(())
    }

    pub(crate) async fn build_download_task(&self, chapter_id: ChapterId) -> Result<DownloadTask> {
        let record = sqlx::query!(
            "SELECT
                c.source_chapter_id, c.name, c.chapter_number, c.volume, c.language,
                m.id as manga_id, m.source_id, m.source_manga_id, m.name as manga_name,
                m.description, s.base_url,
                (SELECT GROUP_CONCAT(p.name, ', ')
                FROM manga_people mp JOIN people p ON mp.person_id = p.id
                WHERE mp.manga_id = m.id and role = 'author') as authors,
                (SELECT GROUP_CONCAT(p.name, ', ')
                FROM manga_people mp JOIN people p ON mp.person_id = p.id
                WHERE mp.manga_id = m.id and role = 'artist') as artists,
                (SELECT GROUP_CONCAT(t.name, ', ')
                FROM manga_tags mt JOIN tags t ON mt.tag_id = t.id
                WHERE mt.manga_id = m.id) as tags
            FROM chapters c
            JOIN manga m ON c.manga_id = m.id
            JOIN sources s ON m.source_id = s.id
            WHERE c.id = ?",
            chapter_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| {
            ServiceError::NotFound(format!(
                "Chapter {chapter_id} not found (deleted after claim)"
            ))
        })?;

        #[cfg(any(test, feature = "test-util"))]
        let source_manager: Arc<dyn kani_core::downloader::PageListFetcher> = if let Some(mock) =
            self.mock_sources
                .get(&record.source_id)
                .map(|r| Arc::clone(r.value()))
        {
            mock
        } else {
            self.sources.get_backend(record.source_id).ok_or_else(|| {
                ServiceError::NotFound(format!("Source {} not found", record.source_id))
            })?
        };

        #[cfg(not(any(test, feature = "test-util")))]
        let source_manager: Arc<dyn kani_core::downloader::PageListFetcher> =
            self.sources.get_backend(record.source_id).ok_or_else(|| {
                ServiceError::NotFound(format!("Source {} not found", record.source_id))
            })?;

        let library_path = self.settings.read().await.library_path.clone();
        let name = chapter_name(record.volume, record.chapter_number, record.name.clone());
        let save_path = library_path.join(format!(
            "{} - {}",
            kani_core::utilities::sanitize_filename(&record.manga_name),
            record.manga_id
        ));

        let comic_info = kani_core::comic_info::ComicInfo {
            xmlns_xsi: "http://www.w3.org/2001/XMLSchema-instance",
            series: record.manga_name.clone(),
            title: record.name,
            number: record.chapter_number,
            volume: record.volume,
            summary: record.description,
            language_iso: Some(record.language),
            writer: record.authors,
            penciller: record.artists,
            genre: record.tags,
            web: Some(format!("{}/{}", record.base_url, record.source_manga_id)),
            pages: None,
        };

        Ok(DownloadTask {
            chapter_id: chapter_id.0,
            manga_id: record.manga_id,
            manga_title: record.manga_name.clone(),
            source_manager,
            source_manga_id: record.source_manga_id,
            source_chapter_id: record.source_chapter_id,
            name,
            library_path,
            save_path,
            comic_info: Some(comic_info),
            http_client: Some(self.smart_client_for_source(record.source_id).await),
        })
    }

    pub async fn get_download_rules(
        &self,
        manga_id: MangaId,
    ) -> Result<Vec<kani_shared::types::DownloadRule>> {
        let rows = sqlx::query_as::<_, crate::models::DownloadRuleRow>(
            "SELECT id, manga_id, rule_type, value FROM download_rules WHERE manga_id=? ORDER BY priority, id",
        )
        .bind(manga_id)
        .fetch_all(&self.db_read)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| kani_shared::types::DownloadRule::try_from(r).ok())
            .collect())
    }

    /// Inserts a download rule and returns the new row id.
    pub async fn add_download_rule(
        &self,
        manga_id: MangaId,
        kind: kani_shared::types::DownloadRuleKind,
    ) -> Result<i64> {
        use kani_shared::types::DownloadRuleKind::*;
        // Rules with a string value need validation; others use a derived string.
        let (rule_type, value): (&str, String) = match &kind {
            LanguageInclude(v) => ("language_include", v.clone()),
            LanguageExclude(v) => ("language_exclude", v.clone()),
            TitleContains(v) => ("title_contains", v.clone()),
            TitleExcludes(v) => ("title_excludes", v.clone()),
            ChapterNumberMin(n) => ("chapter_number_min", n.to_string()),
            ChapterNumberMax(n) => ("chapter_number_max", n.to_string()),
            ExcludeFractional => ("exclude_fractional", String::new()),
            MaxAgeDays(n) => ("max_age_days", n.to_string()),
            PublishedAfter(ts) => ("published_after", ts.to_string()),
        };
        // String-valued rules must be non-empty.
        match &kind {
            LanguageInclude(_) | LanguageExclude(_) | TitleContains(_) | TitleExcludes(_)
                if value.trim().is_empty() =>
            {
                return Err(ServiceError::Validation(
                    "Rule value cannot be empty".into(),
                ));
            }
            _ => {}
        }
        let id = sqlx::query_scalar!(
            "INSERT INTO download_rules (manga_id, rule_type, value) VALUES (?,?,?) RETURNING id",
            manga_id,
            rule_type,
            value
        )
        .fetch_one(&self.db)
        .await?;
        Ok(id)
    }

    pub async fn delete_download_rule(&self, id: i64) -> Result<()> {
        sqlx::query!("DELETE FROM download_rules WHERE id=?", id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn update_download_rule(
        &self,
        id: i64,
        kind: kani_shared::types::DownloadRuleKind,
    ) -> Result<()> {
        use kani_shared::types::DownloadRuleKind::*;
        let (rule_type, value): (&str, String) = match &kind {
            LanguageInclude(v) => ("language_include", v.clone()),
            LanguageExclude(v) => ("language_exclude", v.clone()),
            TitleContains(v) => ("title_contains", v.clone()),
            TitleExcludes(v) => ("title_excludes", v.clone()),
            ChapterNumberMin(n) => ("chapter_number_min", n.to_string()),
            ChapterNumberMax(n) => ("chapter_number_max", n.to_string()),
            ExcludeFractional => ("exclude_fractional", String::new()),
            MaxAgeDays(n) => ("max_age_days", n.to_string()),
            PublishedAfter(ts) => ("published_after", ts.to_string()),
        };
        match &kind {
            LanguageInclude(_) | LanguageExclude(_) | TitleContains(_) | TitleExcludes(_)
                if value.trim().is_empty() =>
            {
                return Err(ServiceError::Validation(
                    "Rule value cannot be empty".into(),
                ));
            }
            _ => {}
        }
        sqlx::query("UPDATE download_rules SET rule_type=?, value=? WHERE id=?")
            .bind(rule_type)
            .bind(value)
            .bind(id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn reorder_download_rules(
        &self,
        manga_id: MangaId,
        ordered_ids: Vec<i64>,
    ) -> Result<()> {
        for (priority, id) in ordered_ids.iter().enumerate() {
            let p = priority as i64;
            sqlx::query("UPDATE download_rules SET priority=? WHERE id=? AND manga_id=?")
                .bind(p)
                .bind(id)
                .bind(manga_id)
                .execute(&self.db)
                .await?;
        }
        Ok(())
    }

    /// Returns the N most recently downloaded chapters (download_status = 2).
    pub async fn get_download_history(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query!(
            "SELECT c.id, c.name, c.chapter_number, c.volume, c.manga_id, m.name as manga_title, c.downloaded_at
             FROM chapters c
             JOIN manga m ON m.id = c.manga_id
             WHERE c.download_status = ?
             ORDER BY c.downloaded_at DESC
             LIMIT ?",
            DownloadStatus::Complete,
            limit
        )
        .fetch_all(&self.db_read)
        .await?;

        let items = rows
            .into_iter()
            .map(|r| {
                let formatted_name = super::chapter_name(r.volume, r.chapter_number, r.name);
                serde_json::json!({
                    "id":           r.id,
                    "name":         formatted_name,
                    "chapterNumber": r.chapter_number,
                    "mangaId":      r.manga_id,
                    "mangaTitle":   r.manga_title,
                    "downloadedAt": r.downloaded_at.map(|t| t.unix_timestamp_nanos() / 1_000_000),
                    "status":       "completed",
                })
            })
            .collect();
        Ok(items)
    }

    pub(crate) async fn retry_pending_deletes(&self) -> Result<()> {
        let pending = sqlx::query!(
            "SELECT c.id, c.manga_id, m.name AS manga_name, c.name AS chapter_name, \
             c.chapter_number, c.volume, c.file_path \
             FROM chapters c JOIN manga m ON m.id = c.manga_id \
             WHERE c.delete_status = 'pending_delete' AND m.deleted_at IS NULL",
        )
        .fetch_all(&self.db_read)
        .await?;

        let library_path = self.settings.read().await.library_path.clone();

        for row in pending {
            let safe_manga = format!(
                "{} - {}",
                kani_core::utilities::sanitize_filename(&row.manga_name),
                row.manga_id
            );
            let chapter_title =
                super::chapter_name(row.volume, row.chapter_number, row.chapter_name);
            // Same rule as chapter_cbz_path: a stored path wins, because the
            // title derivation goes stale the moment a manga is renamed.
            let cbz_path = match row.file_path.as_deref().filter(|p| !p.is_empty()) {
                Some(rel) => library_path.join(rel),
                None => library_path.join(&safe_manga).join(format!(
                    "{}.cbz",
                    kani_core::utilities::sanitize_filename(&chapter_title)
                )),
            };

            match tokio::fs::remove_file(&cbz_path).await {
                Ok(_) => {
                    let _ = sqlx::query!(
                        "UPDATE chapters SET delete_status = NULL, download_status = 0 WHERE id = ?",
                        row.id
                    )
                    .execute(&self.db)
                    .await;
                    let _ = self.clear_chapter_manifest(ChapterId(row.id)).await;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let _ = sqlx::query!(
                        "UPDATE chapters SET delete_status = NULL, download_status = 0 WHERE id = ?",
                        row.id
                    )
                    .execute(&self.db)
                    .await;
                    let _ = self.clear_chapter_manifest(ChapterId(row.id)).await;
                }
                Err(e) => {
                    tracing::warn!("retry_pending_deletes: {:?}: {e}", cbz_path);
                }
            }
        }
        Ok(())
    }
}
