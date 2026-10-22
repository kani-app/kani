//! Self-describing library export.
//!
//! The point of this format is that it outlives Kani. Everything needed to read
//! the contents — the CBZs, their per-page hashes, the series metadata, and a
//! viewer — sits in the directory, and `ARCHIVE.json` carries a BLAKE3 for every
//! file so the whole thing can be re-verified without the database that produced
//! it.
//!
//! This module knows the format and nothing about the database; the caller
//! supplies already-gathered rows.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::manifest::{ChapterManifest, archive_hash};

/// Schema version written to `ARCHIVE.json`.
pub(crate) const ARCHIVE_SCHEMA: u32 = 1;

const VIEWER_HTML: &str = include_str!("archive_viewer.html");

#[derive(Debug)]
/// Caller-supplied series content used to construct a self-contained archive.
pub struct ArchiveSeries {
    pub slug: String,
    pub metadata_json: String,
    pub cover: Option<PathBuf>,
    pub chapters: Vec<ArchiveChapter>,
}

#[derive(Debug)]
/// Caller-supplied chapter archive and integrity manifest.
pub struct ArchiveChapter {
    pub number_prefix: String,
    pub slug: String,
    pub cbz_path: PathBuf,
    pub manifest: ChapterManifest,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
/// Summary of a completed archive export.
pub struct ArchiveReport {
    pub root: String,
    pub series_count: u64,
    pub chapter_count: u64,
    /// Total emitted size in bytes.
    pub total_bytes: u64,
    pub zipped: bool,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
/// Result of validating an archive index and all indexed files.
pub struct ArchiveVerifyReport {
    pub schema: u32,
    pub checked: u64,
    pub ok: u64,
    /// Relative path → what went wrong.
    pub failures: Vec<(String, String)>,
}

impl ArchiveVerifyReport {
    pub fn is_ok(&self) -> bool {
        self.failures.is_empty()
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
/// Portable `ARCHIVE.json` index at the root of an export.
pub(crate) struct ArchiveIndex {
    pub schema: u32,
    /// Export creation time as a Unix timestamp in seconds.
    pub created_at: i64,
    pub generator: String,
    pub series_count: u64,
    pub chapter_count: u64,
    /// Relative path → BLAKE3. Every emitted file appears here except
    /// `ARCHIVE.json` itself, which cannot contain its own hash.
    pub files: BTreeMap<String, String>,
    pub series: Vec<ArchiveIndexSeries>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
/// One series entry in an [`ArchiveIndex`].
pub(crate) struct ArchiveIndexSeries {
    pub slug: String,
    pub cover: Option<String>,
    pub chapters: Vec<ArchiveIndexChapter>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
/// One chapter entry in an [`ArchiveIndexSeries`].
pub(crate) struct ArchiveIndexChapter {
    pub slug: String,
    pub cbz: String,
    pub manifest: String,
    pub page_count: u32,
}

#[derive(Debug)]
/// Failure while writing, reading, or validating the portable archive format.
pub enum ArchiveError {
    Io(std::io::Error),
    Json(String),
    Malformed(String),
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Json(e) => write!(f, "json error: {e}"),
            Self::Malformed(e) => write!(f, "malformed archive: {e}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<std::io::Error> for ArchiveError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for ArchiveError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e.to_string())
    }
}

fn rel(parts: &[&str]) -> String {
    parts.join("/")
}

/// Writes the archive tree under `out`, returning what it produced.
///
/// `progress` is called with (done, total) in chapters.
pub fn write_archive(
    series: &[ArchiveSeries],
    out: &Path,
    include_viewer: bool,
    mut progress: impl FnMut(u64, u64),
) -> Result<ArchiveReport, ArchiveError> {
    std::fs::create_dir_all(out)?;

    let total: u64 = series.iter().map(|s| s.chapters.len() as u64).sum();
    let mut done = 0u64;
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut total_bytes = 0u64;
    let mut index_series = Vec::with_capacity(series.len());

    for s in series {
        let series_dir = out.join("series").join(&s.slug);
        std::fs::create_dir_all(series_dir.join("chapters"))?;

        let series_json_rel = rel(&["series", &s.slug, "series.json"]);
        std::fs::write(out.join(&series_json_rel), s.metadata_json.as_bytes())?;
        files.insert(
            series_json_rel,
            blake3::hash(s.metadata_json.as_bytes())
                .to_hex()
                .to_string(),
        );

        let cover_rel = match &s.cover {
            Some(src) if src.exists() => {
                let ext = src
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("jpg")
                    .to_ascii_lowercase();
                let r = rel(&["series", &s.slug, &format!("cover.{ext}")]);
                std::fs::copy(src, out.join(&r))?;
                total_bytes += std::fs::metadata(out.join(&r))?.len();
                files.insert(r.clone(), archive_hash(&out.join(&r)).map_err(to_arch)?);
                Some(r)
            }
            _ => None,
        };

        let mut index_chapters = Vec::with_capacity(s.chapters.len());
        for c in &s.chapters {
            let base = format!("{}-{}", c.number_prefix, c.slug);
            let cbz_rel = rel(&["series", &s.slug, "chapters", &format!("{base}.cbz")]);
            let man_rel = rel(&[
                "series",
                &s.slug,
                "chapters",
                &format!("{base}.cbz.manifest.json"),
            ]);

            std::fs::copy(&c.cbz_path, out.join(&cbz_rel))?;
            total_bytes += std::fs::metadata(out.join(&cbz_rel))?.len();
            files.insert(
                cbz_rel.clone(),
                archive_hash(&out.join(&cbz_rel)).map_err(to_arch)?,
            );

            let man_json = serde_json::to_string_pretty(&c.manifest)?;
            std::fs::write(out.join(&man_rel), man_json.as_bytes())?;
            files.insert(
                man_rel.clone(),
                blake3::hash(man_json.as_bytes()).to_hex().to_string(),
            );

            index_chapters.push(ArchiveIndexChapter {
                slug: base,
                cbz: cbz_rel,
                manifest: man_rel,
                page_count: c.manifest.page_count,
            });

            done += 1;
            progress(done, total);
        }

        index_series.push(ArchiveIndexSeries {
            slug: s.slug.clone(),
            cover: cover_rel,
            chapters: index_chapters,
        });
    }

    if include_viewer {
        std::fs::write(out.join("README.html"), VIEWER_HTML.as_bytes())?;
        files.insert(
            "README.html".to_string(),
            blake3::hash(VIEWER_HTML.as_bytes()).to_hex().to_string(),
        );
    }

    let index = ArchiveIndex {
        schema: ARCHIVE_SCHEMA,
        created_at: time::OffsetDateTime::now_utc().unix_timestamp(),
        generator: format!("kani {}", env!("CARGO_PKG_VERSION")),
        series_count: series.len() as u64,
        chapter_count: total,
        files,
        series: index_series,
    };
    let index_json = serde_json::to_string_pretty(&index)?;
    std::fs::write(out.join("ARCHIVE.json"), index_json.as_bytes())?;

    Ok(ArchiveReport {
        root: out.to_string_lossy().into_owned(),
        series_count: series.len() as u64,
        chapter_count: total,
        total_bytes,
        zipped: false,
    })
}

fn to_arch(e: crate::manifest::ManifestError) -> ArchiveError {
    ArchiveError::Io(std::io::Error::other(e.to_string()))
}

/// Re-hashes every file `ARCHIVE.json` claims, without consulting anything else.
pub fn verify_archive(root: &Path) -> Result<ArchiveVerifyReport, ArchiveError> {
    let index_path = root.join("ARCHIVE.json");
    let raw = std::fs::read_to_string(&index_path)
        .map_err(|e| ArchiveError::Malformed(format!("ARCHIVE.json unreadable: {e}")))?;
    let index: ArchiveIndex = serde_json::from_str(&raw)?;

    let mut report = ArchiveVerifyReport {
        schema: index.schema,
        ..Default::default()
    };

    for (rel_path, expected) in &index.files {
        report.checked += 1;
        let full = root.join(rel_path);
        if !full.exists() {
            report
                .failures
                .push((rel_path.clone(), "missing".to_string()));
            continue;
        }
        match archive_hash(&full) {
            Ok(actual) if &actual == expected => report.ok += 1,
            Ok(_) => report
                .failures
                .push((rel_path.clone(), "hash mismatch".to_string())),
            Err(e) => report.failures.push((rel_path.clone(), e.to_string())),
        }
    }

    Ok(report)
}

/// Wraps a written archive directory into a single `.zip` beside it.
pub fn zip_archive(root: &Path, out_zip: &Path) -> Result<u64, ArchiveError> {
    let file = std::fs::File::create(out_zip)?;
    let mut zip = zip::ZipWriter::new(file);
    // Stored, not deflated: the payload is already-compressed CBZs, so deflating
    // again costs time and saves nothing.
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = path
                .strip_prefix(root)
                .map_err(|e| ArchiveError::Malformed(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                zip.add_directory(format!("{name}/"), opts)
                    .map_err(|e| ArchiveError::Malformed(e.to_string()))?;
                stack.push(path);
            } else {
                zip.start_file(name, opts)
                    .map_err(|e| ArchiveError::Malformed(e.to_string()))?;
                let bytes = std::fs::read(&path)?;
                zip.write_all(&bytes)?;
            }
        }
    }
    zip.finish()
        .map_err(|e| ArchiveError::Malformed(e.to_string()))?;
    Ok(std::fs::metadata(out_zip)?.len())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::manifest::manifest_for_cbz;

    fn png(shade: u8) -> Vec<u8> {
        let mut img = image::GrayImage::new(8, 12);
        for (x, _y, p) in img.enumerate_pixels_mut() {
            *p = image::Luma([shade.wrapping_add(x as u8)]);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    fn write_cbz(path: &Path, shades: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (i, s) in shades.iter().enumerate() {
            zip.start_file(format!("{:04}.png", i + 1), opts).unwrap();
            zip.write_all(&png(*s)).unwrap();
        }
        zip.finish().unwrap();
    }

    fn fixture(dir: &Path) -> Vec<ArchiveSeries> {
        let mut out = Vec::new();
        for (n, title) in [(1, "alpha"), (2, "beta")] {
            let cbz = dir.join(format!("src{n}.cbz"));
            write_cbz(&cbz, &[n as u8 * 10, n as u8 * 20]);
            out.push(ArchiveSeries {
                slug: title.to_string(),
                metadata_json: format!("{{\"title\":\"{title}\"}}"),
                cover: None,
                chapters: vec![ArchiveChapter {
                    number_prefix: "0001".to_string(),
                    slug: "c1".to_string(),
                    manifest: manifest_for_cbz(&cbz).unwrap(),
                    cbz_path: cbz,
                }],
            });
        }
        out
    }

    #[test]
    fn a_written_archive_verifies_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let series = fixture(&src);
        let out = tmp.path().join("kani-archive");

        let mut seen = Vec::new();
        let report = write_archive(&series, &out, true, |d, t| seen.push((d, t))).unwrap();

        assert_eq!(report.series_count, 2);
        assert_eq!(report.chapter_count, 2);
        assert_eq!(seen, vec![(1, 2), (2, 2)], "progress must reach the total");

        let v = verify_archive(&out).unwrap();
        assert!(v.is_ok(), "fresh archive must verify: {:?}", v.failures);
        assert_eq!(v.schema, ARCHIVE_SCHEMA);
        assert!(v.checked >= 5, "index, manifests and cbzs are all hashed");
    }

    #[test]
    fn the_index_references_every_file_it_emits() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let out = tmp.path().join("kani-archive");
        write_archive(&fixture(&src), &out, true, |_, _| {}).unwrap();

        let index: ArchiveIndex =
            serde_json::from_str(&std::fs::read_to_string(out.join("ARCHIVE.json")).unwrap())
                .unwrap();

        for rel_path in index.files.keys() {
            assert!(
                out.join(rel_path).exists(),
                "{rel_path} is claimed but absent"
            );
        }
        for s in &index.series {
            for c in &s.chapters {
                assert!(index.files.contains_key(&c.cbz), "{} unhashed", c.cbz);
                assert!(
                    index.files.contains_key(&c.manifest),
                    "{} unhashed",
                    c.manifest
                );
            }
        }
        assert!(
            index.files.contains_key("README.html"),
            "the viewer must be covered too, or a tampered viewer passes verify"
        );
    }

    #[test]
    fn a_flipped_byte_is_caught_and_named() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let out = tmp.path().join("kani-archive");
        write_archive(&fixture(&src), &out, false, |_, _| {}).unwrap();

        let index: ArchiveIndex =
            serde_json::from_str(&std::fs::read_to_string(out.join("ARCHIVE.json")).unwrap())
                .unwrap();
        let victim = index.series[0].chapters[0].cbz.clone();
        let victim_path = out.join(&victim);
        let mut bytes = std::fs::read(&victim_path).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xFF;
        std::fs::write(&victim_path, &bytes).unwrap();

        let v = verify_archive(&out).unwrap();
        assert!(!v.is_ok());
        assert_eq!(
            v.failures.len(),
            1,
            "one damaged file must not implicate its neighbours"
        );
        assert_eq!(v.failures[0].0, victim);
    }

    #[test]
    fn a_deleted_file_is_reported_as_missing_not_as_a_pass() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let out = tmp.path().join("kani-archive");
        write_archive(&fixture(&src), &out, false, |_, _| {}).unwrap();

        let index: ArchiveIndex =
            serde_json::from_str(&std::fs::read_to_string(out.join("ARCHIVE.json")).unwrap())
                .unwrap();
        let victim = index.series[0].chapters[0].manifest.clone();
        std::fs::remove_file(out.join(&victim)).unwrap();

        let v = verify_archive(&out).unwrap();
        assert!(!v.is_ok());
        assert_eq!(v.failures[0], (victim, "missing".to_string()));
    }

    #[test]
    fn the_viewer_is_self_contained() {
        assert!(!VIEWER_HTML.contains("http://"), "viewer fetches over http");
        assert!(
            !VIEWER_HTML.contains("https://"),
            "viewer references a remote origin"
        );
        assert!(
            VIEWER_HTML.contains("ARCHIVE.json"),
            "viewer reads the index"
        );
    }

    #[test]
    fn a_zipped_archive_still_contains_the_index() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let out = tmp.path().join("kani-archive");
        write_archive(&fixture(&src), &out, true, |_, _| {}).unwrap();

        let zip_path = tmp.path().join("archive.zip");
        let size = zip_archive(&out, &zip_path).unwrap();
        assert!(size > 0);

        let f = std::fs::File::open(&zip_path).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "ARCHIVE.json"), "{names:?}");
        assert!(names.iter().any(|n| n.ends_with(".cbz")), "{names:?}");
    }
}
