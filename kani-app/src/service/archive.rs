use crate::error::Result;
use crate::ids::MangaId;
use crate::service::AppService;
use kani_core::archive::{ArchiveChapter, ArchiveReport, ArchiveSeries};
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArchiveSpec {
    /// `None` exports the whole library.
    pub manga_ids: Option<Vec<MangaId>>,
    pub zip: bool,
    pub include_viewer: bool,
}

impl Default for ArchiveSpec {
    fn default() -> Self {
        Self {
            manga_ids: None,
            zip: false,
            include_viewer: true,
        }
    }
}

/// Directory-safe, collision-resistant name for a series or chapter.
fn slugify(input: &str, fallback_id: i64) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        format!("untitled-{fallback_id}")
    } else {
        // The id keeps two same-titled series apart, which matters because the
        // slug becomes a directory name.
        format!("{}-{}", &trimmed[..trimmed.len().min(60)], fallback_id)
    }
}

impl AppService {
    /// Writes a self-describing export under `{library}/_archives/<timestamp>/`.
    pub async fn export_archive(
        &self,
        spec: &ArchiveSpec,
        progress: Option<crate::jobs::framework::JobProgressReporter>,
    ) -> Result<ArchiveReport> {
        let library_path = self.settings.read().await.library_path.clone();

        let manga_rows = match &spec.manga_ids {
            Some(ids) if ids.is_empty() => Vec::new(),
            Some(ids) => {
                let mut rows = Vec::new();
                for id in ids {
                    if let Some(r) = self.archive_manga_row(id.0).await? {
                        rows.push(r);
                    }
                }
                rows
            }
            None => self.archive_all_manga_rows().await?,
        };

        let mut series = Vec::with_capacity(manga_rows.len());
        for m in manga_rows {
            let chapters = sqlx::query!(
                "SELECT id, chapter_number, name, manifest_json FROM chapters \
                 WHERE manga_id = ? AND download_status = 2 ORDER BY chapter_number",
                m.id
            )
            .fetch_all(&self.db_read)
            .await?;

            let mut out_chapters = Vec::with_capacity(chapters.len());
            for (i, c) in chapters.iter().enumerate() {
                let Ok(info) = self.chapter_cbz_path(crate::ids::ChapterId(c.id)).await else {
                    continue;
                };
                if !info.path.exists() {
                    continue;
                }

                // Prefer the manifest captured at download; recompute only when
                // the row predates the backfill, so an export never silently
                // ships a chapter with no page hashes.
                let manifest = match c
                    .manifest_json
                    .as_deref()
                    .and_then(|j| serde_json::from_str(j).ok())
                {
                    Some(m) => m,
                    None => {
                        let path = info.path.clone();
                        match tokio::task::spawn_blocking(move || {
                            kani_core::manifest::manifest_for_cbz(&path)
                        })
                        .await
                        {
                            Ok(Ok(m)) => m,
                            _ => continue,
                        }
                    }
                };

                out_chapters.push(ArchiveChapter {
                    number_prefix: format!("{:04}", i + 1),
                    slug: slugify(&info.chapter_title, c.id),
                    cbz_path: info.path,
                    manifest,
                });
            }

            if out_chapters.is_empty() {
                continue;
            }

            let cover = m
                .local_cover_path
                .as_deref()
                .filter(|p| !p.is_empty())
                .map(|p| library_path.join(p));

            series.push(ArchiveSeries {
                slug: slugify(&m.name, m.id),
                metadata_json: serde_json::to_string_pretty(&serde_json::json!({
                    "title": m.name,
                    "description": m.description,
                    "status": m.status,
                    "source": m.source_name,
                    "source_manga_id": m.source_manga_id,
                    "cover_url": m.cover_url,
                    "exported_at": time::OffsetDateTime::now_utc().unix_timestamp(),
                }))
                .unwrap_or_else(|_| "{}".to_string()),
                cover: cover.filter(|p| p.exists()),
                chapters: out_chapters,
            });
        }

        let stamp = time::OffsetDateTime::now_utc().unix_timestamp();
        let root = library_path.join("_archives").join(stamp.to_string());
        let out_dir = root.join("kani-archive");
        let include_viewer = spec.include_viewer;
        let should_zip = spec.zip;

        let mut report = tokio::task::spawn_blocking(move || {
            kani_core::archive::write_archive(&series, &out_dir, include_viewer, |done, total| {
                if let Some(p) = &progress {
                    // The writer is synchronous; hand progress back without
                    // blocking it on the async reporter.
                    let p = p.clone();
                    tokio::spawn(async move {
                        p.report(done, total, format!("Exported {done} of {total} chapters"))
                            .await;
                    });
                }
            })
        })
        .await
        .map_err(|e| crate::error::ServiceError::Internal(e.to_string()))?
        .map_err(|e| crate::error::ServiceError::Internal(e.to_string()))?;

        if should_zip {
            let dir = root.join("kani-archive");
            let zip_path = root.join("kani-archive.zip");
            let size = tokio::task::spawn_blocking(move || {
                kani_core::archive::zip_archive(&dir, &zip_path)
            })
            .await
            .map_err(|e| crate::error::ServiceError::Internal(e.to_string()))?
            .map_err(|e| crate::error::ServiceError::Internal(e.to_string()))?;
            report.zipped = true;
            report.total_bytes = size;
            report.root = root.join("kani-archive.zip").to_string_lossy().into_owned();
        }

        Ok(report)
    }

    async fn archive_manga_row(&self, id: i64) -> Result<Option<ArchiveMangaRow>> {
        let r = sqlx::query!(
            "SELECT m.id, m.name, m.description, m.status, m.cover_url, m.local_cover_path, \
             m.source_manga_id, s.name AS source_name \
             FROM manga m JOIN sources s ON s.id = m.source_id \
             WHERE m.id = ? AND m.deleted_at IS NULL",
            id
        )
        .fetch_optional(&self.db_read)
        .await?;
        Ok(r.map(|r| ArchiveMangaRow {
            id: r.id,
            name: r.name,
            description: r.description,
            status: r.status,
            cover_url: r.cover_url,
            local_cover_path: r.local_cover_path,
            source_manga_id: r.source_manga_id,
            source_name: r.source_name,
        }))
    }

    async fn archive_all_manga_rows(&self) -> Result<Vec<ArchiveMangaRow>> {
        let rows = sqlx::query!(
            "SELECT m.id, m.name, m.description, m.status, m.cover_url, m.local_cover_path, \
             m.source_manga_id, s.name AS source_name \
             FROM manga m JOIN sources s ON s.id = m.source_id \
             WHERE m.deleted_at IS NULL ORDER BY m.name"
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ArchiveMangaRow {
                id: r.id,
                name: r.name,
                description: r.description,
                status: r.status,
                cover_url: r.cover_url,
                local_cover_path: r.local_cover_path,
                source_manga_id: r.source_manga_id,
                source_name: r.source_name,
            })
            .collect())
    }

    /// Resolves a produced archive zip, refusing anything outside `_archives`.
    pub async fn archive_zip_path(&self, root: &str) -> Result<PathBuf> {
        let library_path = self.settings.read().await.library_path.clone();
        let archives = library_path.join("_archives");
        let path = PathBuf::from(root);
        let resolved = tokio::fs::canonicalize(&path)
            .await
            .map_err(|_| crate::error::ServiceError::NotFound("archive not found".into()))?;
        let root_ok = tokio::fs::canonicalize(&archives)
            .await
            .map_err(|_| crate::error::ServiceError::NotFound("no archives yet".into()))?;
        if !resolved.starts_with(&root_ok) {
            return Err(crate::error::ServiceError::NotFound(
                "archive not found".into(),
            ));
        }
        Ok(resolved)
    }
}

struct ArchiveMangaRow {
    id: i64,
    name: String,
    description: Option<String>,
    status: i64,
    cover_url: Option<String>,
    local_cover_path: Option<String>,
    source_manga_id: String,
    source_name: String,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn slugs_are_directory_safe() {
        assert_eq!(slugify("Attack on Titan", 7), "attack-on-titan-7");
        assert_eq!(slugify("Re:Zero — Vol. 1", 3), "re-zero-vol-1-3");
        assert_eq!(slugify("///", 9), "untitled-9");
    }

    #[test]
    fn same_titled_series_do_not_collide() {
        assert_ne!(
            slugify("Berserk", 1),
            slugify("Berserk", 2),
            "the slug becomes a directory; two series sharing a title would \
             otherwise overwrite each other's chapters"
        );
    }

    #[test]
    fn a_very_long_title_is_bounded() {
        let slug = slugify(&"a".repeat(500), 4);
        assert!(slug.len() < 80, "got {} chars", slug.len());
        assert!(slug.ends_with("-4"));
    }
}
