//! Portable per-page chapter manifests and archive-integrity verification.

use std::path::Path;

/// Serialises `perceptual_hash` as 16 hex chars. JSON consumers lose precision on
/// u64, and this manifest is designed to travel (archives, Mesh transfers).
mod phash_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{value:016x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let raw = String::deserialize(d)?;
        u64::from_str_radix(&raw, 16).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
/// Integrity and quality metadata for one CBZ and each page it contains.
pub struct ChapterManifest {
    pub schema: u32,
    pub archive_hash: String,
    pub page_count: u32,
    pub pages: Vec<PageDigest>,
    /// Total uncompressed page bytes recorded in the archive.
    pub total_bytes: u64,
    /// Manifest creation time as a Unix timestamp in seconds.
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
/// Content identity and image measurements for one archive page.
pub struct PageDigest {
    pub name: String,
    pub bytes: u64,
    pub content_hash: String,
    #[serde(with = "phash_hex")]
    pub perceptual_hash: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Whether decoded pixels carry colour. `None` requires manifest backfill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_quality: Option<u8>,
}

/// Current serialized chapter-manifest schema.
pub(crate) const MANIFEST_SCHEMA: u32 = 1;

#[derive(Debug)]
/// Failure while reading an archive or constructing its manifest.
pub enum ManifestError {
    Io(std::io::Error),
    Archive(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Archive(m) => write!(f, "archive error: {m}"),
        }
    }
}

impl std::error::Error for ManifestError {}

impl From<std::io::Error> for ManifestError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Outcome of comparing an archive against a stored [`ChapterManifest`].
pub enum VerifyOutcome {
    Ok,
    ArchiveHashMismatch,
    PageMismatch { page_name: String },
    MissingPage { page_name: String },
    ExtraPage { page_name: String },
    Unreadable(String),
}

fn hash_bytes(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

/// Streaming BLAKE3 of the file as it sits on disk. Changes when the archive is
/// re-zipped even if every page is byte-identical; that is the point of having
/// both this and the per-page hashes.
pub fn archive_hash(path: &Path) -> Result<String, ManifestError> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Identity of the pages themselves, independent of how they were packed.
///
/// `archive_hash` changes whenever the archive is rebuilt, so it cannot answer
/// "same content, different zip". This folds the per-page hashes — taken over
/// decoded entries — in order, so re-zipping or recompressing the container
/// leaves it unchanged while any change to a page, or to page order, does not.
pub fn content_identity(pages: &[PageDigest]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(pages.len() as u64).to_le_bytes());
    for page in pages {
        hasher.update(page.content_hash.as_bytes());
        hasher.update(b"\0");
    }
    hasher.finalize().to_hex().to_string()
}

pub fn verify_archive_hash(path: &Path, expected: &str) -> Result<bool, ManifestError> {
    Ok(archive_hash(path)? == expected)
}

/// Builds a manifest by reading every page once: BLAKE3 over the *decoded* entry
/// bytes (so page identity survives re-zipping at a different compression level),
/// plus dimensions and a perceptual hash from the same decode pass.
///
/// A page that cannot be decoded as an image is still hashed and recorded, with
/// unknown dimensions and a zero perceptual hash — corrupt-but-present is a state
/// the scrub needs to see, not one to error out on.
pub fn manifest_for_cbz(path: &Path) -> Result<ChapterManifest, ManifestError> {
    let names =
        crate::cbz::list_cbz_pages(path).map_err(|e| ManifestError::Archive(e.to_string()))?;

    let mut pages = Vec::with_capacity(names.len());
    let mut total_bytes = 0u64;

    for (index, name) in names.iter().enumerate() {
        let (bytes, _ext) = crate::cbz::read_cbz_page(path, index)
            .map_err(|e| ManifestError::Archive(e.to_string()))?;
        total_bytes += bytes.len() as u64;

        let (width, height, perceptual_hash, colour) = match image::load_from_memory(&bytes) {
            Ok(img) => {
                let (w, h) = (img.width(), img.height());
                (
                    Some(w),
                    Some(h),
                    crate::quality::perceptual_hash_page(&img),
                    Some(crate::quality::is_colour_image(&img)),
                )
            }
            Err(_) => (None, None, 0, None),
        };

        pages.push(PageDigest {
            name: name.clone(),
            bytes: bytes.len() as u64,
            content_hash: hash_bytes(&bytes),
            perceptual_hash,
            width,
            height,
            colour,
            encoder_quality: crate::probe::jpeg_quality(&bytes),
        });
    }

    Ok(ChapterManifest {
        schema: MANIFEST_SCHEMA,
        archive_hash: archive_hash(path)?,
        page_count: pages.len() as u32,
        pages,
        total_bytes,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    })
}

/// Deep verification: re-reads every page and compares against the stored digest,
/// so a mismatch names the page that rotted rather than just the file.
pub fn verify_manifest(
    path: &Path,
    manifest: &ChapterManifest,
) -> Result<VerifyOutcome, ManifestError> {
    let names = match crate::cbz::list_cbz_pages(path) {
        Ok(n) => n,
        Err(e) => return Ok(VerifyOutcome::Unreadable(e.to_string())),
    };

    for expected in &manifest.pages {
        if !names.contains(&expected.name) {
            return Ok(VerifyOutcome::MissingPage {
                page_name: expected.name.clone(),
            });
        }
    }
    for name in &names {
        if !manifest.pages.iter().any(|p| &p.name == name) {
            return Ok(VerifyOutcome::ExtraPage {
                page_name: name.clone(),
            });
        }
    }

    for (index, name) in names.iter().enumerate() {
        let Some(expected) = manifest.pages.iter().find(|p| &p.name == name) else {
            continue;
        };
        let bytes = match crate::cbz::read_cbz_page(path, index) {
            Ok((b, _)) => b,
            Err(e) => return Ok(VerifyOutcome::Unreadable(e.to_string())),
        };
        if hash_bytes(&bytes) != expected.content_hash {
            return Ok(VerifyOutcome::PageMismatch {
                page_name: name.clone(),
            });
        }
    }

    if !verify_archive_hash(path, &manifest.archive_hash)? {
        return Ok(VerifyOutcome::ArchiveHashMismatch);
    }

    Ok(VerifyOutcome::Ok)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn png_bytes(w: u32, h: u32, shade: u8) -> Vec<u8> {
        let mut img = image::GrayImage::new(w, h);
        for (x, _y, p) in img.enumerate_pixels_mut() {
            *p = image::Luma([shade.wrapping_add((x % 255) as u8)]);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    fn build_cbz(path: &Path, pages: &[(&str, Vec<u8>)], compress: bool) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(if compress {
            zip::CompressionMethod::Deflated
        } else {
            zip::CompressionMethod::Stored
        });
        for (name, data) in pages {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    fn fixture(dir: &Path, name: &str, compress: bool) -> std::path::PathBuf {
        let p = dir.join(name);
        build_cbz(
            &p,
            &[
                ("0001.png", png_bytes(40, 60, 10)),
                ("0002.png", png_bytes(40, 60, 90)),
            ],
            compress,
        );
        p
    }

    #[test]
    fn manifest_records_every_page_with_dimensions() {
        let dir = tempfile::TempDir::new().unwrap();
        let cbz = fixture(dir.path(), "a.cbz", true);

        let m = manifest_for_cbz(&cbz).unwrap();

        assert_eq!(m.schema, MANIFEST_SCHEMA);
        assert_eq!(m.page_count, 2);
        assert_eq!(m.pages.len(), 2);
        assert!(m.total_bytes > 0);
        for p in &m.pages {
            assert_eq!(p.width, Some(40));
            assert_eq!(p.height, Some(60));
            assert_eq!(p.content_hash.len(), 64, "blake3 hex is 64 chars");
        }
        assert_ne!(
            m.pages[0].content_hash, m.pages[1].content_hash,
            "different pages must hash differently"
        );
    }

    #[test]
    fn page_hashes_survive_a_rezip_but_archive_hash_does_not() {
        let dir = tempfile::TempDir::new().unwrap();
        let stored = fixture(dir.path(), "stored.cbz", false);
        let deflated = fixture(dir.path(), "deflated.cbz", true);

        let a = manifest_for_cbz(&stored).unwrap();
        let b = manifest_for_cbz(&deflated).unwrap();

        assert_eq!(
            a.pages.iter().map(|p| &p.content_hash).collect::<Vec<_>>(),
            b.pages.iter().map(|p| &p.content_hash).collect::<Vec<_>>(),
            "decoded-entry hashes must be compression-independent"
        );
        assert_ne!(
            a.archive_hash, b.archive_hash,
            "the whole-file hash should change when the container changes"
        );
    }

    #[test]
    fn verify_passes_on_an_untouched_archive() {
        let dir = tempfile::TempDir::new().unwrap();
        let cbz = fixture(dir.path(), "ok.cbz", true);
        let m = manifest_for_cbz(&cbz).unwrap();
        assert_eq!(verify_manifest(&cbz, &m).unwrap(), VerifyOutcome::Ok);
    }

    #[test]
    fn verify_detects_a_changed_page() {
        let dir = tempfile::TempDir::new().unwrap();
        let cbz = fixture(dir.path(), "flip.cbz", true);
        let m = manifest_for_cbz(&cbz).unwrap();

        build_cbz(
            &cbz,
            &[
                ("0001.png", png_bytes(40, 60, 10)),
                ("0002.png", png_bytes(40, 60, 200)),
            ],
            true,
        );

        assert_eq!(
            verify_manifest(&cbz, &m).unwrap(),
            VerifyOutcome::PageMismatch {
                page_name: "0002.png".into()
            }
        );
    }

    #[test]
    fn verify_detects_a_removed_page() {
        let dir = tempfile::TempDir::new().unwrap();
        let cbz = fixture(dir.path(), "gone.cbz", true);
        let m = manifest_for_cbz(&cbz).unwrap();

        build_cbz(&cbz, &[("0001.png", png_bytes(40, 60, 10))], true);

        assert_eq!(
            verify_manifest(&cbz, &m).unwrap(),
            VerifyOutcome::MissingPage {
                page_name: "0002.png".into()
            }
        );
    }

    #[test]
    fn verify_detects_an_added_page() {
        let dir = tempfile::TempDir::new().unwrap();
        let cbz = fixture(dir.path(), "extra.cbz", true);
        let m = manifest_for_cbz(&cbz).unwrap();

        build_cbz(
            &cbz,
            &[
                ("0001.png", png_bytes(40, 60, 10)),
                ("0002.png", png_bytes(40, 60, 90)),
                ("0003.png", png_bytes(40, 60, 120)),
            ],
            true,
        );

        assert_eq!(
            verify_manifest(&cbz, &m).unwrap(),
            VerifyOutcome::ExtraPage {
                page_name: "0003.png".into()
            }
        );
    }

    #[test]
    fn verify_reports_a_truncated_file_as_unreadable() {
        let dir = tempfile::TempDir::new().unwrap();
        let cbz = fixture(dir.path(), "trunc.cbz", true);
        let m = manifest_for_cbz(&cbz).unwrap();

        let raw = std::fs::read(&cbz).unwrap();
        std::fs::write(&cbz, &raw[..raw.len() / 2]).unwrap();

        assert!(matches!(
            verify_manifest(&cbz, &m).unwrap(),
            VerifyOutcome::Unreadable(_)
        ));
    }

    #[test]
    fn undecodable_page_is_still_hashed_and_tracked() {
        let dir = tempfile::TempDir::new().unwrap();
        let cbz = dir.path().join("junk.cbz");
        build_cbz(&cbz, &[("0001.png", b"not-an-image".to_vec())], true);

        let m = manifest_for_cbz(&cbz).unwrap();

        assert_eq!(m.page_count, 1);
        assert_eq!(m.pages[0].width, None);
        assert_eq!(m.pages[0].perceptual_hash, 0);
        assert_eq!(m.pages[0].content_hash.len(), 64);
    }

    fn digest(name: &str, content_hash: &str) -> PageDigest {
        PageDigest {
            name: name.into(),
            bytes: 10,
            content_hash: content_hash.into(),
            perceptual_hash: 0,
            width: None,
            height: None,
            colour: None,
            encoder_quality: None,
        }
    }

    #[test]
    fn content_identity_ignores_page_names() {
        let a = [digest("0001.png", "aa"), digest("0002.png", "bb")];
        let b = [digest("page-1.jpg", "aa"), digest("page-2.jpg", "bb")];
        assert_eq!(
            content_identity(&a),
            content_identity(&b),
            "a repack may rename entries without changing the pages"
        );
    }

    #[test]
    fn content_identity_tracks_page_order() {
        let forward = [digest("1", "aa"), digest("2", "bb")];
        let reversed = [digest("1", "bb"), digest("2", "aa")];
        assert_ne!(
            content_identity(&forward),
            content_identity(&reversed),
            "page order is part of the chapter's identity"
        );
    }

    #[test]
    fn content_identity_tracks_page_count() {
        let two = [digest("1", "aa"), digest("2", "bb")];
        let three = [digest("1", "aa"), digest("2", "bb"), digest("3", "cc")];
        assert_ne!(content_identity(&two), content_identity(&three));
    }

    #[test]
    fn content_identity_separates_pages_that_would_otherwise_concatenate() {
        let split = [digest("1", "ab"), digest("2", "c")];
        let joined = [digest("1", "a"), digest("2", "bc")];
        assert_ne!(
            content_identity(&split),
            content_identity(&joined),
            "hashes must not run together into the same byte sequence"
        );
    }

    #[test]
    fn perceptual_hash_round_trips_through_json_as_hex() {
        let digest = PageDigest {
            name: "0001.png".into(),
            bytes: 10,
            content_hash: "abc".into(),
            perceptual_hash: 0xDEAD_BEEF_1234_5678,
            width: Some(1),
            height: Some(2),
            colour: None,
            encoder_quality: None,
        };
        let json = serde_json::to_string(&digest).unwrap();
        assert!(
            json.contains("\"deadbeef12345678\""),
            "should serialise as hex, got {json}"
        );
        let back: PageDigest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.perceptual_hash, digest.perceptual_hash);
    }

    #[test]
    fn unknown_future_fields_do_not_break_deserialisation() {
        let json = r#"{
            "schema": 2,
            "archive_hash": "a",
            "page_count": 0,
            "pages": [],
            "total_bytes": 0,
            "created_at": 0,
            "future_field": {"nested": true}
        }"#;
        let m: ChapterManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.schema, 2);
    }
}
