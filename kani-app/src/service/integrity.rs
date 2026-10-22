use crate::error::Result;
use crate::service::{AppService, chapter_name};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrubDepth {
    /// One streaming BLAKE3 per archive: tells you *that* a file rotted.
    Quick,
    /// Per-page verification: tells you *which* page rotted.
    Deep,
}

impl ScrubDepth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Deep => "deep",
        }
    }
}

impl std::str::FromStr for ScrubDepth {
    type Err = ();
    fn from_str(s: &str) -> std::result::Result<Self, ()> {
        match s {
            "quick" => Ok(Self::Quick),
            "deep" => Ok(Self::Deep),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ScrubReport {
    pub checked: u64,
    pub ok: u64,
    /// Verified inside the revalidation window, so not re-hashed this run.
    #[serde(default)]
    pub skipped_recently_verified: u64,
    pub orphaned_files: Vec<String>,
    pub missing_files: Vec<i64>,
    pub corrupt: Vec<(i64, String)>,
    pub path_drift: Vec<(i64, String)>,
    pub unhashed: u64,
    pub cover_mismatches: Vec<String>,
    pub exact_duplicates: Vec<Vec<i64>>,
    pub finished_at: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct OrphanDeletion {
    pub removed_count: u64,
    pub failed_count: u64,
    pub dry_run: bool,
}

/// How many archives are hashed at once. Hashing is CPU- and IO-bound and runs
/// on the blocking pool; a scheduled scrub must not starve the request path.
const HASH_CONCURRENCY: usize = 2;

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

struct ChapterRow {
    id: i64,
    manga_id: i64,
    manga_name: String,
    chapter_name: Option<String>,
    chapter_number: f64,
    volume: Option<i64>,
    file_path: Option<String>,
    content_hash: Option<String>,
    manifest_json: Option<String>,
    file_verified_at: Option<i64>,
}

/// Where a chapter's archive is expected, derived from its title. Only a
/// fallback: `file_path` is authoritative once the backfill has run.
fn derived_path(library_path: &Path, r: &ChapterRow) -> PathBuf {
    let safe_manga = format!(
        "{} - {}",
        kani_core::utilities::sanitize_filename(&r.manga_name),
        r.manga_id
    );
    let title = chapter_name(r.volume, r.chapter_number, r.chapter_name.clone());
    library_path.join(&safe_manga).join(format!(
        "{}.cbz",
        kani_core::utilities::sanitize_filename(&title)
    ))
}

impl AppService {
    /// Reconcile the database against the library on disk, verifying archive
    /// contents against the hashes captured at download time.
    ///
    /// `fix` repairs only what cannot lose data: repointing drifted paths and
    /// marking missing or corrupt chapters re-downloadable. Orphan deletion is
    /// deliberately *not* part of it — see [`AppService::delete_orphans`].
    pub async fn scrub_library(
        &self,
        depth: ScrubDepth,
        fix: bool,
        progress: Option<crate::jobs::framework::JobProgressReporter>,
    ) -> Result<ScrubReport> {
        self.scrub_library_inner(depth, fix, false, progress).await
    }

    /// `full` re-checks every chapter regardless of when it was last verified.
    ///
    /// A scheduled scrub passes `false` and skips anything verified inside the
    /// revalidation window, so a nightly run does real work on a rolling slice
    /// rather than re-hashing the whole library every time. A scrub the user
    /// asked for by hand passes `true` — having clicked "scrub now", checking
    /// only a fraction would be a surprising answer.
    pub async fn scrub_library_inner(
        &self,
        depth: ScrubDepth,
        fix: bool,
        full: bool,
        progress: Option<crate::jobs::framework::JobProgressReporter>,
    ) -> Result<ScrubReport> {
        let (library_path, revalidate_days) = {
            let s = self.settings.read().await;
            (s.library_path.clone(), s.integrity_revalidate_after_days)
        };

        let disk_files = collect_cbz_files(&library_path).await;
        let disk_set: HashSet<PathBuf> = disk_files.iter().cloned().collect();

        let verified_cutoff = if full || revalidate_days <= 0 {
            i64::MAX
        } else {
            time::OffsetDateTime::now_utc().unix_timestamp() - revalidate_days * 86_400
        };

        let raw = sqlx::query!(
            "SELECT c.id, c.manga_id, m.name AS manga_name, c.name AS chapter_name, \
             c.chapter_number, c.volume, c.file_path, c.content_hash, c.manifest_json, \
             c.file_verified_at \
             FROM chapters c \
             JOIN manga m ON m.id = c.manga_id \
             WHERE c.download_status = 2 AND m.deleted_at IS NULL"
        )
        .fetch_all(&self.db_read)
        .await?;

        let rows: Vec<ChapterRow> = raw
            .into_iter()
            .map(|r| ChapterRow {
                id: r.id,
                manga_id: r.manga_id,
                manga_name: r.manga_name,
                chapter_name: r.chapter_name,
                chapter_number: r.chapter_number,
                volume: r.volume,
                file_path: r.file_path,
                content_hash: r.content_hash,
                manifest_json: r.manifest_json,
                file_verified_at: r.file_verified_at,
            })
            .collect();

        let total = rows.len() as u64;
        let mut report = ScrubReport {
            checked: total,
            ..Default::default()
        };

        let mut expected: HashSet<PathBuf> = HashSet::new();
        let mut verifiable: Vec<(i64, PathBuf, Option<String>, Option<String>)> = Vec::new();

        for r in &rows {
            let derived = derived_path(&library_path, r);
            // `file_path` is stored relative to the library root, while the disk walk and the
            // derived path are both rooted. Comparing the stored string directly would match
            // nothing and report every chapter as drifted.
            let stored = r
                .file_path
                .as_deref()
                .filter(|p| !p.is_empty())
                .map(|rel| library_path.join(rel));

            // The stored path is authoritative; fall back to the derived one so
            // rows the backfill has not reached are still checkable.
            let (actual, drifted) = match &stored {
                Some(p) if disk_set.contains(p) => (Some(p.clone()), false),
                Some(_) if disk_set.contains(&derived) => (Some(derived.clone()), true),
                Some(_) => (None, false),
                None if disk_set.contains(&derived) => (Some(derived.clone()), false),
                None => (None, false),
            };

            match actual {
                None => report.missing_files.push(r.id),
                Some(path) => {
                    if drifted {
                        // Report the relative form: this value is written straight
                        // back into `file_path`, which resolution re-joins against
                        // the library root.
                        let relative =
                            kani_core::utilities::relative_within_root(&library_path, &path)
                                .unwrap_or_else(|| path.to_string_lossy().into_owned());
                        report.path_drift.push((r.id, relative));
                    }
                    expected.insert(path.clone());
                    if r.file_verified_at.is_some_and(|t| t > verified_cutoff) {
                        report.skipped_recently_verified += 1;
                    } else {
                        verifiable.push((
                            r.id,
                            path,
                            r.content_hash.clone(),
                            r.manifest_json.clone(),
                        ));
                    }
                }
            }
            expected.insert(derived);
            if let Some(p) = stored {
                expected.insert(p);
            }
        }

        report.orphaned_files = disk_files
            .iter()
            .filter(|p| !expected.contains(*p))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();

        let mut verified_ids: Vec<i64> = Vec::new();
        let mut done = 0u64;

        for batch in verifiable.chunks(HASH_CONCURRENCY) {
            let mut tasks = Vec::with_capacity(batch.len());
            for (id, path, hash, manifest) in batch {
                let (id, path, hash, manifest) =
                    (*id, path.clone(), hash.clone(), manifest.clone());
                tasks.push(tokio::task::spawn_blocking(move || {
                    (
                        id,
                        verify_one(depth, &path, hash.as_deref(), manifest.as_deref()),
                    )
                }));
            }
            for task in tasks {
                let Ok((id, outcome)) = task.await else {
                    continue;
                };
                match outcome {
                    Verified::Ok => {
                        report.ok += 1;
                        verified_ids.push(id);
                    }
                    Verified::Unhashed => report.unhashed += 1,
                    Verified::Bad(reason) => report.corrupt.push((id, reason)),
                }
                done += 1;
            }
            if let Some(p) = &progress {
                p.report(done, total, format!("Verified {done} of {total} chapters"))
                    .await;
            }
        }

        report.cover_mismatches = self.missing_covers(&library_path).await?;
        report.exact_duplicates = self.exact_duplicate_groups().await?;
        report.finished_at = now_unix();

        if !verified_ids.is_empty() {
            self.mark_verified(&verified_ids).await?;
        }
        if fix {
            self.apply_scrub_fixes(&report).await?;
        }
        self.persist_scrub_report(depth, &report).await?;

        Ok(report)
    }

    async fn missing_covers(&self, library_path: &Path) -> Result<Vec<String>> {
        let rows = sqlx::query!(
            "SELECT local_cover_path FROM manga \
             WHERE local_cover_path IS NOT NULL AND deleted_at IS NULL"
        )
        .fetch_all(&self.db_read)
        .await?;

        let mut missing = Vec::new();
        for row in rows {
            if let Some(rel) = row.local_cover_path {
                let full = library_path.join(&rel);
                if tokio::fs::metadata(&full).await.is_err() {
                    missing.push(full.to_string_lossy().into_owned());
                }
            }
        }
        Ok(missing)
    }

    /// Chapters holding the same pages, grouped. Report-only: this complements
    /// the title-similarity dedup in `service::dedup`, which answers a
    /// different question and stays as it is.
    ///
    /// Keyed on page identity rather than `content_hash`, so the same pages
    /// re-zipped still group. Chapters without a manifest fall back to
    /// `content_hash`, which only matches a byte-identical archive.
    async fn exact_duplicate_groups(&self) -> Result<Vec<Vec<i64>>> {
        let rows = sqlx::query!(
            "SELECT content_hash, manifest_json, id FROM chapters \
             WHERE content_hash IS NOT NULL AND download_status = 2 \
             ORDER BY content_hash, id"
        )
        .fetch_all(&self.db_read)
        .await?;

        let mut groups: HashMap<String, Vec<i64>> = HashMap::new();
        for row in rows {
            let pages_key = row
                .manifest_json
                .as_deref()
                .and_then(|json| {
                    serde_json::from_str::<kani_core::manifest::ChapterManifest>(json).ok()
                })
                .map(|manifest| kani_core::manifest::content_identity(&manifest.pages));
            let key = match (pages_key, row.content_hash) {
                (Some(pages), _) => pages,
                (None, Some(archive)) => archive,
                (None, None) => continue,
            };
            groups.entry(key).or_default().push(row.id);
        }
        let mut out: Vec<Vec<i64>> = groups.into_values().filter(|g| g.len() > 1).collect();
        out.sort();
        Ok(out)
    }

    async fn mark_verified(&self, ids: &[i64]) -> Result<()> {
        let now = now_unix();
        let mut tx = self.db.begin().await?;
        for id in ids {
            sqlx::query!(
                "UPDATE chapters SET file_verified_at = ? WHERE id = ?",
                now,
                id
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Only the repairs that cannot destroy anything: repoint a moved file, and
    /// mark a gone-or-rotten chapter re-downloadable.
    async fn apply_scrub_fixes(&self, report: &ScrubReport) -> Result<()> {
        let mut tx = self.db.begin().await?;

        for (id, path) in &report.path_drift {
            sqlx::query!("UPDATE chapters SET file_path = ? WHERE id = ?", path, id)
                .execute(&mut *tx)
                .await?;
        }
        for id in &report.missing_files {
            sqlx::query!(
                "UPDATE chapters SET download_status = 0, file_path = NULL, content_hash = NULL, \
                 manifest_json = NULL, file_verified_at = NULL WHERE id = ?",
                id
            )
            .execute(&mut *tx)
            .await?;
        }
        for (id, _) in &report.corrupt {
            sqlx::query!(
                "UPDATE chapters SET download_status = 0, content_hash = NULL, \
                 manifest_json = NULL, file_verified_at = NULL WHERE id = ?",
                id
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn persist_scrub_report(&self, depth: ScrubDepth, report: &ScrubReport) -> Result<()> {
        let json = serde_json::to_string(report).unwrap_or_else(|_| "{}".to_string());
        let depth_s = depth.as_str();
        sqlx::query!(
            "INSERT INTO scrub_reports (depth, report_json) VALUES (?, ?)",
            depth_s,
            json
        )
        .execute(&self.db)
        .await?;
        sqlx::query!(
            "DELETE FROM scrub_reports WHERE id NOT IN \
             (SELECT id FROM scrub_reports ORDER BY created_at DESC, id DESC LIMIT 20)"
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn last_scrub_report(&self) -> Result<Option<(String, ScrubReport, i64)>> {
        let row = sqlx::query!(
            "SELECT depth, report_json, created_at FROM scrub_reports \
             ORDER BY created_at DESC, id DESC LIMIT 1"
        )
        .fetch_optional(&self.db_read)
        .await?;

        Ok(row.and_then(|r| {
            serde_json::from_str::<ScrubReport>(&r.report_json)
                .ok()
                .map(|report| (r.depth, report, r.created_at))
        }))
    }

    /// Delete orphaned archives. Separate from `scrub_library` on purpose: a
    /// scheduled scrub must never be able to remove a file, so removal is an
    /// explicit call naming the exact paths, with a dry run available.
    pub async fn delete_orphans(&self, paths: &[String], dry_run: bool) -> Result<OrphanDeletion> {
        let library_path = self.settings.read().await.library_path.clone();
        let root = tokio::fs::canonicalize(&library_path)
            .await
            .unwrap_or_else(|_| library_path.clone());

        let mut removed_count = 0u64;
        let mut failed_count = 0u64;

        for path_str in paths {
            let path = PathBuf::from(path_str);
            // A caller-supplied path must not be able to reach outside the
            // library, whatever it claims to be.
            let resolved = tokio::fs::canonicalize(&path).await;
            let inside = matches!(&resolved, Ok(p) if p.starts_with(&root));
            let is_cbz = path.extension().is_some_and(|e| e == "cbz");
            if !inside || !is_cbz {
                failed_count += 1;
                tracing::warn!("delete_orphans: refusing {path_str}");
                continue;
            }
            if dry_run {
                removed_count += 1;
                continue;
            }
            match tokio::fs::remove_file(&path).await {
                Ok(()) => removed_count += 1,
                Err(e) => {
                    tracing::warn!("delete_orphans: failed to remove {path_str}: {e}");
                    failed_count += 1;
                }
            }
        }

        Ok(OrphanDeletion {
            removed_count,
            failed_count,
            dry_run,
        })
    }
}

enum Verified {
    Ok,
    Unhashed,
    Bad(String),
}

fn verify_one(
    depth: ScrubDepth,
    path: &Path,
    content_hash: Option<&str>,
    manifest_json: Option<&str>,
) -> Verified {
    use kani_core::manifest::{VerifyOutcome, verify_archive_hash, verify_manifest};

    if depth == ScrubDepth::Deep
        && let Some(json) = manifest_json
        && let Ok(manifest) = serde_json::from_str(json)
    {
        return match verify_manifest(path, &manifest) {
            Ok(VerifyOutcome::Ok) => Verified::Ok,
            Ok(other) => Verified::Bad(format!("{other:?}")),
            Err(e) => Verified::Bad(e.to_string()),
        };
    }

    let Some(expected) = content_hash else {
        return Verified::Unhashed;
    };
    match verify_archive_hash(path, expected) {
        Ok(true) => Verified::Ok,
        Ok(false) => Verified::Bad("ArchiveHashMismatch".to_string()),
        Err(e) => Verified::Bad(e.to_string()),
    }
}

async fn collect_cbz_files(library_path: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let Ok(mut manga_dirs) = tokio::fs::read_dir(library_path).await else {
        return result;
    };
    while let Ok(Some(entry)) = manga_dirs.next_entry().await {
        let Ok(ft) = entry.file_type().await else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        if entry.file_name() == "covers" {
            continue;
        }
        let manga_dir = entry.path();
        let Ok(mut chapter_files) = tokio::fs::read_dir(&manga_dir).await else {
            continue;
        };
        while let Ok(Some(f)) = chapter_files.next_entry().await {
            let name = f.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".cbz") {
                result.push(f.path());
            }
        }
    }
    result
}
