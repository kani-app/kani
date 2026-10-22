//! Page-derived quality signals and policy-driven chapter replacement decisions.

use crate::manifest::ChapterManifest;

/// dHash: grayscale, resize to 9x8, compare each pixel with its right neighbour.
/// 64 comparisons produce 64 bits. Resilient to re-encoding and mild rescaling,
/// which is exactly what a source silently re-uploading a chapter does.
pub(crate) fn perceptual_hash_page(decoded: &image::DynamicImage) -> u64 {
    use image::imageops::FilterType;

    let small = decoded
        .grayscale()
        .resize_exact(9, 8, FilterType::Triangle)
        .to_luma8();

    let mut hash = 0u64;
    let mut bit = 0;
    for y in 0..8u32 {
        for x in 0..8u32 {
            let left = small.get_pixel(x, y).0[0];
            let right = small.get_pixel(x + 1, y).0[0];
            if left > right {
                hash |= 1 << bit;
            }
            bit += 1;
        }
    }
    hash
}

pub fn phash_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// How colour is distributed across a chapter.
///
/// A single colour page is the norm, not the exception: scanlators routinely
/// open (or close) a monochrome chapter with a colour page, and colour spreads
/// appear mid-chapter. Treating any colour page as "this is a colour release"
/// would mislabel most of a library.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourProfile {
    Monochrome,
    /// Some colour pages — an opener, a closer, or a spread — in an otherwise
    /// monochrome chapter.
    ColourAccent,
    FullColour,
    /// Nothing conclusive was readable. Ordinary for JPEG chapters probed by
    /// header alone, whose three-component encoding says nothing about content.
    #[default]
    Unknown,
}

/// Classifies a chapter from the colour flags of the pages actually sampled.
///
/// `FullColour` requires *every* readable page to be colour. With the usual
/// three-page sample of first/middle/last, a colour opener and a colour closer
/// give two of three — which is an accented chapter, not a colour release, and
/// a majority threshold would get it wrong.
pub(crate) fn colour_profile_from_flags(flags: impl IntoIterator<Item = bool>) -> ColourProfile {
    let known: Vec<bool> = flags.into_iter().collect();
    if known.is_empty() {
        return ColourProfile::Unknown;
    }
    let colour = known.iter().filter(|c| **c).count();
    if colour == 0 {
        ColourProfile::Monochrome
    } else if colour == known.len() && known.len() > 1 {
        ColourProfile::FullColour
    } else {
        ColourProfile::ColourAccent
    }
}

/// Whether a decoded page actually *carries* colour, rather than merely being
/// stored in a colour-capable encoding.
///
/// Most manga pages are three-component JPEGs holding grey content, so the
/// encoding alone says nothing — which is why the header probe can only ever
/// answer `Unknown` for them. With the decoded pixels in hand the question is
/// answerable: sample a bounded grid and count pixels whose channels diverge by
/// more than chroma subsampling and ringing produce on grey input.
pub(crate) fn is_colour_image(decoded: &image::DynamicImage) -> bool {
    use image::imageops::FilterType;

    const CHANNEL_SPREAD: u8 = 24;
    const COLOUR_PIXEL_RATIO: f32 = 0.02;
    const GRID: u32 = 64;

    if decoded.color().channel_count() < 3 {
        return false;
    }

    let small = decoded
        .resize_exact(GRID, GRID, FilterType::Triangle)
        .to_rgb8();
    let mut coloured = 0u32;
    for p in small.pixels() {
        let [r, g, b] = p.0;
        let spread = r.max(g).max(b) - r.min(g).min(b);
        if spread > CHANNEL_SPREAD {
            coloured += 1;
        }
    }
    (coloured as f32 / (GRID * GRID) as f32) > COLOUR_PIXEL_RATIO
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
/// Comparable image-quality measurements aggregated across a chapter.
pub struct QualityScore {
    /// Median longer image edge in pixels.
    pub median_long_edge_px: u32,
    /// Encoded page bytes per megapixel of measured image area.
    pub bytes_per_megapixel: f32,
    pub page_count: u32,
    /// Median estimated encoder quality (1–100) across sampled pages, when the
    /// pages are JPEGs and the estimate could be read at all.
    #[serde(default)]
    pub median_encoder_quality: Option<u8>,
    #[serde(default)]
    pub colour: ColourProfile,
}

pub(crate) fn median_of(mut values: Vec<u8>) -> Option<u8> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

pub fn score_from_manifest(m: &ChapterManifest) -> QualityScore {
    let mut long_edges: Vec<u32> = m
        .pages
        .iter()
        .filter_map(|p| match (p.width, p.height) {
            (Some(w), Some(h)) => Some(w.max(h)),
            _ => None,
        })
        .collect();
    long_edges.sort_unstable();

    let median_long_edge_px = if long_edges.is_empty() {
        0
    } else {
        long_edges[long_edges.len() / 2]
    };

    let total_pixels: u64 = m
        .pages
        .iter()
        .filter_map(|p| match (p.width, p.height) {
            (Some(w), Some(h)) => Some(u64::from(w) * u64::from(h)),
            _ => None,
        })
        .sum();

    let bytes_per_megapixel = if total_pixels == 0 {
        0.0
    } else {
        (m.total_bytes as f64 / (total_pixels as f64 / 1_000_000.0)) as f32
    };

    QualityScore {
        median_long_edge_px,
        bytes_per_megapixel,
        page_count: m.page_count,
        median_encoder_quality: median_of(
            m.pages.iter().filter_map(|p| p.encoder_quality).collect(),
        ),
        colour: colour_profile_from_flags(m.pages.iter().filter_map(|p| p.colour)),
    }
}

/// Which axis decided a comparison. Carried to the UI so the dialogue can say
/// *why* something is offered, rather than only that it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityReason {
    Resolution,
    Colour,
    Encoder,
    Bitrate,
    /// The held copy has no readable dimensions, so anything readable is an
    /// improvement on knowing nothing.
    Unmeasured,
}

impl QualityReason {
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Resolution => "upgrade.reason.resolution",
            Self::Colour => "upgrade.reason.colour",
            Self::Encoder => "upgrade.reason.encoder",
            Self::Bitrate => "upgrade.reason.bitrate",
            Self::Unmeasured => "upgrade.reason.unmeasured",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict", content = "reason")]
/// Policy result for a candidate chapter relative to the held copy.
pub enum QualityVerdict {
    Better(QualityReason),
    /// Measurably worse on some axis — never offered as an upgrade.
    Worse,
    /// Nothing separates them beyond noise.
    Same,
}

/// How much authority one axis has over a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisRule {
    /// This axis never decides anything, in either direction.
    Off,
    /// An improvement is an upgrade; a regression is ignored.
    Gain,
    /// An improvement is an upgrade; a regression blocks the candidate.
    #[default]
    Both,
}

impl AxisRule {
    pub fn parse(s: &str) -> Self {
        match s {
            "off" => Self::Off,
            "gain" => Self::Gain,
            _ => Self::Both,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Gain => "gain",
            Self::Both => "both",
        }
    }

    fn counts_gain(self) -> bool {
        !matches!(self, Self::Off)
    }

    fn blocks_loss(self) -> bool {
        matches!(self, Self::Both)
    }
}

/// Which axes may decide an upgrade, and which may veto one.
///
/// None of this is universal taste. A colour release is not automatically the
/// better artefact — plenty of readers prefer the original monochrome scan —
/// and a reader who only cares about pixel count wants the encoder axis to stay
/// out of it entirely. The defaults reproduce the behaviour these rules
/// replaced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityPolicy {
    pub min_res_gain: f32,
    pub resolution: AxisRule,
    pub colour: AxisRule,
    pub encoder: AxisRule,
    pub bitrate: AxisRule,
}

impl Default for QualityPolicy {
    fn default() -> Self {
        Self {
            min_res_gain: 1.2,
            resolution: AxisRule::Both,
            colour: AxisRule::Both,
            encoder: AxisRule::Both,
            bitrate: AxisRule::Gain,
        }
    }
}

/// Encoder-quality points that count as a real difference.
const ENCODER_MARGIN: u8 = 12;
/// Bytes-per-megapixel ratio that counts as a real difference at the same
/// resolution. The margin is the noise guard that stops a trivially larger
/// re-encode from being announced as an upgrade.
const BYTES_MARGIN: f32 = 1.5;

/// Judges a candidate against what is on disk across every axis we can measure:
/// resolution first, then colour, then encoder quality, then bitrate.
///
/// Order matters. Resolution is the only axis every source exposes, so it leads;
/// a resolution *drop* short-circuits to `Worse` before the cheaper signals get
/// a chance to argue for a downgrade. Colour outranks the encoder axes because a
/// colour release is a different artefact, not a better encode of the same one.
pub fn compare_quality(
    candidate: &QualityScore,
    current: &QualityScore,
    policy: &QualityPolicy,
) -> QualityVerdict {
    use ColourProfile::{FullColour, Monochrome};

    if candidate.median_long_edge_px == 0 {
        return QualityVerdict::Same;
    }
    if current.median_long_edge_px == 0 {
        return QualityVerdict::Better(QualityReason::Unmeasured);
    }

    // A monochrome re-upload of a colour release is a downgrade whatever the
    // pixel count says — checked before resolution, since a higher-resolution
    // greyscale rip would otherwise read as an upgrade.
    if policy.colour.blocks_loss() && current.colour == FullColour && candidate.colour == Monochrome
    {
        return QualityVerdict::Worse;
    }

    let res_ratio = candidate.median_long_edge_px as f32 / current.median_long_edge_px as f32;
    if policy.resolution.blocks_loss() && res_ratio < 1.0 {
        return QualityVerdict::Worse;
    }
    if policy.colour.counts_gain() && candidate.colour == FullColour && current.colour == Monochrome
    {
        return QualityVerdict::Better(QualityReason::Colour);
    }
    if policy.resolution.counts_gain() && res_ratio >= policy.min_res_gain {
        return QualityVerdict::Better(QualityReason::Resolution);
    }

    if let (Some(cand_q), Some(cur_q)) = (
        candidate.median_encoder_quality,
        current.median_encoder_quality,
    ) {
        if policy.encoder.counts_gain() && cand_q >= cur_q.saturating_add(ENCODER_MARGIN) {
            return QualityVerdict::Better(QualityReason::Encoder);
        }
        if policy.encoder.blocks_loss() && cand_q.saturating_add(ENCODER_MARGIN) <= cur_q {
            return QualityVerdict::Worse;
        }
    }

    // A large bytes-per-megapixel *drop* at the same resolution is the mirror of
    // the gain below, and only blocks when the axis is set to police both
    // directions — bitrate is the noisiest signal, so it does not by default.
    if policy.bitrate.blocks_loss()
        && candidate.bytes_per_megapixel > 0.0
        && current.bytes_per_megapixel >= candidate.bytes_per_megapixel * BYTES_MARGIN
    {
        return QualityVerdict::Worse;
    }
    if policy.bitrate.counts_gain()
        && current.bytes_per_megapixel > 0.0
        && candidate.bytes_per_megapixel >= current.bytes_per_megapixel * BYTES_MARGIN
    {
        return QualityVerdict::Better(QualityReason::Bitrate);
    }

    QualityVerdict::Same
}

pub fn is_meaningfully_better(
    candidate: &QualityScore,
    current: &QualityScore,
    policy: &QualityPolicy,
) -> bool {
    matches!(
        compare_quality(candidate, current, policy),
        QualityVerdict::Better(_)
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::manifest::PageDigest;

    fn manifest(pages: Vec<(Option<u32>, Option<u32>)>, total_bytes: u64) -> ChapterManifest {
        ChapterManifest {
            schema: 1,
            archive_hash: "x".into(),
            page_count: pages.len() as u32,
            total_bytes,
            created_at: 0,
            pages: pages
                .into_iter()
                .enumerate()
                .map(|(i, (w, h))| PageDigest {
                    name: format!("{i:04}.jpg"),
                    bytes: 0,
                    content_hash: "h".into(),
                    perceptual_hash: 0,
                    width: w,
                    height: h,
                    colour: None,
                    encoder_quality: None,
                })
                .collect(),
        }
    }

    fn solid(w: u32, h: u32, v: u8) -> image::DynamicImage {
        image::DynamicImage::ImageLuma8(image::GrayImage::from_pixel(w, h, image::Luma([v])))
    }

    #[test]
    fn identical_images_have_distance_zero() {
        let a = perceptual_hash_page(&solid(64, 64, 128));
        let b = perceptual_hash_page(&solid(64, 64, 128));
        assert_eq!(phash_distance(a, b), 0);
    }

    #[test]
    fn phash_distance_counts_differing_bits() {
        assert_eq!(phash_distance(0b1011, 0b1000), 2);
        assert_eq!(phash_distance(u64::MAX, 0), 64);
    }

    #[test]
    fn gradient_survives_a_rescale() {
        let mut img = image::GrayImage::new(90, 80);
        for (x, _y, p) in img.enumerate_pixels_mut() {
            *p = image::Luma([(x * 2) as u8]);
        }
        let full = image::DynamicImage::ImageLuma8(img);
        let half = full.resize_exact(45, 40, image::imageops::FilterType::Triangle);

        let d = phash_distance(perceptual_hash_page(&full), perceptual_hash_page(&half));
        assert!(
            d < 6,
            "rescaled gradient should stay close, distance was {d}"
        );
    }

    #[test]
    fn score_uses_median_long_edge_and_skips_unknown_dims() {
        let m = manifest(
            vec![
                (Some(800), Some(1200)),
                (Some(900), Some(1400)),
                (None, None),
                (Some(1000), Some(1600)),
            ],
            3_000_000,
        );
        let s = score_from_manifest(&m);
        assert_eq!(s.median_long_edge_px, 1400, "median of 1200/1400/1600");
        assert_eq!(s.page_count, 4);
        assert!(s.bytes_per_megapixel > 0.0);
    }

    #[test]
    fn manifest_without_known_dimensions_scores_zero_and_never_wins() {
        let unknown = score_from_manifest(&manifest(vec![(None, None)], 100));
        assert_eq!(unknown.median_long_edge_px, 0);
        let known = score_from_manifest(&manifest(vec![(Some(800), Some(1200))], 100));
        assert!(!is_meaningfully_better(
            &unknown,
            &known,
            &QualityPolicy::default()
        ));
    }

    #[test]
    fn higher_resolution_wins() {
        let current = score_from_manifest(&manifest(vec![(Some(800), Some(1200))], 1_000_000));
        let better = score_from_manifest(&manifest(vec![(Some(1600), Some(2400))], 2_000_000));
        assert!(is_meaningfully_better(
            &better,
            &current,
            &QualityPolicy::default()
        ));
    }

    #[test]
    fn equal_resolution_slightly_larger_file_does_not_win() {
        let current = score_from_manifest(&manifest(vec![(Some(800), Some(1200))], 1_000_000));
        let barely = score_from_manifest(&manifest(vec![(Some(800), Some(1200))], 1_100_000));
        assert!(
            !is_meaningfully_better(&barely, &current, &QualityPolicy::default()),
            "a 10% larger re-encode is not an upgrade"
        );
    }

    #[test]
    fn equal_resolution_much_larger_file_wins() {
        let current = score_from_manifest(&manifest(vec![(Some(800), Some(1200))], 1_000_000));
        let much = score_from_manifest(&manifest(vec![(Some(800), Some(1200))], 2_000_000));
        assert!(is_meaningfully_better(
            &much,
            &current,
            &QualityPolicy::default()
        ));
    }

    fn score(long_edge: u32, bpm: f32, enc: Option<u8>, colour: ColourProfile) -> QualityScore {
        QualityScore {
            median_long_edge_px: long_edge,
            bytes_per_megapixel: bpm,
            page_count: 20,
            median_encoder_quality: enc,
            colour,
        }
    }

    #[test]
    fn a_colour_release_beats_a_monochrome_one_at_the_same_resolution() {
        let held = score(1600, 1000.0, None, ColourProfile::Monochrome);
        let cand = score(1600, 1000.0, None, ColourProfile::FullColour);
        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Better(QualityReason::Colour)
        );
    }

    #[test]
    fn a_monochrome_rip_never_beats_a_colour_release_however_large() {
        let held = score(1600, 1000.0, None, ColourProfile::FullColour);
        let cand = score(3200, 9000.0, None, ColourProfile::Monochrome);
        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Worse,
            "a higher-resolution greyscale rip of a colour chapter is a downgrade"
        );
    }

    #[test]
    fn an_accented_chapter_is_not_treated_as_a_colour_release() {
        assert_eq!(
            colour_profile_from_flags([true, false, true]),
            ColourProfile::ColourAccent
        );
        let held = score(1600, 1000.0, None, ColourProfile::Monochrome);
        let cand = score(1600, 1000.0, None, ColourProfile::ColourAccent);
        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Same,
            "a colour opener is not a colour release"
        );
    }

    #[test]
    fn a_single_known_colour_page_cannot_be_a_full_colour_release() {
        assert_eq!(
            colour_profile_from_flags([true]),
            ColourProfile::ColourAccent,
            "one sample is too thin to call a whole chapter colour"
        );
        assert_eq!(
            colour_profile_from_flags([false]),
            ColourProfile::Monochrome
        );
        assert_eq!(
            colour_profile_from_flags(std::iter::empty()),
            ColourProfile::Unknown
        );
    }

    #[test]
    fn a_materially_better_encode_wins_at_the_same_resolution() {
        let held = score(1600, 1000.0, Some(70), ColourProfile::Unknown);
        let cand = score(1600, 1000.0, Some(92), ColourProfile::Unknown);
        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Better(QualityReason::Encoder)
        );
    }

    #[test]
    fn an_encoder_difference_inside_the_estimates_error_does_not_win() {
        let held = score(1600, 1000.0, Some(80), ColourProfile::Unknown);
        let cand = score(1600, 1000.0, Some(88), ColourProfile::Unknown);
        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Same,
            "8 points is within the estimator's own error, so it proves nothing"
        );
    }

    #[test]
    fn a_worse_encode_at_the_same_resolution_is_a_downgrade() {
        let held = score(1600, 1000.0, Some(92), ColourProfile::Unknown);
        let cand = score(1600, 4000.0, Some(60), ColourProfile::Unknown);
        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Worse,
            "a bloated low-quality re-encode must not win on bytes-per-megapixel"
        );
    }

    #[test]
    fn resolution_outranks_a_worse_encode() {
        let held = score(800, 1000.0, Some(95), ColourProfile::Unknown);
        let cand = score(1600, 1000.0, Some(60), ColourProfile::Unknown);
        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Better(QualityReason::Resolution),
            "double the pixels is worth more than the encoder estimate"
        );
    }

    #[test]
    fn an_unmeasurable_held_copy_yields_the_unmeasured_reason() {
        let held = score(0, 0.0, None, ColourProfile::Unknown);
        let cand = score(1600, 1000.0, None, ColourProfile::Unknown);
        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Better(QualityReason::Unmeasured)
        );
    }

    #[test]
    fn an_unmeasurable_candidate_is_never_offered() {
        let held = score(1600, 1000.0, None, ColourProfile::Unknown);
        let cand = score(0, 0.0, None, ColourProfile::Unknown);
        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Same
        );
    }

    #[test]
    fn a_grey_image_is_not_reported_as_colour() {
        let grey = image::DynamicImage::ImageLuma8(image::GrayImage::from_pixel(
            32,
            32,
            image::Luma([120]),
        ));
        assert!(!is_colour_image(&grey));

        let grey_rgb = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            32,
            32,
            image::Rgb([120, 120, 120]),
        ));
        assert!(!is_colour_image(&grey_rgb));
    }

    #[test]
    fn a_saturated_image_is_reported_as_colour() {
        let red = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            32,
            32,
            image::Rgb([200, 30, 30]),
        ));
        assert!(is_colour_image(&red));
    }

    #[test]
    fn a_lone_colour_speck_does_not_make_a_page_colour() {
        let mut img = image::RgbImage::from_pixel(64, 64, image::Rgb([128, 128, 128]));
        img.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        assert!(
            !is_colour_image(&image::DynamicImage::ImageRgb8(img)),
            "one stray pixel is scanner noise, not a colour page"
        );
    }

    #[test]
    fn colour_can_be_told_it_is_not_an_upgrade() {
        let held = score(1600, 1000.0, None, ColourProfile::Monochrome);
        let cand = score(1600, 1000.0, None, ColourProfile::FullColour);
        let policy = QualityPolicy {
            colour: AxisRule::Off,
            ..QualityPolicy::default()
        };
        assert_eq!(compare_quality(&cand, &held, &policy), QualityVerdict::Same);
    }

    #[test]
    fn the_colour_guard_can_be_lifted_so_a_monochrome_rip_competes_on_pixels() {
        let held = score(1600, 1000.0, None, ColourProfile::FullColour);
        let cand = score(3200, 1000.0, None, ColourProfile::Monochrome);

        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Worse,
            "by default losing colour vetoes the candidate"
        );

        let policy = QualityPolicy {
            colour: AxisRule::Gain,
            ..QualityPolicy::default()
        };
        assert_eq!(
            compare_quality(&cand, &held, &policy),
            QualityVerdict::Better(QualityReason::Resolution),
            "with the guard lifted, resolution decides it"
        );
    }

    #[test]
    fn the_encoder_axis_can_be_silenced_without_affecting_the_others() {
        let held = score(1600, 1000.0, Some(70), ColourProfile::Unknown);
        let cand = score(1600, 1000.0, Some(95), ColourProfile::Unknown);
        let policy = QualityPolicy {
            encoder: AxisRule::Off,
            ..QualityPolicy::default()
        };
        assert_eq!(compare_quality(&cand, &held, &policy), QualityVerdict::Same);
    }

    #[test]
    fn bitrate_can_be_asked_to_police_regressions_too() {
        let held = score(1600, 3000.0, None, ColourProfile::Unknown);
        let cand = score(1600, 1000.0, None, ColourProfile::Unknown);

        assert_eq!(
            compare_quality(&cand, &held, &QualityPolicy::default()),
            QualityVerdict::Same,
            "bitrate is noisy, so by default a drop is not a veto"
        );

        let policy = QualityPolicy {
            bitrate: AxisRule::Both,
            ..QualityPolicy::default()
        };
        assert_eq!(
            compare_quality(&cand, &held, &policy),
            QualityVerdict::Worse
        );
    }

    #[test]
    fn the_default_policy_reproduces_the_behaviour_it_replaced() {
        let p = QualityPolicy::default();
        assert_eq!(p.resolution, AxisRule::Both);
        assert_eq!(p.colour, AxisRule::Both);
        assert_eq!(p.encoder, AxisRule::Both);
        assert_eq!(
            p.bitrate,
            AxisRule::Gain,
            "a bitrate drop never vetoed a candidate before this was configurable"
        );
    }

    #[test]
    fn axis_rules_round_trip_through_their_stored_form() {
        for rule in [AxisRule::Off, AxisRule::Gain, AxisRule::Both] {
            assert_eq!(AxisRule::parse(rule.as_str()), rule);
        }
        assert_eq!(
            AxisRule::parse("nonsense"),
            AxisRule::Both,
            "an unreadable setting must fall back to the strictest rule, not the loosest"
        );
    }

    #[test]
    fn lower_resolution_never_wins() {
        let current = score_from_manifest(&manifest(vec![(Some(1600), Some(2400))], 1_000_000));
        let worse = score_from_manifest(&manifest(vec![(Some(800), Some(1200))], 9_000_000));
        assert!(
            !is_meaningfully_better(&worse, &current, &QualityPolicy::default()),
            "a downgrade cannot win on file size alone"
        );
    }
}
