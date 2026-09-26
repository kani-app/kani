//! Upgrade detection: notices when a source now offers a better version of a
//! chapter you already hold, and replaces it on request.
//!
//! Everything here is metadata-only until the user acts. Detection never
//! downloads images; the most it does is confirm a page list, bounded per manga
//! per scan.

use crate::error::{Result, ServiceError};
use crate::ids::{ChapterId, MangaId, UserId};
use crate::service::AppService;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeKind {
    /// The source now lists more pages than the copy on disk.
    QualityReupload,
    /// The source now lists *fewer* pages. Surfaced as reassurance, never as a
    /// prompt to replace.
    SourceDowngraded,
    /// A version exists from a scanlator this manga ranks higher.
    PreferredScanlator,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpgradeCandidate {
    pub held_chapter_id: i64,
    pub kind: UpgradeKind,
    pub candidate_chapter_id: Option<i64>,
    pub candidate_source_chapter_id: String,
    pub candidate_scanlator: Option<String>,
    /// The group whose release is on disk — without it the comparison has only
    /// one side.
    pub held_scanlator: Option<String>,
    pub candidate_page_count: Option<i64>,
    pub held_page_count: Option<i64>,
    /// What the held copy actually measures, from its manifest. `None` for a
    /// chapter whose manifest predates dimension capture.
    #[serde(default)]
    pub held_score: Option<kani_core::quality::QualityScore>,
    /// What the candidate measures, from header probes. `None` when the
    /// confirmation budget was spent, the source refused range requests, or
    /// nothing readable came back.
    #[serde(default)]
    pub candidate_score: Option<kani_core::quality::QualityScore>,
    /// Which axis decided it, when a probe made a real comparison possible.
    #[serde(default)]
    pub verdict: Option<kani_core::quality::QualityVerdict>,
    pub reason_key: String,
    pub detected_at: i64,
}

/// A candidate plus the series and chapter it belongs to, for views that span
/// the whole library.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibraryUpgrade {
    pub manga_id: i64,
    pub manga_title: String,
    pub chapter_number: f64,
    pub chapter_name: Option<String>,
    pub candidate: UpgradeCandidate,
}

/// What is stored in `chapters.upgrade_available`. Dismissals live inside the
/// descriptor so a dismissed candidate survives re-scans without a side table.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct UpgradeDescriptor {
    #[serde(default)]
    pub candidates: Vec<UpgradeCandidate>,
    /// `(candidate_source_chapter_id, scanlator)` pairs the user has waved away.
    #[serde(default)]
    pub dismissed: Vec<(String, Option<String>)>,
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// Pages sampled per confirmation. Three spans the chapter without turning a
/// scan into a crawl.
const PROBE_SAMPLES: usize = 3;

fn dismissal_key(c: &UpgradeCandidate) -> (String, Option<String>) {
    (
        c.candidate_source_chapter_id.clone(),
        c.candidate_scanlator.clone(),
    )
}

impl AppService {
    /// The comparison rules as currently configured.
    pub(crate) async fn quality_policy(&self) -> kani_core::quality::QualityPolicy {
        use kani_core::quality::AxisRule;
        let s = self.settings.read().await;
        kani_core::quality::QualityPolicy {
            min_res_gain: s.upgrade_min_res_gain as f32,
            resolution: AxisRule::parse(&s.upgrade_axis_resolution),
            colour: AxisRule::parse(&s.upgrade_axis_colour),
            encoder: AxisRule::parse(&s.upgrade_axis_encoder),
            bitrate: AxisRule::parse(&s.upgrade_axis_bitrate),
        }
    }

    /// Re-derives the upgrade candidates for one manga and stores them on the
    /// affected chapter rows. Returns everything currently flagged.
    pub async fn evaluate_upgrades(&self, manga_id: MangaId) -> Result<Vec<UpgradeCandidate>> {
        let (enabled, mut confirm_budget) = {
            let s = self.settings.read().await;
            (
                s.upgrade_detection_enabled,
                s.upgrade_confirm_fetches.max(0),
            )
        };
        if !enabled {
            return Ok(Vec::new());
        }
        let policy = self.quality_policy().await;

        let source = sqlx::query!(
            "SELECT source_id, source_manga_id FROM manga WHERE id = ?",
            manga_id
        )
        .fetch_optional(&self.db_read)
        .await?;

        let rows = sqlx::query!(
            "SELECT id, source_chapter_id, chapter_number, scanlator, page_count, \
             source_page_count, download_status, manifest_json, upgrade_available, \
             quality_long_edge, quality_bytes_per_mp, quality_encoder, quality_colour \
             FROM chapters WHERE manga_id = ? ORDER BY chapter_number",
            manga_id
        )
        .fetch_all(&self.db_read)
        .await?;

        let prefs = self.effective_scanlator_prefs(manga_id).await?;
        let rank = |scanlator: &Option<String>| -> Option<i64> {
            let s = scanlator.as_deref()?;
            prefs
                .iter()
                .find(|p| p.scanlator == s && !p.blocked)
                .map(|p| p.priority)
        };

        let mut found: Vec<UpgradeCandidate> = Vec::new();

        for held in rows.iter().filter(|r| r.download_status == 2) {
            let existing: UpgradeDescriptor = held
                .upgrade_available
                .as_deref()
                .and_then(|j| serde_json::from_str(j).ok())
                .unwrap_or_default();

            let held_score = stored_score(
                held.quality_long_edge,
                held.quality_bytes_per_mp,
                held.page_count,
                held.quality_encoder,
                held.quality_colour.as_deref(),
            );
            let (held_score, held_pages) = match held_score {
                Some(score) => (Some(score), held.page_count),
                None => {
                    let m = held.manifest_json.as_deref().and_then(|j| {
                        serde_json::from_str::<kani_core::manifest::ChapterManifest>(j).ok()
                    });
                    (
                        m.as_ref().map(kani_core::quality::score_from_manifest),
                        m.as_ref().map(|m| m.page_count as i64).or(held.page_count),
                    )
                }
            };

            let mut candidates: Vec<UpgradeCandidate> = Vec::new();

            if let (Some(hp), Some(lp)) = (held_pages, held.source_page_count)
                && hp != lp
            {
                // Page count alone cannot tell better from worse — a longer listing may be the
                // same scan re-split. Spend a confirmation probe where the budget allows,
                // reading page headers rather than downloading anything.
                let mut kind = if lp > hp {
                    UpgradeKind::QualityReupload
                } else {
                    UpgradeKind::SourceDowngraded
                };
                let mut reason_key = match kind {
                    UpgradeKind::SourceDowngraded => "upgrade.reason.source_downgraded",
                    _ => "upgrade.reason.more_pages",
                }
                .to_string();
                let mut candidate_score = None;
                let mut verdict = None;

                if confirm_budget > 0
                    && let Some(src) = &source
                    && let Some(held_score) = held_score
                {
                    confirm_budget -= 1;
                    if let Some(urls) = self
                        .candidate_page_urls(
                            src.source_id,
                            &src.source_manga_id,
                            &held.source_chapter_id,
                        )
                        .await
                        && let Some(cand_score) = self
                            .probe_page_quality(src.source_id, &urls, PROBE_SAMPLES)
                            .await
                    {
                        let v =
                            kani_core::quality::compare_quality(&cand_score, &held_score, &policy);
                        kind = match v {
                            kani_core::quality::QualityVerdict::Better(_) => {
                                UpgradeKind::QualityReupload
                            }
                            _ => UpgradeKind::SourceDowngraded,
                        };
                        if let kani_core::quality::QualityVerdict::Better(reason) = v {
                            reason_key = reason.i18n_key().to_string();
                        }
                        candidate_score = Some(cand_score);
                        verdict = Some(v);
                    }
                }
                candidates.push(UpgradeCandidate {
                    held_chapter_id: held.id,
                    kind,
                    candidate_chapter_id: Some(held.id),
                    candidate_source_chapter_id: held.source_chapter_id.clone(),
                    candidate_scanlator: held.scanlator.clone(),
                    held_scanlator: held.scanlator.clone(),
                    candidate_page_count: Some(lp),
                    held_page_count: Some(hp),
                    held_score,
                    candidate_score,
                    verdict,
                    reason_key,
                    detected_at: now_unix(),
                });
            }

            // (b) A sibling at the same chapter number from a better-ranked
            // scanlator.
            let held_rank = rank(&held.scanlator);
            for other in &rows {
                if other.id == held.id
                    || (other.chapter_number - held.chapter_number).abs() > f64::EPSILON
                {
                    continue;
                }
                let Some(other_rank) = rank(&other.scanlator) else {
                    continue;
                };
                let better = match held_rank {
                    Some(hr) => other_rank > hr,
                    None => true,
                };
                if better {
                    let mut candidate_score = stored_score(
                        other.quality_long_edge,
                        other.quality_bytes_per_mp,
                        other.page_count,
                        other.quality_encoder,
                        other.quality_colour.as_deref(),
                    )
                    .or_else(|| {
                        other
                            .manifest_json
                            .as_deref()
                            .and_then(|j| {
                                serde_json::from_str::<kani_core::manifest::ChapterManifest>(j).ok()
                            })
                            .as_ref()
                            .map(kani_core::quality::score_from_manifest)
                    });

                    if candidate_score.is_none()
                        && confirm_budget > 0
                        && let Some(src) = &source
                    {
                        confirm_budget -= 1;
                        if let Some(urls) = self
                            .candidate_page_urls(
                                src.source_id,
                                &src.source_manga_id,
                                &other.source_chapter_id,
                            )
                            .await
                        {
                            candidate_score = self
                                .probe_page_quality(src.source_id, &urls, PROBE_SAMPLES)
                                .await;
                        }
                    }

                    let verdict = match (candidate_score, held_score) {
                        (Some(c), Some(h)) => {
                            Some(kani_core::quality::compare_quality(&c, &h, &policy))
                        }
                        _ => None,
                    };

                    candidates.push(UpgradeCandidate {
                        held_chapter_id: held.id,
                        kind: UpgradeKind::PreferredScanlator,
                        candidate_chapter_id: Some(other.id),
                        candidate_source_chapter_id: other.source_chapter_id.clone(),
                        candidate_scanlator: other.scanlator.clone(),
                        held_scanlator: held.scanlator.clone(),
                        candidate_page_count: other.source_page_count.or(other.page_count),
                        held_page_count: held_pages,
                        held_score,
                        candidate_score,
                        verdict,
                        reason_key: "upgrade.reason.preferred_scanlator".to_string(),
                        detected_at: now_unix(),
                    });
                }
            }

            candidates.retain(|c| !existing.dismissed.contains(&dismissal_key(c)));

            let descriptor = UpgradeDescriptor {
                candidates: candidates.clone(),
                dismissed: existing.dismissed,
            };
            self.store_descriptor(ChapterId(held.id), &descriptor)
                .await?;
            found.extend(candidates);
        }

        match self.run_auto_replace(manga_id, &found).await {
            Ok(n) if n > 0 => {
                tracing::info!("Auto-replaced {n} chapter(s) for manga {manga_id}");
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("Auto-replace failed for {manga_id}: {e}"),
        }

        Ok(found)
    }

    async fn store_descriptor(
        &self,
        chapter_id: ChapterId,
        descriptor: &UpgradeDescriptor,
    ) -> Result<()> {
        // An empty descriptor with nothing dismissed is just noise in the row.
        let json = if descriptor.candidates.is_empty() && descriptor.dismissed.is_empty() {
            None
        } else {
            serde_json::to_string(descriptor).ok()
        };
        sqlx::query!(
            "UPDATE chapters SET upgrade_available = ? WHERE id = ?",
            json,
            chapter_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Every pending candidate in the library, attributed to its series and
    /// chapter.
    ///
    /// A bare `UpgradeCandidate` names neither, so a library-wide list of them
    /// could only render a column of unlabelled Replace buttons. Anything
    /// listing upgrades across series needs this shape, not that one.
    pub async fn all_upgrades(&self) -> Result<Vec<LibraryUpgrade>> {
        let rows = sqlx::query!(
            "SELECT c.upgrade_available, c.chapter_number, c.name AS chapter_name, \
             m.id AS manga_id, COALESCE(m.local_name, m.name) AS manga_title \
             FROM chapters c \
             JOIN manga m ON m.id = c.manga_id \
             WHERE c.upgrade_available IS NOT NULL AND m.deleted_at IS NULL \
             ORDER BY manga_title, c.chapter_number"
        )
        .fetch_all(&self.db_read)
        .await?;

        // Reassurance entries are noise in a list whose whole purpose is
        // deciding what to replace: there is nothing to do about them, and they
        // pad the count. The per-chapter badge still shows them in context.
        let show_downgrades = self.settings.read().await.upgrade_show_downgrades;

        let mut out = Vec::new();
        for row in rows {
            let Some(json) = row.upgrade_available else {
                continue;
            };
            let Ok(descriptor) = serde_json::from_str::<UpgradeDescriptor>(&json) else {
                continue;
            };
            for candidate in descriptor.candidates {
                if !show_downgrades && candidate.kind == UpgradeKind::SourceDowngraded {
                    continue;
                }
                out.push(LibraryUpgrade {
                    manga_id: row.manga_id,
                    manga_title: row.manga_title.clone(),
                    chapter_number: row.chapter_number,
                    chapter_name: row.chapter_name.clone(),
                    candidate,
                });
            }
        }
        Ok(out)
    }

    /// Whether `upgrade_auto_replace` should act on this candidate, given the
    /// configured reason list.
    fn auto_replace_matches(candidate: &UpgradeCandidate, allowed: &[String]) -> bool {
        use kani_core::quality::{QualityReason, QualityVerdict};
        match candidate.kind {
            UpgradeKind::SourceDowngraded => false,
            UpgradeKind::PreferredScanlator => allowed.iter().any(|r| r == "preferred_scanlator"),
            UpgradeKind::QualityReupload => {
                // A re-upload only auto-replaces on a reason we actually measured. An
                // unconfirmed candidate — no probe, so no verdict — is a page-count difference
                // and nothing more, which is not enough to rewrite a file unattended.
                let Some(QualityVerdict::Better(reason)) = candidate.verdict else {
                    return false;
                };
                let name = match reason {
                    QualityReason::Resolution => "resolution",
                    QualityReason::Colour => "colour",
                    QualityReason::Encoder => "encoder",
                    QualityReason::Bitrate => "bitrate",
                    QualityReason::Unmeasured => return false,
                };
                allowed.iter().any(|r| r == name)
            }
        }
    }

    /// Applies every candidate the manga's auto-replace setting covers.
    ///
    /// Returns how many replacements were queued. Failures are logged rather
    /// than propagated: this runs inside a scan, and a source that cannot be
    /// reached must not fail the refresh.
    async fn run_auto_replace(
        &self,
        manga_id: MangaId,
        candidates: &[UpgradeCandidate],
    ) -> Result<u64> {
        let enabled: bool = sqlx::query_scalar!(
            "SELECT upgrade_auto_replace FROM manga WHERE id = ?",
            manga_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .unwrap_or(false);
        if !enabled {
            return Ok(0);
        }

        let allowed: Vec<String> = {
            let s = self.settings.read().await;
            s.upgrade_auto_replace_reasons
                .split(',')
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .collect()
        };
        if allowed.is_empty() {
            return Ok(0);
        }

        let mut queued = 0u64;
        let mut seen: Vec<i64> = Vec::new();
        for candidate in candidates {
            if !Self::auto_replace_matches(candidate, &allowed) {
                continue;
            }
            // One replacement per held chapter, however many candidates it
            // attracted — the second would re-queue a chapter already mid-download.
            if seen.contains(&candidate.held_chapter_id) {
                continue;
            }
            seen.push(candidate.held_chapter_id);
            match self
                .apply_upgrade(ChapterId(candidate.held_chapter_id), None)
                .await
            {
                Ok(_) => queued += 1,
                Err(e) => tracing::warn!(
                    "Auto-replace failed for chapter {}: {e}",
                    candidate.held_chapter_id
                ),
            }
        }
        Ok(queued)
    }

    /// Records every current candidate for this chapter as dismissed, so a later
    /// scan does not raise them again.
    pub async fn dismiss_upgrade(&self, chapter_id: ChapterId) -> Result<()> {
        let row = sqlx::query!(
            "SELECT upgrade_available FROM chapters WHERE id = ?",
            chapter_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Chapter {chapter_id} not found")))?;

        let mut descriptor: UpgradeDescriptor = row
            .upgrade_available
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default();

        for c in &descriptor.candidates {
            let key = dismissal_key(c);
            if !descriptor.dismissed.contains(&key) {
                descriptor.dismissed.push(key);
            }
        }
        descriptor.candidates.clear();
        self.store_descriptor(chapter_id, &descriptor).await?;
        Ok(())
    }

    pub async fn set_upgrade_auto_replace(&self, manga_id: MangaId, on: bool) -> Result<()> {
        sqlx::query!(
            "UPDATE manga SET upgrade_auto_replace = ? WHERE id = ?",
            on,
            manga_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Moves the held file into `.replaced/` and re-queues the chapter, so the
    /// existing download pipeline produces the new copy.
    ///
    /// The old file is *moved*, never deleted: `TrashPurgeJob` sweeps
    /// `.replaced/` on the same retention window as the trash, which is the only
    /// thing making this reversible.
    pub async fn apply_upgrade(
        &self,
        chapter_id: ChapterId,
        user_id: Option<UserId>,
    ) -> Result<uuid::Uuid> {
        let info = self.chapter_cbz_path(chapter_id).await?;
        let library_path = self.settings.read().await.library_path.clone();

        let hash: Option<String> =
            sqlx::query_scalar!("SELECT content_hash FROM chapters WHERE id = ?", chapter_id)
                .fetch_optional(&self.db_read)
                .await?
                .flatten();

        if info.path.exists() {
            let replaced_dir = library_path.join(".replaced");
            tokio::fs::create_dir_all(&replaced_dir).await.ok();
            let prefix = hash.as_deref().unwrap_or("nohash");
            let dest = replaced_dir.join(format!(
                "{}-{}.cbz",
                chapter_id.0,
                &prefix[..prefix.len().min(12)]
            ));
            if let Err(e) = tokio::fs::rename(&info.path, &dest).await {
                // Cross-device rename fails; fall back to copy so the old copy
                // still exists before the new download can overwrite it.
                tokio::fs::copy(&info.path, &dest).await.map_err(|_| {
                    ServiceError::Internal(format!("cannot preserve the old file: {e}"))
                })?;
                tokio::fs::remove_file(&info.path).await.ok();
            }
        }

        sqlx::query!(
            "UPDATE chapters SET download_status = 0, file_path = NULL, content_hash = NULL, \
             manifest_json = NULL, file_verified_at = NULL, upgrade_available = NULL WHERE id = ?",
            chapter_id
        )
        .execute(&self.db)
        .await?;

        self.audit(
            user_id,
            "chapter.upgrade.apply",
            Some("chapter"),
            Some(serde_json::json!({ "chapter_id": chapter_id.0 })),
        )
        .await;

        self.download_chapter(chapter_id).await
    }

    /// Removes `.replaced/` files older than the trash retention window.
    pub async fn purge_replaced(&self, retention_days: i64) -> Result<u64> {
        let library_path = self.settings.read().await.library_path.clone();
        let dir = library_path.join(".replaced");
        let cutoff = now_unix() - retention_days.max(0) * 86_400;

        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            return Ok(0);
        };
        let mut removed = 0u64;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            // `<=`, not `<`: with a retention of 0 the cutoff *is* now, and a
            // file written this second would otherwise survive a sweep that
            // was asked to remove everything.
            if modified <= cutoff && tokio::fs::remove_file(entry.path()).await.is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

impl AppService {
    /// Page URLs for a chapter as the source currently lists them.
    async fn candidate_page_urls(
        &self,
        source_id: i64,
        source_manga_id: &str,
        source_chapter_id: &str,
    ) -> Option<Vec<String>> {
        let backend = self.sources.get_backend(source_id)?;
        let chapter = backend
            .get_pages(source_manga_id, source_chapter_id)
            .await
            .ok()?;
        let raw = serde_json::to_string(&chapter).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let pages = parsed.get("pages")?.as_array()?;
        Some(
            pages
                .iter()
                .filter_map(|p| p.get("url")?.as_str().map(str::to_owned))
                .collect(),
        )
    }

    /// Reads dimensions, size and encoding of a few pages without downloading
    /// them, by range-requesting each page's header.
    ///
    /// Source listings expose page counts and URLs but not the sampled quality
    /// axes required for a pre-download comparison.
    pub async fn probe_page_quality(
        &self,
        source_id: i64,
        page_urls: &[String],
        samples: usize,
    ) -> Option<kani_core::quality::QualityScore> {
        let client = self.smart_client_for_source(source_id).await;
        use kani_core::probe::{PROBE_PREFIX_BYTES, probe_header, sample_indices};

        let mut probes = Vec::new();
        for idx in sample_indices(page_urls.len(), samples) {
            let Some(url) = page_urls.get(idx) else {
                continue;
            };
            let mut headers = rquest::header::HeaderMap::new();
            if let Ok(v) = rquest::header::HeaderValue::from_str(&format!(
                "bytes=0-{}",
                PROBE_PREFIX_BYTES - 1
            )) {
                headers.insert(rquest::header::RANGE, v);
            }

            let Ok(resp) = client.safe_get(url, Some(headers)).await else {
                continue;
            };
            let total = content_range_total(resp.headers());
            let Ok(bytes) = resp.bytes_prefix(PROBE_PREFIX_BYTES).await else {
                continue;
            };
            probes.push(probe_header(&bytes, total));
        }

        kani_core::probe::score_from_probes(&probes, page_urls.len() as u32)
    }
}

/// Total size from `Content-Range: bytes 0-4095/123456`, falling back to
/// `Content-Length` when the server served the whole file.
fn content_range_total(headers: &rquest::header::HeaderMap) -> Option<u64> {
    if let Some(cr) = headers
        .get(rquest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        && let Some((_, total)) = cr.rsplit_once('/')
        && let Ok(n) = total.trim().parse::<u64>()
    {
        return Some(n);
    }
    headers
        .get(rquest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
}

/// Maps a reading position from an old page count onto a new one.
///
/// Equal counts carry straight across. Otherwise the position is scaled and
/// clamped, so a re-upload with a different split never throws the reader back
/// to page one or past the end.
pub fn remap_progress(last_page_read: i64, old_count: i64, new_count: i64) -> i64 {
    if old_count <= 0 || new_count <= 0 {
        return 0;
    }
    if old_count == new_count {
        return last_page_read.clamp(0, new_count - 1);
    }
    let ratio = last_page_read as f64 / old_count as f64;
    ((ratio * new_count as f64).round() as i64).clamp(0, new_count - 1)
}

/// Rebuilds a `QualityScore` from the columns `manifest_capture` writes.
///
/// `None` when the row predates the columns, which sends the caller back to the
/// manifest JSON. Resolution and page count are the two that must be present:
/// a score without them cannot be compared, whereas a missing encoder estimate
/// or colour profile is an ordinary "not known" that the comparator handles.
fn stored_score(
    long_edge: Option<i64>,
    bytes_per_mp: Option<f64>,
    page_count: Option<i64>,
    encoder: Option<i64>,
    colour: Option<&str>,
) -> Option<kani_core::quality::QualityScore> {
    let long_edge = long_edge?;
    let page_count = page_count?;
    if long_edge <= 0 {
        return None;
    }
    Some(kani_core::quality::QualityScore {
        median_long_edge_px: long_edge as u32,
        bytes_per_megapixel: bytes_per_mp.unwrap_or(0.0) as f32,
        page_count: page_count as u32,
        median_encoder_quality: encoder.map(|q| q.clamp(1, 100) as u8),
        colour: colour
            .map(crate::service::manifest_capture::colour_from_column)
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn headers_with(pairs: &[(rquest::header::HeaderName, &str)]) -> rquest::header::HeaderMap {
        let mut h = rquest::header::HeaderMap::new();
        for (k, v) in pairs {
            h.insert(k.clone(), rquest::header::HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn total_size_comes_from_content_range_when_the_server_honours_it() {
        let h = headers_with(&[
            (rquest::header::CONTENT_RANGE, "bytes 0-4095/523118"),
            (rquest::header::CONTENT_LENGTH, "4096"),
        ]);
        assert_eq!(
            content_range_total(&h),
            Some(523_118),
            "Content-Length is the slice, not the page — using it would make \
             every probed page look tiny"
        );
    }

    #[test]
    fn total_size_falls_back_when_range_was_ignored() {
        let h = headers_with(&[(rquest::header::CONTENT_LENGTH, "523118")]);
        assert_eq!(content_range_total(&h), Some(523_118));
        assert_eq!(content_range_total(&rquest::header::HeaderMap::new()), None);
    }

    #[test]
    fn a_malformed_content_range_does_not_poison_the_size() {
        let h = headers_with(&[(rquest::header::CONTENT_RANGE, "bytes */unknown")]);
        assert_eq!(content_range_total(&h), None);
    }

    #[test]
    fn equal_page_counts_carry_progress_unchanged() {
        assert_eq!(remap_progress(7, 20, 20), 7);
        assert_eq!(remap_progress(0, 20, 20), 0);
    }

    #[test]
    fn a_longer_reupload_scales_the_position() {
        assert_eq!(remap_progress(10, 20, 40), 20);
    }

    #[test]
    fn progress_never_lands_past_the_end() {
        assert_eq!(
            remap_progress(19, 20, 5),
            4,
            "a shorter re-upload must clamp, not point past the last page"
        );
        assert_eq!(remap_progress(999, 20, 20), 19);
    }

    #[test]
    fn a_missing_page_count_does_not_panic() {
        assert_eq!(remap_progress(5, 0, 10), 0);
        assert_eq!(remap_progress(5, 10, 0), 0);
    }

    #[test]
    fn a_dismissed_candidate_is_matched_by_source_id_and_scanlator() {
        let base = UpgradeCandidate {
            held_chapter_id: 1,
            kind: UpgradeKind::PreferredScanlator,
            candidate_chapter_id: Some(2),
            candidate_source_chapter_id: "ch-7".into(),
            candidate_scanlator: Some("Group A".into()),
            held_scanlator: Some("Group Z".into()),
            candidate_page_count: Some(20),
            held_page_count: Some(18),
            held_score: None,
            candidate_score: None,
            verdict: None,
            reason_key: "upgrade.reason.preferred_scanlator".into(),
            detected_at: 0,
        };
        let other_group = UpgradeCandidate {
            candidate_scanlator: Some("Group B".into()),
            ..base.clone()
        };
        assert_eq!(
            dismissal_key(&base),
            ("ch-7".to_string(), Some("Group A".into()))
        );
        assert_ne!(
            dismissal_key(&base),
            dismissal_key(&other_group),
            "dismissing one group's release must not silence another's"
        );
    }
}
