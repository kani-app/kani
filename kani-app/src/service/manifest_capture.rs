use std::path::{Path, PathBuf};

use kani_core::manifest::ChapterManifest;

use crate::ids::ChapterId;
use crate::service::AppService;

pub struct CapturedManifest {
    pub manifest: ChapterManifest,
    pub rel_path: Option<String>,
    pub quality_long_edge: i64,
    pub quality_bytes_per_mp: f64,
    pub quality_encoder: Option<i64>,
    pub quality_colour: String,
}

/// The stored form of a `ColourProfile`. Matches its serde representation so
/// the two cannot drift apart silently.
pub(crate) fn colour_to_column(c: kani_core::quality::ColourProfile) -> String {
    use kani_core::quality::ColourProfile::*;
    match c {
        Monochrome => "monochrome",
        ColourAccent => "colour_accent",
        FullColour => "full_colour",
        Unknown => "unknown",
    }
    .to_string()
}

pub(crate) fn colour_from_column(s: &str) -> kani_core::quality::ColourProfile {
    use kani_core::quality::ColourProfile::*;
    match s {
        "monochrome" => Monochrome,
        "colour_accent" => ColourAccent,
        "full_colour" => FullColour,
        _ => Unknown,
    }
}

/// Computes a chapter's manifest off the runtime. Hashing and decoding every
/// page is CPU-bound and a large chapter would otherwise stall the executor.
pub async fn capture(cbz_path: PathBuf, library_path: &Path) -> Option<CapturedManifest> {
    let for_blocking = cbz_path.clone();
    let manifest =
        tokio::task::spawn_blocking(move || kani_core::manifest::manifest_for_cbz(&for_blocking))
            .await
            .ok()?
            .inspect_err(|e| {
                tracing::warn!("manifest capture failed for {}: {e}", cbz_path.display())
            })
            .ok()?;

    let score = kani_core::quality::score_from_manifest(&manifest);
    let rel_path = kani_core::utilities::relative_within_root(library_path, &cbz_path);
    if rel_path.is_none() {
        tracing::warn!(
            "{} is not under the library root {} — its canonical path cannot be stored",
            cbz_path.display(),
            library_path.display()
        );
    }

    Some(CapturedManifest {
        manifest,
        rel_path,
        quality_long_edge: i64::from(score.median_long_edge_px),
        quality_bytes_per_mp: f64::from(score.bytes_per_megapixel),
        quality_encoder: score.median_encoder_quality.map(i64::from),
        quality_colour: colour_to_column(score.colour),
    })
}

impl AppService {
    /// Records content-addressing columns for a freshly downloaded chapter.
    ///
    /// Never fails the caller: a completed download that cannot be hashed stays
    /// completed with NULL columns, and the backfill or the next scrub picks it
    /// up. Losing a manifest is recoverable; losing the download is not.
    pub async fn record_chapter_manifest(&self, chapter_id: ChapterId, cbz_path: PathBuf) {
        let library_path = { self.settings.read().await.library_path.clone() };
        let Some(captured) = capture(cbz_path, &library_path).await else {
            return;
        };

        let manifest_json = match serde_json::to_string(&captured.manifest) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!("manifest serialisation failed for chapter {chapter_id}: {e}");
                return;
            }
        };
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let page_count = i64::from(captured.manifest.page_count);
        // Read before the update: a re-download whose split differs moves every
        // reader's saved position, and the old count is the only way to place it.
        let previous_page_count: Option<i64> =
            sqlx::query_scalar!("SELECT page_count FROM chapters WHERE id = ?", chapter_id)
                .fetch_optional(&self.db)
                .await
                .ok()
                .flatten()
                .flatten();
        let archive_hash = captured.manifest.archive_hash.clone();

        if let Err(e) = sqlx::query!(
            "UPDATE chapters SET file_path = ?, content_hash = ?, manifest_json = ?, \
             file_verified_at = ?, quality_long_edge = ?, quality_bytes_per_mp = ?, \
             quality_encoder = ?, quality_colour = ?, \
             page_count = ? WHERE id = ?",
            captured.rel_path,
            archive_hash,
            manifest_json,
            now,
            captured.quality_long_edge,
            captured.quality_bytes_per_mp,
            captured.quality_encoder,
            captured.quality_colour,
            page_count,
            chapter_id,
        )
        .execute(&self.db)
        .await
        {
            tracing::warn!("failed to persist manifest for chapter {chapter_id}: {e}");
            return;
        }

        if let Some(old_count) = previous_page_count
            && old_count > 0
            && old_count != page_count
        {
            self.remap_reading_progress(chapter_id, old_count, page_count)
                .await;
        }
    }

    /// Moves every saved reading position onto a chapter's new page count.
    ///
    /// A re-download can split the same chapter differently, which would
    /// otherwise leave a reader on a page that no longer holds what they read,
    /// or past the end. Best-effort, like the manifest write it follows.
    async fn remap_reading_progress(&self, chapter_id: ChapterId, old_count: i64, new_count: i64) {
        let Ok(rows) = sqlx::query!(
            "SELECT user_id, last_page_read FROM user_chapter_tracking \
             WHERE chapter_id = ? AND last_page_read > 0",
            chapter_id
        )
        .fetch_all(&self.db)
        .await
        else {
            return;
        };

        for row in rows {
            let remapped = super::quality::remap_progress(row.last_page_read, old_count, new_count);
            if remapped == row.last_page_read {
                continue;
            }
            if let Err(e) = sqlx::query!(
                "UPDATE user_chapter_tracking SET last_page_read = ? \
                 WHERE user_id = ? AND chapter_id = ?",
                remapped,
                row.user_id,
                chapter_id
            )
            .execute(&self.db)
            .await
            {
                tracing::warn!("failed to remap reading position for chapter {chapter_id}: {e}");
            }
        }
    }

    /// Submits a one-off backfill if any downloaded chapter still lacks a stored
    /// path. Guarded so a restart loop cannot queue duplicates.
    pub async fn submit_manifest_backfill_if_needed(&self) {
        let pending: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM chapters WHERE download_status = 2 AND file_path IS NULL"
        )
        .fetch_one(&self.db_read)
        .await
        .unwrap_or(0);
        if pending == 0 {
            return;
        }

        let active: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM jobs WHERE job_type = 'manifest_backfill' \
             AND status IN ('pending', 'running')"
        )
        .fetch_one(&self.db_read)
        .await
        .unwrap_or(0);
        if active > 0 {
            return;
        }

        if let Err(e) = self
            .job_manager
            .submit(crate::jobs::manifest_backfill::ManifestBackfillJob::new())
            .await
        {
            tracing::warn!("failed to submit manifest backfill: {e}");
        }
    }

    /// Clears content addressing when a chapter's file is removed, so a stale
    /// hash can never make a deleted chapter look present to the scrub.
    pub async fn clear_chapter_manifest(&self, chapter_id: ChapterId) -> crate::error::Result<()> {
        sqlx::query!(
            "UPDATE chapters SET file_path = NULL, content_hash = NULL, manifest_json = NULL, \
             file_verified_at = NULL WHERE id = ?",
            chapter_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_colour_column_round_trips() {
        for profile in [
            kani_core::quality::ColourProfile::Monochrome,
            kani_core::quality::ColourProfile::ColourAccent,
            kani_core::quality::ColourProfile::FullColour,
            kani_core::quality::ColourProfile::Unknown,
        ] {
            assert_eq!(colour_from_column(&colour_to_column(profile)), profile);
        }
    }

    #[test]
    fn an_unknown_colour_column_falls_back_rather_than_failing() {
        assert_eq!(
            colour_from_column("sepia"),
            kani_core::quality::ColourProfile::Unknown
        );
    }
}
