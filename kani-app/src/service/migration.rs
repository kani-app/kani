use super::*;
use futures::StreamExt;

const AUTO_MATCH_THRESHOLD: f64 = 0.9;
const MAX_CANDIDATES: usize = 8;
const MATCH_CONCURRENCY: usize = 3;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationMatchQuery {
    pub manga_id: crate::ids::MangaId,
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationCandidate {
    pub id: String,
    pub title: String,
    pub cover_url: Option<String>,
    pub score: f64,
    pub in_library: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationMatch {
    pub manga_id: crate::ids::MangaId,
    pub title: String,
    pub candidates: Vec<MigrationCandidate>,
    pub best: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BulkMigrationItem {
    pub manga_id: crate::ids::MangaId,
    pub target_source_manga_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BulkMigrationSubmission {
    pub manga_id: crate::ids::MangaId,
    pub job_id: Option<crate::jobs::JobId>,
    pub error: Option<String>,
}

fn item_error(e: ServiceError) -> String {
    match e {
        ServiceError::NotFound(m)
        | ServiceError::Conflict(m)
        | ServiceError::Validation(m)
        | ServiceError::Forbidden(m) => m,
        other => other.to_string(),
    }
}

pub(super) struct MigrationContext {
    pub new_details: wit_types::MangaInfo,
    pub target_chapters: Vec<wit_types::ChapterInfo>,
    pub matched: Vec<(i64, String)>,
    pub orphaned_ids: Vec<i64>,
    pub unmatched_new: Vec<wit_types::ChapterInfo>,
    pub downloaded_orphan_ids: Vec<i64>,
    /// The target's listing hit the page ceiling, so "absent from the listing"
    /// does not mean "does not exist on the target".
    pub listing_truncated: bool,
}

impl AppService {
    pub(super) async fn resolve_migration_context(
        &self,
        manga_db_id: crate::ids::MangaId,
        target_source_id: i64,
        target_source_manga_id: &str,
    ) -> Result<MigrationContext> {
        let conflict = sqlx::query_scalar!(
            "SELECT id FROM manga WHERE source_id = ? AND source_manga_id = ?",
            target_source_id,
            target_source_manga_id
        )
        .fetch_optional(&self.db_read)
        .await?;

        if conflict.is_some() {
            return Err(ServiceError::Conflict(
                "Target manga is already in your library from this source".to_string(),
            ));
        }

        let raw = self
            .get_manga_details(target_source_id, target_source_manga_id)
            .await?;
        let new_details: wit_types::MangaInfo = serde_json::from_str(&raw)
            .map_err(|e| ServiceError::Internal(format!("Failed to parse manga details: {e}")))?;

        let (target_chapters, listing_truncated) = self
            .fetch_all_chapter_pages_checked(target_source_id, target_source_manga_id)
            .await?;

        let existing_chapters = sqlx::query!(
            "SELECT id, chapter_number, download_status FROM chapters WHERE manga_id = ?",
            manga_db_id
        )
        .fetch_all(&self.db_read)
        .await?;

        let existing_pairs: Vec<(i64, f64)> = existing_chapters
            .iter()
            .map(|c| (c.id, c.chapter_number))
            .collect();
        let (matched, orphaned_ids, unmatched_new) =
            match_chapters_inner(&existing_pairs, &target_chapters);

        let downloaded_orphan_ids: Vec<i64> = existing_chapters
            .iter()
            .filter(|c| orphaned_ids.contains(&c.id) && c.download_status == 2)
            .map(|c| c.id)
            .collect();

        Ok(MigrationContext {
            new_details,
            target_chapters,
            matched,
            orphaned_ids,
            unmatched_new,
            downloaded_orphan_ids,
            listing_truncated,
        })
    }

    pub async fn preview_migration(
        &self,
        manga_db_id: crate::ids::MangaId,
        target_source_id: i64,
        target_source_manga_id: String,
    ) -> Result<MigrationPreview> {
        let ctx = self
            .resolve_migration_context(manga_db_id, target_source_id, &target_source_manga_id)
            .await?;

        Ok(MigrationPreview {
            target_title: ctx.new_details.title,
            target_cover_url: ctx.new_details.cover_url,
            chapters_matched: ctx.matched.len(),
            chapters_orphaned: ctx.orphaned_ids.len(),
            chapters_new: ctx.unmatched_new.len(),
            downloaded_chapters_at_risk: ctx.downloaded_orphan_ids.len(),
        })
    }

    pub async fn migrate_manga(
        &self,
        manga_db_id: crate::ids::MangaId,
        target_source_id: i64,
        target_source_manga_id: String,
        keep_orphaned_downloads: bool,
    ) -> Result<MigrationResult> {
        let old_manga = sqlx::query!("SELECT name FROM manga WHERE id = ?", manga_db_id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Manga {manga_db_id} not found")))?;
        let old_manga_name = old_manga.name;

        let ctx = self
            .resolve_migration_context(manga_db_id, target_source_id, &target_source_manga_id)
            .await?;

        let MigrationContext {
            new_details,
            target_chapters,
            matched,
            orphaned_ids,
            unmatched_new,
            downloaded_orphan_ids,
            listing_truncated,
        } = ctx;

        let new_count = unmatched_new.len();

        if !keep_orphaned_downloads && !downloaded_orphan_ids.is_empty() && matched.is_empty() {
            return Err(ServiceError::Validation(format!(
                "The target source matches none of this series' {} existing chapters, so \
                 migrating would delete every download. Refused. If this is intentional, \
                 migrate with 'keep downloaded chapters' enabled.",
                downloaded_orphan_ids.len()
            )));
        }

        if !keep_orphaned_downloads && !downloaded_orphan_ids.is_empty() && listing_truncated {
            return Err(ServiceError::Validation(format!(
                "The target source's chapter listing was cut short at the page ceiling, so \
                 it cannot be told apart from a listing that genuinely lacks {} of this \
                 series' downloaded chapters. Refused rather than risk deleting them. \
                 Migrate with 'keep downloaded chapters' enabled to proceed.",
                downloaded_orphan_ids.len()
            )));
        }

        let library_path = self.settings.read().await.library_path.clone();
        let old_dir_name = format!(
            "{} - {}",
            kani_core::utilities::sanitize_filename(&old_manga_name),
            manga_db_id
        );
        let new_dir_name = format!(
            "{} - {}",
            kani_core::utilities::sanitize_filename(&new_details.title),
            manga_db_id
        );

        let non_downloaded_orphan_ids: Vec<i64> = orphaned_ids
            .iter()
            .copied()
            .filter(|id| !downloaded_orphan_ids.contains(id))
            .collect();

        let mut orphaned_cbz_paths: Vec<std::path::PathBuf> = Vec::new();
        if !keep_orphaned_downloads {
            for orphan_id in &downloaded_orphan_ids {
                let ch = sqlx::query!(
                    "SELECT name, chapter_number, volume FROM chapters WHERE id = ?",
                    orphan_id
                )
                .fetch_optional(&self.db_read)
                .await?;

                if let Some(ch) = ch {
                    let ch_name = chapter_name(ch.volume, ch.chapter_number, ch.name);
                    orphaned_cbz_paths.push(library_path.join(&old_dir_name).join(format!(
                        "{}.cbz",
                        kani_core::utilities::sanitize_filename(&ch_name)
                    )));
                }
            }
        }

        let mut tx = self.db.begin().await?;
        let status: i64 = new_details.status.into();

        sqlx::query!(
            "UPDATE manga SET source_id = ?, source_manga_id = ?, name = ?,
            cover_url = ?, description = ?, status = ? WHERE id = ?",
            target_source_id,
            target_source_manga_id,
            new_details.title,
            new_details.cover_url,
            new_details.description,
            status,
            manga_db_id
        )
        .execute(&mut *tx)
        .await?;

        // Migrating by hand is the answer to an id the resolve job could not
        // repair, so it also settles the question the flag was asking.
        sqlx::query!(
            "DELETE FROM manga_import_links WHERE manga_id = ?",
            manga_db_id
        )
        .execute(&mut *tx)
        .await?;

        for (existing_id, new_source_chapter_id) in &matched {
            let target_ch = target_chapters
                .iter()
                .find(|c| c.id == *new_source_chapter_id)
                .ok_or_else(|| {
                    ServiceError::Internal("Chapter match inconsistency during migration".into())
                })?;

            let vol: Option<i64> = target_ch.volume.map(|v| v as i64);
            sqlx::query!(
                "UPDATE chapters SET source_chapter_id = ?, name = ?, language = ?,
                scanlator = ?, uploaded_at = ?, volume = ? WHERE id = ?",
                new_source_chapter_id,
                target_ch.title,
                target_ch.language,
                target_ch.scanlator,
                target_ch.date_uploaded,
                vol,
                existing_id
            )
            .execute(&mut *tx)
            .await?;
        }
        if keep_orphaned_downloads {
            for orphan_id in &downloaded_orphan_ids {
                sqlx::query!(
                    "UPDATE chapters SET is_orphaned = 1 WHERE id = ?",
                    orphan_id
                )
                .execute(&mut *tx)
                .await?;
            }

            for orphan_id in &non_downloaded_orphan_ids {
                sqlx::query!("DELETE FROM chapters WHERE id = ?", orphan_id)
                    .execute(&mut *tx)
                    .await?;
            }
        } else {
            for orphan_id in &orphaned_ids {
                sqlx::query!("DELETE FROM chapters WHERE id = ?", orphan_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }

        for ch in &unmatched_new {
            let vol: Option<i64> = ch.volume.map(|v| v as i64);
            sqlx::query!(
                "INSERT OR IGNORE INTO chapters
                (manga_id, source_chapter_id, name, chapter_number, language,
                volume, scanlator, uploaded_at, discovered_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)",
                manga_db_id,
                ch.id,
                ch.title,
                ch.number,
                ch.language,
                vol,
                ch.scanlator,
                ch.date_uploaded
            )
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query!("DELETE FROM manga_people WHERE manga_id = ?", manga_db_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query!("DELETE FROM manga_tags WHERE manga_id = ?", manga_db_id)
            .execute(&mut *tx)
            .await?;
        Self::sync_manga_metadata(&mut tx, manga_db_id, &new_details).await?;

        tx.commit().await?;

        // The target's chapters exist now, so progress a backup parked against
        // this manga can finally be matched. Migrating by hand is the answer for
        // exactly the manga the resolve job never reaches.
        if let Err(e) = self.apply_stored_import_progress(manga_db_id).await {
            tracing::warn!("Applying imported read progress after migration failed: {e}");
        }

        // Committed: the orphan rows are gone for good, so their files can go
        // too. Any failure above returned early with every download intact.
        for cbz_path in &orphaned_cbz_paths {
            if cbz_path.exists()
                && let Err(e) = tokio::fs::remove_file(cbz_path).await
            {
                tracing::warn!("Failed to delete orphaned CBZ {:?}: {}", cbz_path, e);
            }
        }

        if old_dir_name != new_dir_name {
            let old_path = library_path.join(&old_dir_name);
            let new_path = library_path.join(&new_dir_name);
            if old_path.exists() {
                match tokio::fs::rename(&old_path, &new_path).await {
                    Ok(()) => {
                        // Migration is the one flow that actually moves files on disk, so stored
                        // paths have to follow. Exact-prefix matching rather than LIKE: a manga
                        // name can contain % or _, which LIKE would treat as wildcards.
                        let old_prefix = format!("{old_dir_name}/");
                        let new_prefix = format!("{new_dir_name}/");
                        if let Err(e) = sqlx::query!(
                            "UPDATE chapters \
                             SET file_path = ? || substr(file_path, length(?) + 1) \
                             WHERE manga_id = ? AND substr(file_path, 1, length(?)) = ?",
                            new_prefix,
                            old_prefix,
                            manga_db_id,
                            old_prefix,
                            old_prefix,
                        )
                        .execute(&self.db)
                        .await
                        {
                            tracing::warn!(
                                "Renamed {:?} → {:?} but failed to repoint stored chapter paths: {}",
                                old_path,
                                new_path,
                                e
                            );
                        }
                    }
                    Err(e) => tracing::warn!(
                        "Failed to rename library directory {:?} → {:?}: {}",
                        old_path,
                        new_path,
                        e
                    ),
                }
            }
        }

        let kept_count = if keep_orphaned_downloads {
            downloaded_orphan_ids.len()
        } else {
            0
        };
        let removed_count = if keep_orphaned_downloads {
            non_downloaded_orphan_ids.len()
        } else {
            orphaned_ids.len()
        };

        Ok(MigrationResult {
            chapters_matched: matched.len(),
            chapters_orphaned: removed_count,
            chapters_new: new_count,
            chapters_kept: kept_count,
        })
    }

    /// Queues a migration and returns the job id.
    ///
    /// Rejects a second migration of the same series while one is already
    /// pending or running: the two would race over the same chapter rows and
    /// CBZs, and the loser could delete files the winner had just re-matched.
    pub async fn submit_migration(
        &self,
        manga_db_id: crate::ids::MangaId,
        target_source_id: i64,
        target_source_manga_id: String,
        keep_orphaned_downloads: bool,
    ) -> Result<crate::jobs::JobId> {
        let job = crate::jobs::migration::MigrationJob::new(
            manga_db_id,
            target_source_id,
            target_source_manga_id,
            keep_orphaned_downloads,
        );
        self.submit_migration_job(job).await
    }

    async fn submit_migration_job(
        &self,
        job: crate::jobs::migration::MigrationJob,
    ) -> Result<crate::jobs::JobId> {
        if self.migration_job_active(job.manga_id).await {
            return Err(ServiceError::Conflict(
                "A migration for this manga is already in progress".to_string(),
            ));
        }
        self.job_manager
            .submit(job)
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))
    }

    pub async fn match_migration_targets(
        &self,
        target_source_id: i64,
        queries: Vec<MigrationMatchQuery>,
    ) -> Result<Vec<MigrationMatch>> {
        self.require_source_active(target_source_id).await?;
        Ok(futures::stream::iter(queries)
            .map(|q| self.match_migration_target(target_source_id, q))
            .buffered(MATCH_CONCURRENCY)
            .collect()
            .await)
    }

    async fn match_migration_target(
        &self,
        target_source_id: i64,
        query: MigrationMatchQuery,
    ) -> MigrationMatch {
        let manga_id = query.manga_id;
        let failed = |title: String, error: String| MigrationMatch {
            manga_id,
            title,
            candidates: Vec::new(),
            best: None,
            error: Some(error),
        };

        let title = match sqlx::query_scalar!(
            "SELECT name FROM manga WHERE id = ? AND deleted_at IS NULL",
            manga_id
        )
        .fetch_optional(&self.db_read)
        .await
        {
            Ok(Some(title)) => title,
            Ok(None) => return failed(String::new(), format!("Manga {manga_id} not found")),
            Err(e) => return failed(String::new(), item_error(e.into())),
        };

        let search = query
            .query
            .map(|q| q.trim().to_string())
            .filter(|q| !q.is_empty())
            .unwrap_or_else(|| title.clone());

        let list = match self
            .search_manga(target_source_id, &search, 1, 20, None)
            .await
            .and_then(|raw| {
                serde_json::from_str::<wit_types::MangaList>(&raw).map_err(|e| {
                    ServiceError::Internal(format!("Failed to parse search results: {e}"))
                })
            }) {
            Ok(list) => list,
            Err(e) => return failed(title, item_error(e)),
        };

        let mut candidates = Vec::with_capacity(list.manga.len());
        for item in list.manga {
            let in_library = sqlx::query_scalar!(
                "SELECT COUNT(*) FROM manga WHERE source_id = ? AND source_manga_id = ?",
                target_source_id,
                item.id
            )
            .fetch_one(&self.db_read)
            .await
            .map(|n| n > 0)
            .unwrap_or(false);
            let score = dedup::title_similarity(&title, &item.title)
                .max(dedup::title_similarity(&search, &item.title));
            candidates.push(MigrationCandidate {
                id: item.id,
                title: item.title,
                cover_url: item.cover_url,
                score,
                in_library,
            });
        }
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        candidates.truncate(MAX_CANDIDATES);

        let best = candidates
            .first()
            .filter(|c| c.score >= AUTO_MATCH_THRESHOLD && !c.in_library)
            .map(|c| c.id.clone());

        MigrationMatch {
            manga_id,
            title,
            candidates,
            best,
            error: None,
        }
    }

    pub async fn submit_bulk_migration(
        &self,
        target_source_id: i64,
        items: Vec<BulkMigrationItem>,
        keep_orphaned_downloads: bool,
    ) -> Result<Vec<BulkMigrationSubmission>> {
        let mut claimed: HashSet<String> = HashSet::new();
        let mut outcomes = Vec::with_capacity(items.len());
        for item in items {
            let manga_id = item.manga_id;
            let outcome = if claimed.contains(&item.target_source_manga_id) {
                Err(ServiceError::Conflict(
                    "Another title in this batch is already migrating to that series".to_string(),
                ))
            } else {
                let target = item.target_source_manga_id.clone();
                let job = crate::jobs::migration::MigrationJob::new(
                    manga_id,
                    target_source_id,
                    item.target_source_manga_id,
                    keep_orphaned_downloads,
                )
                .bulk();
                let submitted = self.submit_migration_job(job).await;
                if submitted.is_ok() {
                    claimed.insert(target);
                }
                submitted
            };
            outcomes.push(match outcome {
                Ok(job_id) => BulkMigrationSubmission {
                    manga_id,
                    job_id: Some(job_id),
                    error: None,
                },
                Err(e) => BulkMigrationSubmission {
                    manga_id,
                    job_id: None,
                    error: Some(item_error(e)),
                },
            });
        }
        Ok(outcomes)
    }

    pub async fn migration_statuses(
        &self,
        job_ids: Vec<crate::jobs::JobId>,
    ) -> Result<Vec<crate::jobs::manager::JobStatus>> {
        let mut statuses = Vec::with_capacity(job_ids.len());
        for job_id in job_ids {
            let id = job_id.to_string();
            let is_migration = sqlx::query_scalar!(
                "SELECT COUNT(*) FROM jobs WHERE id = ? AND job_type = 'migration'",
                id
            )
            .fetch_one(&self.db_read)
            .await?
                > 0;
            if is_migration {
                statuses.push(self.job_manager.status(job_id).await?);
            }
        }
        Ok(statuses)
    }

    async fn migration_job_active(&self, manga_id: i64) -> bool {
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM jobs WHERE job_type = 'migration' \
             AND status IN ('pending', 'running') \
             AND json_extract(params_json, '$.manga_id') = ?",
            manga_id
        )
        .fetch_one(&self.db_read)
        .await
        .map(|c| c > 0)
        .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::{AUTO_MATCH_THRESHOLD, dedup::title_similarity};

    #[test]
    fn formatting_differences_alone_still_auto_match() {
        for (library, target) in [
            ("Held Series", "The Held Series"),
            ("Kaguya-sama: Love is War", "Kaguya-sama - Love Is War"),
            ("Tomb Raider King", "Tomb Raider Kings"),
            ("The Promised Neverland (2019)", "The Promised Neverland"),
            ("Chainsaw Man", "Chainsaw Man (2018)"),
        ] {
            let score = title_similarity(library, target);
            assert!(
                score >= AUTO_MATCH_THRESHOLD,
                "{library:?} vs {target:?} scored {score}"
            );
        }
    }

    #[test]
    fn a_sequel_or_side_story_is_never_auto_matched() {
        for (library, target) in [
            ("Solo Leveling", "Solo Leveling 2"),
            ("Solo Leveling", "Solo Leveling: Ragnarok"),
            ("Held Series", "Held Series Side Story"),
            ("Omniscient Reader's Viewpoint", "Omniscient Reader"),
            (
                "That Time I Got Reincarnated as a Slime",
                "That Time I Got Reincarnated as a Slime 2",
            ),
            ("Attack on Titan Season 3", "Attack on Titan"),
            ("Hellsing (1997)", "Hellsing (2006)"),
        ] {
            let score = title_similarity(library, target);
            assert!(
                score < AUTO_MATCH_THRESHOLD,
                "{library:?} vs {target:?} scored {score}, which would migrate onto the wrong series"
            );
        }
    }

    #[test]
    fn an_empty_title_matches_nothing() {
        assert_eq!(title_similarity("", "Held Series"), 0.0);
        assert_eq!(title_similarity("!!!", "Held Series"), 0.0);
    }
}
