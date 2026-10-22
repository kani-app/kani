//! CBZ archive reading utilities.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use image::GenericImageView;

use crate::error::{Error, Result};

fn is_image_entry(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
        || lower.ends_with(".avif")
}

fn open_archive(path: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    let file = std::fs::File::open(path)
        .map_err(|_| Error::NotFound(format!("CBZ not found: {}", path.display())))?;
    zip::ZipArchive::new(file).map_err(|e| Error::Internal(format!("Failed to open CBZ: {e}")))
}

fn sorted_image_names(archive: &mut zip::ZipArchive<std::fs::File>) -> Vec<String> {
    let mut names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_owned();
            is_image_entry(&name).then_some(name)
        })
        .collect();
    names.sort();
    names
}

/// Maps a lowercase file extension (without dot) to its image content type.
pub(crate) fn content_type_for_ext(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

/// Returns a sorted list of image entry names from a CBZ archive.
///
/// Entries are sorted lexicographically, which matches the `0001.jpg` naming
/// convention used by the downloader.
pub fn list_cbz_pages(path: &Path) -> Result<Vec<String>> {
    let mut archive = open_archive(path)?;
    Ok(sorted_image_names(&mut archive))
}

/// Reads a specific page by sorted index from a CBZ archive.
///
/// Returns the raw image bytes and the lowercase file extension (without dot).
/// Opens the archive once, builds the sorted page list, then reads the entry.
pub fn read_cbz_page(path: &Path, page_num: usize) -> Result<(Vec<u8>, String)> {
    let mut archive = open_archive(path)?;
    let names = sorted_image_names(&mut archive);

    let name = names
        .get(page_num)
        .ok_or_else(|| {
            Error::NotFound(format!(
                "Page {page_num} out of range ({} pages)",
                names.len()
            ))
        })?
        .clone();

    let ext = Path::new(&name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg")
        .to_lowercase();

    let mut entry = archive
        .by_name(&name)
        .map_err(|_| Error::Internal(format!("Entry '{name}' missing from CBZ")))?;

    let mut buf = Vec::with_capacity(entry.size() as usize);
    std::io::Read::read_to_end(&mut entry, &mut buf)
        .map_err(|e| Error::Internal(format!("CBZ read error: {e}")))?;

    Ok((buf, ext))
}

/// Reads a CBZ page, optionally downscaling to `max_width` and/or re-encoding to `format`.
///
/// When `max_width == 0` and `format` is `None`, returns the stored bytes verbatim
/// (zero decode cost). Otherwise decodes with dimension limits (defusing decode bombs),
/// downscales with Lanczos3 only when wider than `max_width`, and re-encodes as JPEG
/// (quality 85) or WebP. Any other target format is rejected.
pub fn read_cbz_page_transcoded(
    path: &Path,
    page_num: usize,
    max_width: u32,
    format: Option<image::ImageFormat>,
) -> Result<(Vec<u8>, &'static str)> {
    if max_width == 0 && format.is_none() {
        let (bytes, ext) = read_cbz_page(path, page_num)?;
        return Ok((bytes, content_type_for_ext(&ext)));
    }

    let (bytes, _ext) = read_cbz_page(path, page_num)?;

    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);

    let mut reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| Error::Internal(format!("image format guess failed: {e}")))?;
    reader.limits(limits);
    let mut img = reader
        .decode()
        .map_err(|e| Error::Internal(format!("image decode failed: {e}")))?;

    if max_width > 0 && img.width() > max_width {
        img = img.resize(max_width, u32::MAX, image::imageops::FilterType::Lanczos3);
    }

    let out_format = format.unwrap_or(image::ImageFormat::Jpeg);
    let mut out = std::io::Cursor::new(Vec::new());
    match out_format {
        image::ImageFormat::Jpeg => {
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
            img.write_with_encoder(enc)
                .map_err(|e| Error::Internal(format!("jpeg encode failed: {e}")))?;
            Ok((out.into_inner(), "image/jpeg"))
        }
        image::ImageFormat::WebP => {
            img.write_to(&mut out, image::ImageFormat::WebP)
                .map_err(|e| Error::Internal(format!("webp encode failed: {e}")))?;
            Ok((out.into_inner(), "image/webp"))
        }
        other => Err(Error::Internal(format!(
            "unsupported transcode format: {other:?}"
        ))),
    }
}

/// Read the raw UTF-8 content of `ComicInfo.xml` from a CBZ archive, if present.
pub(crate) fn read_cbz_comic_info(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("ComicInfo.xml").ok()?;
    let mut buf = String::new();
    std::io::Read::read_to_string(&mut entry, &mut buf).ok()?;
    Some(buf)
}

/// Returns `(double_page_indices, spread_analysed)` from a CBZ's `ComicInfo.xml`.
pub fn read_double_page_flags(path: &Path) -> (HashSet<usize>, bool) {
    let Some(xml) = read_cbz_comic_info(path) else {
        return (HashSet::new(), false);
    };
    // Strip the XML declaration so the deserialiser sees a bare element.
    let body = xml
        .strip_prefix("<?xml version=\"1.0\" encoding=\"utf-8\"?>")
        .unwrap_or(&xml)
        .trim();
    let analysed = crate::comic_info::has_pages_metadata(body);
    let flags = crate::comic_info::parse_double_pages(body)
        .into_iter()
        .map(|i| i as usize)
        .collect();
    (flags, analysed)
}

const STRIP_W: u32 = 32;
const SAMPLE_H: u32 = 64;
const VARIANCE_THRESHOLD: f64 = 200.0;
const DIFF_THRESHOLD: f64 = 20.0;

/// Returns 0-based indices that should be flagged `DoublePage=true` in `ComicInfo.xml`.
/// Wide (w/h ≥ 1.2) images are flagged directly; portrait pairs are confirmed via pixel edge comparison.
pub(crate) fn detect_spread_pages(page_paths: &[PathBuf]) -> HashSet<usize> {
    let mut double_pages = HashSet::new();
    let mut candidate: Option<(usize, PathBuf, u32, u32)> = None;

    for (i, path) in page_paths.iter().enumerate() {
        let (w, h) = match imagesize::size(path) {
            Ok(dim) => (dim.width as u32, dim.height as u32),
            Err(e) => {
                tracing::warn!(
                    "detect_spread_pages: could not read dims for {}: {e}",
                    path.display()
                );
                candidate = None;
                continue;
            }
        };

        if h == 0 {
            candidate = None;
            continue;
        }

        if w as f64 / h as f64 >= 1.2 {
            double_pages.insert(i);
            candidate = None;
            continue;
        }

        let plausible_pair = candidate.as_ref().is_some_and(|(_, _, cw, ch)| {
            let ratio = (*cw + w) as f64 / (*ch).max(h) as f64;
            (1.2..=2.5).contains(&ratio)
        });

        if plausible_pair {
            let (prev_idx, prev_path, _, _) = candidate.take().expect("checked above");

            let matched = image::open(&prev_path)
                .ok()
                .zip(image::open(path).ok())
                .map(|(img_a, img_b)| {
                    pixel_edges_adjacent(&img_a, &img_b) || pixel_edges_adjacent(&img_b, &img_a)
                })
                .unwrap_or(false);

            if matched {
                double_pages.insert(prev_idx);
            } else {
                candidate = Some((i, path.clone(), w, h));
            }
        } else {
            candidate = Some((i, path.clone(), w, h));
        }
    }

    double_pages
}

fn pixel_edges_adjacent(left_img: &image::DynamicImage, right_img: &image::DynamicImage) -> bool {
    let (aw, ah) = left_img.dimensions();
    let (bw, bh) = right_img.dimensions();

    if aw < STRIP_W || bw < STRIP_W || ah == 0 || bh == 0 {
        return false;
    }

    let strip_a = left_img
        .crop_imm(aw - STRIP_W, 0, STRIP_W, ah)
        .resize_exact(STRIP_W, SAMPLE_H, image::imageops::FilterType::Nearest)
        .into_rgb8();

    let strip_b = right_img
        .crop_imm(0, 0, STRIP_W, bh)
        .resize_exact(STRIP_W, SAMPLE_H, image::imageops::FilterType::Nearest)
        .into_rgb8();

    let pixel_count = (STRIP_W * SAMPLE_H) as f64;
    let channel_count = pixel_count * 3.0;

    let strip_variance = |strip: &image::ImageBuffer<image::Rgb<u8>, Vec<u8>>| -> f64 {
        let mut sum = [0f64; 3];
        for px in strip.pixels() {
            sum[0] += px[0] as f64;
            sum[1] += px[1] as f64;
            sum[2] += px[2] as f64;
        }
        let mean = [
            sum[0] / pixel_count,
            sum[1] / pixel_count,
            sum[2] / pixel_count,
        ];
        let mut var = 0f64;
        for px in strip.pixels() {
            let dr = px[0] as f64 - mean[0];
            let dg = px[1] as f64 - mean[1];
            let db = px[2] as f64 - mean[2];
            var += dr * dr + dg * dg + db * db;
        }
        var / channel_count
    };

    if strip_variance(&strip_a) < VARIANCE_THRESHOLD
        || strip_variance(&strip_b) < VARIANCE_THRESHOLD
    {
        return false;
    }

    let mut diff = 0f64;
    for y in 0..SAMPLE_H {
        for x in 0..STRIP_W {
            let pa = strip_a.get_pixel(STRIP_W - 1 - x, y);
            let pb = strip_b.get_pixel(x, y);
            diff += (pa[0] as f64 - pb[0] as f64).abs()
                + (pa[1] as f64 - pb[1] as f64).abs()
                + (pa[2] as f64 - pb[2] as f64).abs();
        }
    }
    diff / channel_count < DIFF_THRESHOLD
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    fn make_cbz(dir: &TempDir, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = dir.path().join(name);
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        for (entry_name, data) in entries {
            writer.start_file(*entry_name, opts).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    fn make_png(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
        let img = image::ImageBuffer::from_pixel(w, h, image::Rgb([r, g, b]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    /// Produces high variance while keeping the two center-edge strips identical.
    fn make_gradient_png(w: u32, h: u32) -> Vec<u8> {
        let img = image::ImageBuffer::from_fn(w, h, |_x, y| {
            let v = (y * 255 / h.max(1)) as u8;
            image::Rgb([v, 0u8, 128u8])
        });
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    /// Adds deterministic compression-like noise without crossing the spread threshold.
    fn make_noisy_gradient_png(w: u32, h: u32, amplitude: u8) -> Vec<u8> {
        let a = amplitude as i16;
        let range = (2 * a + 1) as u64;
        let img = image::ImageBuffer::from_fn(w, h, |x, y| {
            let mut v = 0xdead_beef_cafe_babeu64
                .wrapping_add((x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
                .wrapping_add((y as u64).wrapping_mul(0x6c62_272e_07bb_0142));
            v ^= v >> 30;
            v = v.wrapping_mul(0xbf58_476d_1ce4_e5b9);
            v ^= v >> 27;
            v = v.wrapping_mul(0x94d0_49bb_1331_11eb);
            v ^= v >> 31;
            let noise = |bits: u64| -> i16 { (bits % range) as i16 - a };
            let gradient_r = (y * 255 / h.max(1)) as i16;
            let mid_g = 128i16;
            let mid_b = 128i16;
            image::Rgb([
                (gradient_r + noise(v)).clamp(0, 255) as u8,
                (mid_g + noise(v >> 8)).clamp(0, 255) as u8,
                (mid_b + noise(v >> 16)).clamp(0, 255) as u8,
            ])
        });
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    /// Encode a PNG where every pixel has a unique deterministic colour derived
    /// from its coordinates and a seed.  No spatial correlation — adjacent
    /// columns look completely different.  Simulates a digital manga page: high
    /// variance everywhere, no matching edges with any unrelated page.
    fn make_noise_png(w: u32, h: u32, seed: u64) -> Vec<u8> {
        let img = image::ImageBuffer::from_fn(w, h, |x, y| {
            let mut v = seed
                .wrapping_add((x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
                .wrapping_add((y as u64).wrapping_mul(0x6c62_272e_07bb_0142));
            v ^= v >> 30;
            v = v.wrapping_mul(0xbf58_476d_1ce4_e5b9);
            v ^= v >> 27;
            v = v.wrapping_mul(0x94d0_49bb_1331_11eb);
            v ^= v >> 31;
            image::Rgb([v as u8, (v >> 8) as u8, (v >> 16) as u8])
        });
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn lists_only_image_entries() {
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(
            &dir,
            "test.cbz",
            &[
                ("0001.jpg", b"fake-jpg"),
                ("0002.png", b"fake-png"),
                ("0003.webp", b"fake-webp"),
                ("ComicInfo.xml", b"<ComicInfo/>"),
                ("readme.txt", b"ignore me"),
            ],
        );
        let pages = list_cbz_pages(&cbz).unwrap();
        assert_eq!(pages, vec!["0001.jpg", "0002.png", "0003.webp"]);
    }

    #[test]
    fn pages_are_sorted_lexicographically() {
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(
            &dir,
            "sorted.cbz",
            &[("0003.jpg", b"c"), ("0001.jpg", b"a"), ("0002.jpg", b"b")],
        );
        let pages = list_cbz_pages(&cbz).unwrap();
        assert_eq!(pages, vec!["0001.jpg", "0002.jpg", "0003.jpg"]);
    }

    #[test]
    fn reads_page_content_by_index() {
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(
            &dir,
            "read.cbz",
            &[("0001.jpg", b"image-data-1"), ("0002.png", b"image-data-2")],
        );
        let (data, ext) = read_cbz_page(&cbz, 0).unwrap();
        assert_eq!(data, b"image-data-1");
        assert_eq!(ext, "jpg");

        let (data2, ext2) = read_cbz_page(&cbz, 1).unwrap();
        assert_eq!(data2, b"image-data-2");
        assert_eq!(ext2, "png");
    }

    #[test]
    fn out_of_bounds_page_returns_error() {
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(&dir, "oob.cbz", &[("0001.jpg", b"data")]);
        let err = read_cbz_page(&cbz, 5).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }

    #[test]
    fn missing_file_returns_not_found() {
        let err = list_cbz_pages(std::path::Path::new("/nonexistent/path.cbz")).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }

    #[test]
    fn empty_archive_has_no_pages() {
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(&dir, "empty.cbz", &[("readme.txt", b"nothing here")]);
        let pages = list_cbz_pages(&cbz).unwrap();
        assert!(pages.is_empty());
    }

    #[test]
    fn case_insensitive_extension_matching() {
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(
            &dir,
            "caps.cbz",
            &[
                ("0001.JPG", b"uppercase"),
                ("0002.JPEG", b"also-jpeg"),
                ("0003.PNG", b"uppercase-png"),
            ],
        );
        let pages = list_cbz_pages(&cbz).unwrap();
        assert_eq!(pages.len(), 3);
    }

    #[test]
    fn transcode_passthrough_matches_raw_read() {
        let dir = TempDir::new().unwrap();
        let png = make_png(120, 80, 10, 20, 30);
        let cbz = make_cbz(&dir, "pass.cbz", &[("0001.png", &png)]);

        let (raw, ext) = read_cbz_page(&cbz, 0).unwrap();
        let (bytes, ct) = read_cbz_page_transcoded(&cbz, 0, 0, None).unwrap();
        assert_eq!(bytes, raw);
        assert_eq!(ext, "png");
        assert_eq!(ct, "image/png");
    }

    #[test]
    fn transcode_downscales_wide_page() {
        let dir = TempDir::new().unwrap();
        let png = make_png(400, 300, 200, 100, 50);
        let cbz = make_cbz(&dir, "wide.cbz", &[("0001.png", &png)]);

        let (bytes, ct) = read_cbz_page_transcoded(&cbz, 0, 200, None).unwrap();
        assert_eq!(ct, "image/jpeg");
        let out = image::load_from_memory(&bytes).unwrap();
        assert!(
            out.width() <= 200,
            "width should be clamped: {}",
            out.width()
        );
        assert_eq!(out.height(), 150);
    }

    #[test]
    fn transcode_does_not_upscale() {
        let dir = TempDir::new().unwrap();
        let png = make_png(100, 100, 5, 5, 5);
        let cbz = make_cbz(&dir, "small.cbz", &[("0001.png", &png)]);

        let (bytes, _) = read_cbz_page_transcoded(&cbz, 0, 500, None).unwrap();
        let out = image::load_from_memory(&bytes).unwrap();
        assert_eq!((out.width(), out.height()), (100, 100));
    }

    #[test]
    fn transcode_to_webp() {
        let dir = TempDir::new().unwrap();
        let png = make_png(64, 64, 90, 90, 90);
        let cbz = make_cbz(&dir, "webp.cbz", &[("0001.png", &png)]);

        let (bytes, ct) =
            read_cbz_page_transcoded(&cbz, 0, 0, Some(image::ImageFormat::WebP)).unwrap();
        assert_eq!(ct, "image/webp");
        let out = image::load_from_memory(&bytes).unwrap();
        assert_eq!((out.width(), out.height()), (64, 64));
    }

    #[test]
    fn transcode_out_of_range_page_is_not_found() {
        let dir = TempDir::new().unwrap();
        let png = make_png(32, 32, 1, 1, 1);
        let cbz = make_cbz(&dir, "oob.cbz", &[("0001.png", &png)]);
        let err = read_cbz_page_transcoded(&cbz, 9, 100, None).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }

    #[test]
    fn content_type_table() {
        assert_eq!(content_type_for_ext("jpg"), "image/jpeg");
        assert_eq!(content_type_for_ext("jpeg"), "image/jpeg");
        assert_eq!(content_type_for_ext("png"), "image/png");
        assert_eq!(content_type_for_ext("webp"), "image/webp");
        assert_eq!(content_type_for_ext("gif"), "image/gif");
        assert_eq!(content_type_for_ext("avif"), "image/avif");
        assert_eq!(content_type_for_ext("bin"), "application/octet-stream");
    }

    #[test]
    fn reads_comic_info_xml_from_cbz() {
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(
            &dir,
            "meta.cbz",
            &[
                ("0001.jpg", b"img"),
                (
                    "ComicInfo.xml",
                    b"<ComicInfo><Series>X</Series></ComicInfo>",
                ),
            ],
        );
        let xml = read_cbz_comic_info(&cbz).unwrap();
        assert!(xml.contains("<Series>X</Series>"));
    }

    #[test]
    fn comic_info_absent_returns_none() {
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(&dir, "nometa.cbz", &[("0001.jpg", b"img")]);
        assert!(read_cbz_comic_info(&cbz).is_none());
    }

    #[test]
    fn double_page_flags_absent_when_no_comic_info() {
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(&dir, "noflags.cbz", &[("0001.jpg", b"img")]);
        let (flags, analysed) = read_double_page_flags(&cbz);
        assert!(flags.is_empty());
        assert!(!analysed);
    }

    #[test]
    fn double_page_flags_parsed_from_comic_info() {
        let xml = r#"<ComicInfo>
  <Series>Test</Series>
  <Pages>
    <Page Image="0" />
    <Page Image="1" DoublePage="true" />
    <Page Image="2" />
  </Pages>
</ComicInfo>"#;
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(
            &dir,
            "flags.cbz",
            &[
                ("0001.jpg", b"img"),
                ("0002.jpg", b"img"),
                ("0003.jpg", b"img"),
                ("ComicInfo.xml", xml.as_bytes()),
            ],
        );
        let (flags, analysed) = read_double_page_flags(&cbz);
        assert!(analysed, "should detect pages metadata");
        assert_eq!(flags, [1usize].into_iter().collect::<HashSet<_>>());
    }

    #[test]
    fn not_analysed_when_no_pages_block() {
        let xml = b"<ComicInfo><Series>Test</Series></ComicInfo>";
        let dir = TempDir::new().unwrap();
        let cbz = make_cbz(
            &dir,
            "nopages.cbz",
            &[("0001.jpg", b"img"), ("ComicInfo.xml", xml)],
        );
        let (flags, analysed) = read_double_page_flags(&cbz);
        assert!(!analysed);
        assert!(flags.is_empty());
    }

    #[test]
    fn landscape_images_are_flagged() {
        let dir = TempDir::new().unwrap();
        let wide = make_png(132, 100, 128, 128, 128);
        let portrait = make_png(100, 150, 64, 64, 64);

        let p0 = dir.path().join("0001.png");
        let p1 = dir.path().join("0002.png");
        let p2 = dir.path().join("0003.png");
        std::fs::write(&p0, &portrait).unwrap();
        std::fs::write(&p1, &wide).unwrap();
        std::fs::write(&p2, &portrait).unwrap();

        let flags = detect_spread_pages(&[p0, p1, p2]);
        assert!(flags.contains(&1), "wide page (idx 1) should be flagged");
        assert!(
            !flags.contains(&0),
            "portrait page (idx 0) should not be flagged"
        );
        assert!(
            !flags.contains(&2),
            "portrait page (idx 2) should not be flagged"
        );
    }

    #[test]
    fn uniform_portrait_pages_not_paired() {
        let dir = TempDir::new().unwrap();
        let page = make_png(720, 1080, 255, 255, 255);
        let paths: Vec<PathBuf> = (0..4)
            .map(|i| {
                let p = dir.path().join(format!("{:04}.png", i + 1));
                std::fs::write(&p, &page).unwrap();
                p
            })
            .collect();

        let flags = detect_spread_pages(&paths);
        assert!(
            flags.is_empty(),
            "solid-colour portrait pages must not be paired: {flags:?}"
        );
    }

    #[test]
    fn split_gradient_image_detected_as_spread() {
        let dir = TempDir::new().unwrap();
        let full_w: u32 = 200;
        let h: u32 = 150;
        let gradient = make_gradient_png(full_w, h);

        let full_img = image::load_from_memory(&gradient).unwrap();
        let half_w = full_w / 2;

        let left = full_img.crop_imm(0, 0, half_w, h);
        let right = full_img.crop_imm(half_w, 0, half_w, h);

        let p0 = dir.path().join("0001.png");
        let p1 = dir.path().join("0002.png");
        let write_png = |img: image::DynamicImage, path: &PathBuf| {
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
            std::fs::write(path, buf.into_inner()).unwrap();
        };
        write_png(left, &p0);
        write_png(right, &p1);

        let flags = detect_spread_pages(&[p0, p1]);
        assert!(
            flags.contains(&0),
            "left half of split scan (idx 0) should be flagged as double-page"
        );
    }

    #[test]
    fn empty_page_list_returns_empty_flags() {
        let flags = detect_spread_pages(&[]);
        assert!(flags.is_empty());
    }

    #[test]
    fn independent_noise_pages_not_paired() {
        let dir = TempDir::new().unwrap();
        let page_a = make_noise_png(100, 150, 0xdead_beef_cafe_babe);
        let page_b = make_noise_png(100, 150, 0x1234_5678_9abc_def0);
        let p0 = dir.path().join("0001.png");
        let p1 = dir.path().join("0002.png");
        std::fs::write(&p0, &page_a).unwrap();
        std::fs::write(&p1, &page_b).unwrap();
        let flags = detect_spread_pages(&[p0, p1]);
        assert!(
            flags.is_empty(),
            "independently-generated noise pages must not be paired: {flags:?}"
        );
    }

    #[test]
    fn split_scan_with_compression_noise_detected() {
        let dir = TempDir::new().unwrap();
        let full_w: u32 = 200;
        let h: u32 = 150;
        let noisy = make_noisy_gradient_png(full_w, h, 8);
        let full_img = image::load_from_memory(&noisy).unwrap();
        let half_w = full_w / 2;
        let left = full_img.crop_imm(0, 0, half_w, h);
        let right = full_img.crop_imm(half_w, 0, half_w, h);

        let write_png = |img: image::DynamicImage, path: &std::path::PathBuf| {
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
            std::fs::write(path, buf.into_inner()).unwrap();
        };
        let p0 = dir.path().join("0001.png");
        let p1 = dir.path().join("0002.png");
        write_png(left, &p0);
        write_png(right, &p1);

        let flags = detect_spread_pages(&[p0, p1]);
        assert!(
            flags.contains(&0),
            "split scan with JPEG-level compression noise should still be detected as a spread"
        );
    }
}
