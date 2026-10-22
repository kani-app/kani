//! Judging a page's quality from the first couple of kilobytes.
//!
//! Everything here is derived from image *headers*, so a caller can range-request
//! a small prefix of a remote page and learn its dimensions, colour-ness and
//! encoder quality without downloading it. That is what makes a pre-download
//! upgrade comparison possible at all: a source listing exposes page counts and
//! URLs, never dimensions.

/// What a header prefix can tell us about a page.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PageProbe {
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Total encoded size, from the response rather than the prefix.
    pub bytes: Option<u64>,
    /// Whether the encoding *can* carry colour. A three-component JPEG may still
    /// hold a grey image, so this is "colour release" evidence, not proof.
    pub colour: Option<bool>,
    /// Estimated encoder quality (1–100), JPEG only.
    ///
    /// Approximate: it inverts libjpeg's table scaling, while encoders may use
    /// different tables. Upgrade detection relies on ordering, not the absolute
    /// value.
    pub jpeg_quality: Option<u8>,
}

/// Enough bytes for a PNG IHDR, a JPEG's SOF and quantisation tables, and the
/// dimension headers of every other format we care about.
pub const PROBE_PREFIX_BYTES: usize = 4096;

pub fn probe_header(prefix: &[u8], bytes: Option<u64>) -> PageProbe {
    let (width, height) = match imagesize::blob_size(prefix) {
        Ok(s) => (Some(s.width as u32), Some(s.height as u32)),
        Err(_) => (None, None),
    };

    let (colour, jpeg_quality) = if is_png(prefix) {
        (png_is_colour(prefix), None)
    } else if is_jpeg(prefix) {
        (jpeg_is_colour(prefix), jpeg_quality(prefix))
    } else {
        (None, None)
    };

    PageProbe {
        width,
        height,
        bytes,
        colour,
        jpeg_quality,
    }
}

/// The image type a byte prefix actually *is*, from its magic number.
///
/// Content-Type is a claim, not evidence: CDNs routinely serve real images as
/// `application/octet-stream`, and an attacker can label anything `image/png`.
/// Deciding on the bytes fixes both — a legitimate image is accepted whatever
/// its label, and an HTML error or challenge page is refused however it is
/// labelled.
pub fn sniff_image_mime(b: &[u8]) -> Option<&'static str> {
    if is_png(b) {
        return Some("image/png");
    }
    if is_jpeg(b) {
        return Some("image/jpeg");
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if b.starts_with(&[0x42, 0x4D]) {
        return Some("image/bmp");
    }
    // ISO-BMFF brands: AVIF and HEIC share the `ftyp` box layout.
    if b.len() >= 12 && &b[4..8] == b"ftyp" {
        return match &b[8..12] {
            b"avif" | b"avis" => Some("image/avif"),
            b"heic" | b"heix" | b"hevc" | b"mif1" => Some("image/heic"),
            _ => None,
        };
    }
    None
}

fn is_png(b: &[u8]) -> bool {
    b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
}

fn is_jpeg(b: &[u8]) -> bool {
    b.starts_with(&[0xFF, 0xD8])
}

/// PNG colour type lives at byte 25, inside the IHDR that must come first.
/// 0 = grey, 2 = RGB, 3 = palette, 4 = grey+alpha, 6 = RGBA.
fn png_is_colour(b: &[u8]) -> Option<bool> {
    let ct = *b.get(25)?;
    match ct {
        0 | 4 => Some(false),
        2 | 3 | 6 => Some(true),
        _ => None,
    }
}

/// Walks JPEG segments to the frame header and reads its component count.
///
/// Only a single component is conclusive. Most encoders — including the one in
/// the `image` crate — write three-component YCbCr even for greyscale input, so
/// three components means "unknown", not "colour". Claiming otherwise would
/// label every ordinary manga page a colour release.
fn jpeg_is_colour(b: &[u8]) -> Option<bool> {
    for (marker, seg) in jpeg_segments(b) {
        // Any SOF except SOF4/SOF8/SOF12 (which are not frame headers).
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            // precision(1) height(2) width(2) components(1)
            return match seg.get(5) {
                Some(1) => Some(false),
                _ => None,
            };
        }
    }
    None
}

/// The luminance quantisation table libjpeg would have produced for quality 50.
/// Scaling this table is how libjpeg derives every other quality, so comparing
/// against it inverts back to the quality that was requested.
const STD_LUMINANCE_Q: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

/// Inverts libjpeg's quality→table scaling.
///
/// libjpeg builds a table as `clamp((std * scale + 50) / 100, 1, 255)` where
/// `scale` is `5000/q` below quality 50 and `200 - 2q` at or above it. That
/// division floors, so a stored entry `q` corresponds to a scale anywhere in
/// `[(q*100-50)/std, (q*100+50)/std)` — the midpoint is `q*100/std`, and using
/// the lower bound instead biases every estimate downward. Averaging across the
/// table absorbs the remaining per-entry rounding.
pub fn jpeg_quality(b: &[u8]) -> Option<u8> {
    let table = jpeg_luminance_table(b)?;

    let mut scales = Vec::with_capacity(64);
    for (i, &q) in table.iter().enumerate() {
        let std = f64::from(STD_LUMINANCE_Q[i]);
        // Saturated entries carry no information about the scale.
        if q == 0 || q >= 255 {
            continue;
        }
        scales.push(f64::from(q) * 100.0 / std);
    }
    if scales.is_empty() {
        return None;
    }
    let scale = scales.iter().sum::<f64>() / scales.len() as f64;

    let quality = if scale <= 0.0 {
        100.0
    } else if scale > 100.0 {
        5000.0 / scale
    } else {
        (200.0 - scale) / 2.0
    };
    Some(quality.round().clamp(1.0, 100.0) as u8)
}

/// The first DQT table with id 0, expanded to 64 entries.
fn jpeg_luminance_table(b: &[u8]) -> Option<[u16; 64]> {
    for (marker, seg) in jpeg_segments(b) {
        if marker != 0xDB {
            continue;
        }
        let mut p = 0usize;
        // A DQT segment may carry several tables back to back.
        while p < seg.len() {
            let pq_tq = *seg.get(p)?;
            let precision = pq_tq >> 4;
            let id = pq_tq & 0x0F;
            p += 1;
            let entries = if precision == 0 { 64 } else { 128 };
            if p + entries > seg.len() {
                return None;
            }
            if id == 0 {
                let mut table = [0u16; 64];
                for (i, slot) in table.iter_mut().enumerate() {
                    *slot = if precision == 0 {
                        u16::from(seg[p + i])
                    } else {
                        u16::from_be_bytes([seg[p + i * 2], seg[p + i * 2 + 1]])
                    };
                }
                return Some(table);
            }
            p += entries;
        }
    }
    None
}

/// Yields `(marker, payload)` for each JPEG segment present in the prefix.
fn jpeg_segments(b: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    const JPEG_START_OF_IMAGE_LEN: usize = 2;
    let mut i = JPEG_START_OF_IMAGE_LEN;
    while i + 3 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        // Standalone markers carry no length.
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        // Start of scan: entropy-coded data follows, nothing more to parse.
        if marker == 0xDA {
            break;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if len < 2 || i + 2 + len > b.len() {
            break;
        }
        out.push((marker, &b[i + 4..i + 2 + len]));
        i += 2 + len;
    }
    out
}

pub use crate::quality::ColourProfile;

/// Classifies a chapter from the colour flags of the pages actually probed.
pub(crate) fn colour_profile(probes: &[PageProbe]) -> ColourProfile {
    crate::quality::colour_profile_from_flags(probes.iter().filter_map(|p| p.colour))
}

/// Builds a comparable score from a sample of probed pages.
///
/// `page_count` comes from the listing rather than the sample, since only a few
/// pages are ever probed. Pages whose dimensions could not be read are skipped
/// rather than counted as zero, which would drag the median toward nothing.
pub fn score_from_probes(
    probes: &[PageProbe],
    page_count: u32,
) -> Option<crate::quality::QualityScore> {
    let colour = colour_profile(probes);
    let mut long_edges: Vec<u32> = probes
        .iter()
        .filter_map(|p| match (p.width, p.height) {
            (Some(w), Some(h)) => Some(w.max(h)),
            _ => None,
        })
        .collect();
    if long_edges.is_empty() {
        return None;
    }
    long_edges.sort_unstable();
    let median_long_edge_px = long_edges[long_edges.len() / 2];

    let sampled_pixels: u64 = probes
        .iter()
        .filter_map(|p| match (p.width, p.height) {
            (Some(w), Some(h)) => Some(u64::from(w) * u64::from(h)),
            _ => None,
        })
        .sum();
    let sampled_bytes: u64 = probes
        .iter()
        .filter(|p| p.width.is_some())
        .filter_map(|p| p.bytes)
        .sum();

    let bytes_per_megapixel = if sampled_pixels == 0 || sampled_bytes == 0 {
        0.0
    } else {
        (sampled_bytes as f64 / (sampled_pixels as f64 / 1_000_000.0)) as f32
    };

    Some(crate::quality::QualityScore {
        median_long_edge_px,
        bytes_per_megapixel,
        page_count,
        median_encoder_quality: crate::quality::median_of(
            probes.iter().filter_map(|p| p.jpeg_quality).collect(),
        ),
        colour,
    })
}

/// Picks which page indices to probe: the ends and the middle, rather than the
/// first N. Opening pages are often covers or credits and are unrepresentative
/// of the scan.
pub fn sample_indices(page_count: usize, samples: usize) -> Vec<usize> {
    if page_count == 0 || samples == 0 {
        return Vec::new();
    }
    let samples = samples.min(page_count);
    if samples == 1 {
        return vec![page_count / 2];
    }
    let mut out: Vec<usize> = (0..samples)
        .map(|i| i * (page_count - 1) / (samples - 1))
        .collect();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn jpeg_at(quality: u8, colour: bool) -> Vec<u8> {
        let img = if colour {
            image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(64, 96, |x, y| {
                image::Rgb([(x * 3) as u8, (y * 2) as u8, 128])
            }))
        } else {
            image::DynamicImage::ImageLuma8(image::GrayImage::from_fn(64, 96, |x, y| {
                image::Luma([((x + y) % 255) as u8])
            }))
        };
        let mut out = std::io::Cursor::new(Vec::new());
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
        enc.encode_image(&img).unwrap();
        out.into_inner()
    }

    fn png_grey() -> Vec<u8> {
        let img = image::DynamicImage::ImageLuma8(image::GrayImage::new(40, 60));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn png_colour() -> Vec<u8> {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(40, 60));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn dimensions_come_from_a_prefix_not_the_whole_file() {
        let jpeg = jpeg_at(85, false);
        let prefix = &jpeg[..jpeg.len().min(PROBE_PREFIX_BYTES)];
        let p = probe_header(prefix, Some(jpeg.len() as u64));
        assert_eq!((p.width, p.height), (Some(64), Some(96)));
        assert_eq!(p.bytes, Some(jpeg.len() as u64));
    }

    #[test]
    fn a_truncated_prefix_still_yields_dimensions() {
        let jpeg = jpeg_at(80, true);
        let p = probe_header(&jpeg[..2048.min(jpeg.len())], None);
        assert_eq!(
            (p.width, p.height),
            (Some(64), Some(96)),
            "dimensions must be readable from the header alone"
        );
    }

    #[test]
    fn png_colour_type_distinguishes_grey_from_rgb() {
        assert_eq!(probe_header(&png_grey(), None).colour, Some(false));
        assert_eq!(probe_header(&png_colour(), None).colour, Some(true));
    }

    #[test]
    fn a_three_component_jpeg_is_unknown_rather_than_colour() {
        assert_eq!(probe_header(&jpeg_at(80, false), None).colour, None);
        assert_eq!(probe_header(&jpeg_at(80, true), None).colour, None);
    }

    #[test]
    fn a_single_component_jpeg_is_conclusively_grey() {
        const START_OF_IMAGE: &[u8] = &[0xFF, 0xD8];
        const BASELINE_FRAME: &[u8] = &[0xFF, 0xC0, 0x00, 0x0B];
        const SINGLE_COMPONENT_96_BY_64: &[u8] =
            &[0x08, 0x00, 0x60, 0x00, 0x40, 0x01, 0x01, 0x11, 0x00];
        let jpeg = [START_OF_IMAGE, BASELINE_FRAME, SINGLE_COMPONENT_96_BY_64].concat();
        assert_eq!(jpeg_is_colour(&jpeg), Some(false));
    }

    #[test]
    fn jpeg_quality_is_recovered_from_the_quantisation_table() {
        for requested in [40u8, 60, 75, 90] {
            let est = probe_header(&jpeg_at(requested, false), None)
                .jpeg_quality
                .unwrap_or_else(|| panic!("no estimate at q={requested}"));
            let delta = est.abs_diff(requested);
            assert!(
                delta <= 8,
                "estimated {est} for a q={requested} encode (off by {delta}); \
                 encoders deviate from libjpeg's scaling, so this is a band, \
                 not an equality"
            );
        }
    }

    #[test]
    fn a_heavier_recompression_scores_lower() {
        let good = probe_header(&jpeg_at(90, false), None)
            .jpeg_quality
            .unwrap();
        let bad = probe_header(&jpeg_at(45, false), None)
            .jpeg_quality
            .unwrap();
        assert!(
            good > bad,
            "the ordering is what upgrade detection relies on: {good} vs {bad}"
        );
    }

    #[test]
    fn a_png_has_no_jpeg_quality() {
        assert_eq!(probe_header(&png_grey(), None).jpeg_quality, None);
    }

    fn grey() -> PageProbe {
        PageProbe {
            colour: Some(false),
            ..Default::default()
        }
    }
    fn coloured() -> PageProbe {
        PageProbe {
            colour: Some(true),
            ..Default::default()
        }
    }
    fn unknown() -> PageProbe {
        PageProbe {
            colour: None,
            ..Default::default()
        }
    }

    #[test]
    fn an_all_monochrome_chapter_is_monochrome() {
        assert_eq!(
            colour_profile(&[grey(), grey(), grey()]),
            ColourProfile::Monochrome
        );
    }

    #[test]
    fn a_colour_opener_does_not_make_a_colour_release() {
        assert_eq!(
            colour_profile(&[coloured(), grey(), grey()]),
            ColourProfile::ColourAccent
        );
    }

    #[test]
    fn a_mid_chapter_colour_spread_is_an_accent() {
        assert_eq!(
            colour_profile(&[grey(), coloured(), grey()]),
            ColourProfile::ColourAccent
        );
    }

    #[test]
    fn a_colour_opener_and_closer_together_are_still_accents() {
        assert_eq!(
            colour_profile(&[coloured(), grey(), coloured()]),
            ColourProfile::ColourAccent,
            "two of three is a majority, and a majority threshold would call \
             this a colour release; it is a monochrome chapter with bookends"
        );
    }

    #[test]
    fn every_page_colour_is_a_colour_release() {
        assert_eq!(
            colour_profile(&[coloured(), coloured(), coloured()]),
            ColourProfile::FullColour
        );
    }

    #[test]
    fn a_single_probed_page_never_claims_a_full_colour_release() {
        assert_eq!(
            colour_profile(&[coloured()]),
            ColourProfile::ColourAccent,
            "one page is not evidence about the rest of the chapter"
        );
    }

    #[test]
    fn an_all_jpeg_chapter_is_unknown_rather_than_monochrome() {
        assert_eq!(
            colour_profile(&[unknown(), unknown(), unknown()]),
            ColourProfile::Unknown,
            "guessing monochrome here would be a confident wrong answer for \
             every JPEG chapter in the library"
        );
    }

    #[test]
    fn unreadable_pages_do_not_dilute_the_readable_ones() {
        assert_eq!(
            colour_profile(&[coloured(), unknown(), coloured()]),
            ColourProfile::FullColour
        );
        assert_eq!(
            colour_profile(&[grey(), unknown(), grey()]),
            ColourProfile::Monochrome
        );
    }

    #[test]
    fn a_score_ignores_pages_whose_header_was_unreadable() {
        let probes = vec![
            PageProbe {
                width: Some(1600),
                height: Some(2400),
                bytes: Some(400_000),
                ..Default::default()
            },
            PageProbe::default(),
            PageProbe {
                width: Some(1600),
                height: Some(2400),
                bytes: Some(400_000),
                ..Default::default()
            },
        ];
        let s = score_from_probes(&probes, 20).unwrap();
        assert_eq!(s.median_long_edge_px, 2400);
        assert_eq!(s.page_count, 20, "page count comes from the listing");
        assert!(
            s.bytes_per_megapixel > 0.0,
            "an unreadable page must not zero the whole score"
        );
    }

    #[test]
    fn a_sample_with_no_readable_page_yields_no_score() {
        assert!(score_from_probes(&[PageProbe::default()], 10).is_none());
        assert!(score_from_probes(&[], 10).is_none());
    }

    #[test]
    fn a_probed_score_orders_a_higher_resolution_candidate_above_the_held_copy() {
        let held = score_from_probes(
            &[PageProbe {
                width: Some(800),
                height: Some(1200),
                bytes: Some(120_000),
                ..Default::default()
            }],
            20,
        )
        .unwrap();
        let candidate = score_from_probes(
            &[PageProbe {
                width: Some(1600),
                height: Some(2400),
                bytes: Some(400_000),
                ..Default::default()
            }],
            20,
        )
        .unwrap();
        assert!(
            crate::quality::is_meaningfully_better(
                &candidate,
                &held,
                &crate::quality::QualityPolicy::default()
            ),
            "this is the comparison that was impossible before probing"
        );
        assert!(!crate::quality::is_meaningfully_better(
            &held,
            &candidate,
            &crate::quality::QualityPolicy::default()
        ));
    }

    #[test]
    fn samples_span_the_chapter_rather_than_clustering_at_the_front() {
        assert_eq!(sample_indices(20, 3), vec![0, 9, 19]);
        assert_eq!(sample_indices(1, 3), vec![0]);
        assert_eq!(sample_indices(10, 1), vec![5]);
        assert!(sample_indices(0, 3).is_empty());
        assert!(sample_indices(10, 0).is_empty());
    }

    #[test]
    fn garbage_probes_to_nothing_rather_than_panicking() {
        for junk in [
            &b""[..],
            &b"not an image at all"[..],
            &[0xFF, 0xD8][..],
            &[0x89, b'P', b'N', b'G'][..],
        ] {
            let p = probe_header(junk, None);
            assert_eq!(p.width, None);
            assert_eq!(p.jpeg_quality, None);
        }
    }
}

#[cfg(test)]
mod sniff_tests {
    use super::sniff_image_mime;

    #[test]
    fn real_magic_numbers_are_recognised() {
        assert_eq!(
            sniff_image_mime(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
            Some("image/png")
        );
        assert_eq!(
            sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("image/jpeg")
        );
        assert_eq!(sniff_image_mime(b"GIF89a...."), Some("image/gif"));
        assert_eq!(
            sniff_image_mime(b"RIFF\0\0\0\0WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(sniff_image_mime(b"\0\0\0\x20ftypavif"), Some("image/avif"));
    }

    #[test]
    fn a_page_pretending_to_be_an_image_is_not_one() {
        assert_eq!(sniff_image_mime(b"<!DOCTYPE html><html>"), None);
        assert_eq!(
            sniff_image_mime(b"<html><body>Just a moment...</body>"),
            None
        );
        assert_eq!(sniff_image_mime(b"{\"error\": \"nope\"}"), None);
    }

    #[test]
    fn a_truncated_prefix_is_refused_rather_than_guessed() {
        assert_eq!(sniff_image_mime(b""), None);
        assert_eq!(sniff_image_mime(&[0x89]), None);
        assert_eq!(sniff_image_mime(b"RIFF"), None);
    }
}
