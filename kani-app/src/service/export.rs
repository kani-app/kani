use std::io::{Cursor, Read, Write as _};
use std::path::Path;

use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ZipLibrary};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};

use super::AppService;
use crate::error::{Result, ServiceError};
use crate::ids::ChapterId;

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceProfile {
    Standard,
    KindlePaperwhite,
    KindleScribe,
    KoboClara,
    KoboLibra,
    KoboSage,
    Custom {
        width: u32,
        height: u32,
        grayscale: bool,
        gamma: f32,
    },
}

impl DeviceProfile {
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        match self {
            Self::Standard => None,
            Self::KindlePaperwhite => Some((1236, 1648)),
            Self::KindleScribe => Some((1860, 2480)),
            Self::KoboClara => Some((1072, 1448)),
            Self::KoboLibra => Some((1264, 1680)),
            Self::KoboSage => Some((1440, 1920)),
            Self::Custom { width, height, .. } => Some((*width, *height)),
        }
    }

    pub fn grayscale(&self) -> bool {
        match self {
            Self::Standard => false,
            Self::Custom { grayscale, .. } => *grayscale,
            _ => true,
        }
    }

    pub fn gamma(&self) -> Option<f32> {
        match self {
            Self::Standard => None,
            Self::Custom { gamma, .. } => Some(*gamma),
            _ => Some(1.5),
        }
    }
}

impl std::str::FromStr for DeviceProfile {
    type Err = ();
    fn from_str(s: &str) -> std::result::Result<Self, ()> {
        Ok(match s {
            "kindle-pw" | "kindle-paperwhite" => Self::KindlePaperwhite,
            "kindle-scribe" => Self::KindleScribe,
            "kobo-clara" => Self::KoboClara,
            "kobo-libra" => Self::KoboLibra,
            "kobo-sage" => Self::KoboSage,
            _ => Self::Standard,
        })
    }
}

pub enum KccFormat {
    Epub,
    Mobi,
    Cbz,
}

impl KccFormat {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Epub => "EPUB",
            Self::Mobi => "MOBI",
            Self::Cbz => "CBZ",
        }
    }

    pub fn mime(&self) -> &'static str {
        match self {
            Self::Epub => "application/epub+zip",
            Self::Mobi => "application/x-mobipocket-ebook",
            Self::Cbz => "application/x-cbz",
        }
    }

    pub fn ext(&self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Mobi => "mobi",
            Self::Cbz => "cbz",
        }
    }
}

impl std::str::FromStr for KccFormat {
    type Err = ();
    fn from_str(s: &str) -> std::result::Result<Self, ()> {
        Ok(match s.to_uppercase().as_str() {
            "MOBI" => Self::Mobi,
            "CBZ" => Self::Cbz,
            _ => Self::Epub,
        })
    }
}

pub struct KccOptions {
    pub format: KccFormat,
    pub profile: String,
    pub manga_mode: bool,
}

impl AppService {
    /// Export a downloaded chapter as an EPUB3 file, optionally with device-specific
    /// image optimisation. Returns `(epub_bytes, suggested_filename)`.
    pub async fn export_chapter_epub(
        &self,
        chapter_id: ChapterId,
        profile: DeviceProfile,
    ) -> Result<(Vec<u8>, String)> {
        let info = self.chapter_cbz_path(chapter_id).await?;
        let filename = format!("{} - {}.epub", info.manga_name, info.chapter_title);
        let (cbz_path, chapter_title, manga_name) =
            (info.path, info.chapter_title, info.manga_name);
        let bytes = tokio::task::spawn_blocking(move || {
            build_epub_zip(&cbz_path, &chapter_title, &manga_name, &profile)
        })
        .await
        .map_err(|e| ServiceError::Internal(format!("Task join: {e}")))??;

        Ok((bytes, filename))
    }

    /// Export a downloaded chapter as a Kobo KEPUB file. Returns `(kepub_bytes, filename)`.
    pub async fn export_chapter_kepub(
        &self,
        chapter_id: ChapterId,
        profile: DeviceProfile,
    ) -> Result<(Vec<u8>, String)> {
        let info = self.chapter_cbz_path(chapter_id).await?;
        let filename = format!("{} - {}.kepub.epub", info.manga_name, info.chapter_title);
        let (cbz_path, chapter_title, manga_name) =
            (info.path, info.chapter_title, info.manga_name);
        let bytes = tokio::task::spawn_blocking(move || {
            let epub = build_epub_zip(&cbz_path, &chapter_title, &manga_name, &profile)?;
            kepub_transform(epub, &profile)
        })
        .await
        .map_err(|e| ServiceError::Internal(format!("Task join: {e}")))??;

        Ok((bytes, filename))
    }

    /// Export a downloaded chapter via Kindle Comic Converter (KCC).
    /// Returns `Err(ServiceError::Internal)` if `kcc-c2e` is not in PATH.
    pub async fn export_chapter_kcc(
        &self,
        chapter_id: ChapterId,
        opts: KccOptions,
    ) -> Result<(Vec<u8>, String, &'static str)> {
        if which::which("kcc-c2e").is_err() {
            return Err(ServiceError::Internal(
                "KCC not available: kcc-c2e not found in PATH".into(),
            ));
        }

        let info = self.chapter_cbz_path(chapter_id).await?;
        let (cbz_path, chapter_title, manga_name) =
            (info.path, info.chapter_title, info.manga_name);

        let tmp = tempfile::TempDir::new()
            .map_err(|e| ServiceError::Internal(format!("TempDir: {e}")))?;

        let mut cmd = tokio::process::Command::new("kcc-c2e");
        cmd.arg("-p")
            .arg(&opts.profile)
            .arg("-f")
            .arg(opts.format.as_str())
            .arg("-o")
            .arg(tmp.path());
        if opts.manga_mode {
            cmd.arg("-m");
        }
        cmd.arg(&cbz_path);

        let out = tokio::time::timeout(std::time::Duration::from_secs(300), cmd.output())
            .await
            .map_err(|_| ServiceError::Internal("KCC timed out after 5 minutes".into()))?
            .map_err(|e| ServiceError::Internal(format!("KCC process error: {e}")))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(ServiceError::Internal(format!("KCC failed: {stderr}")));
        }

        let ext = opts.format.ext();
        let mime = opts.format.mime();
        let output_file = std::fs::read_dir(tmp.path())
            .map_err(ServiceError::Io)?
            .filter_map(|e| e.ok())
            .find(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x.eq_ignore_ascii_case(ext))
                    .unwrap_or(false)
            })
            .ok_or_else(|| ServiceError::Internal(format!("KCC produced no .{ext} output file")))?;

        let bytes = std::fs::read(output_file.path()).map_err(ServiceError::Io)?;
        let filename = format!("{manga_name} - {chapter_title}.{ext}");

        Ok((bytes, filename, mime))
    }

    /// Check whether KCC is installed and return its version string if so.
    pub async fn kcc_version() -> Option<String> {
        if which::which("kcc-c2e").is_err() {
            return None;
        }
        let out = tokio::process::Command::new("kcc-c2e")
            .arg("--version")
            .output()
            .await
            .ok()?;
        let raw = String::from_utf8_lossy(&out.stdout);
        Some(raw.trim().to_string())
    }
}

struct ComicInfo {
    writer: Option<String>,
    summary: Option<String>,
    language_iso: Option<String>,
}

fn parse_comic_info(xml_bytes: &[u8]) -> ComicInfo {
    use quick_xml::{Reader, events::Event};

    let mut info = ComicInfo {
        writer: None,
        summary: None,
        language_iso: None,
    };
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut current_tag = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
            }
            Ok(Event::Text(e)) => {
                let text = e.xml_content().map(|c| c.into_owned()).unwrap_or_default();
                match current_tag.as_str() {
                    "Writer" => info.writer = Some(text),
                    "Summary" => info.summary = Some(text),
                    "LanguageISO" => info.language_iso = Some(text),
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    info
}

fn build_epub_zip(
    cbz_path: &Path,
    chapter_title: &str,
    manga_name: &str,
    profile: &DeviceProfile,
) -> Result<Vec<u8>> {
    let file = std::fs::File::open(cbz_path).map_err(ServiceError::Io)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| ServiceError::Internal(format!("Open CBZ: {e}")))?;

    let mut comic_info_bytes: Option<Vec<u8>> = None;
    let mut images: Vec<(String, Vec<u8>)> = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| ServiceError::Internal(format!("CBZ entry: {e}")))?;
        let name = entry.name().to_owned();

        if name.eq_ignore_ascii_case("comicinfo.xml") {
            let mut b = Vec::new();
            entry.read_to_end(&mut b).map_err(ServiceError::Io)?;
            comic_info_bytes = Some(b);
            continue;
        }
        if is_image(&name) {
            let mut b = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut b).map_err(ServiceError::Io)?;
            images.push((name, b));
        }
    }

    images.sort_by(|a, b| a.0.cmp(&b.0));

    if images.is_empty() {
        return Err(ServiceError::NotFound(format!(
            "CBZ has no images: {}",
            cbz_path.display()
        )));
    }

    let comic_info = comic_info_bytes
        .as_deref()
        .map(parse_comic_info)
        .unwrap_or(ComicInfo {
            writer: None,
            summary: None,
            language_iso: None,
        });

    let author = comic_info.writer.as_deref().unwrap_or(manga_name);

    let mut builder = EpubBuilder::new(
        ZipLibrary::new().map_err(|e| ServiceError::Internal(format!("ZipLibrary: {e}")))?,
    )
    .map_err(|e| ServiceError::Internal(format!("EpubBuilder: {e}")))?;

    builder
        .metadata("title", chapter_title)
        .map_err(|e| ServiceError::Internal(format!("EPUB metadata: {e}")))?;
    builder
        .metadata("author", author)
        .map_err(|e| ServiceError::Internal(format!("EPUB metadata: {e}")))?;
    if let Some(summary) = &comic_info.summary {
        builder
            .metadata("description", summary.as_str())
            .map_err(|e| ServiceError::Internal(format!("EPUB metadata: {e}")))?;
    }
    if let Some(lang) = &comic_info.language_iso {
        builder
            .metadata("lang", lang.as_str())
            .map_err(|e| ServiceError::Internal(format!("EPUB metadata: {e}")))?;
    }

    builder.epub_version(EpubVersion::V30);

    let viewport = profile
        .dimensions()
        .map(|(w, h)| format!(r#"<meta name="viewport" content="width={w},height={h}"/>"#))
        .unwrap_or_default();

    for (i, (_filename, raw_bytes)) in images.iter().enumerate() {
        let processed = if *profile == DeviceProfile::Standard {
            raw_bytes.clone()
        } else {
            process_image(raw_bytes, profile)?
        };

        let res_name = format!("img{i:04}.jpg");

        if i == 0 {
            builder
                .add_cover_image(&res_name, Cursor::new(processed.as_slice()), "image/jpeg")
                .map_err(|e| ServiceError::Internal(format!("EPUB cover: {e}")))?;
        } else {
            builder
                .add_resource(&res_name, Cursor::new(processed.as_slice()), "image/jpeg")
                .map_err(|e| ServiceError::Internal(format!("EPUB resource: {e}")))?;
        }

        let xhtml = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Page {page}</title>{viewport}
<style>html,body{{margin:0;padding:0;background:#000}}img{{max-width:100%;height:auto;display:block;margin:0 auto}}</style>
</head>
<body><img src="{res_name}" alt="Page {page}"/></body>
</html>"#,
            page = i + 1,
        );
        builder
            .add_content(
                EpubContent::new(format!("page{i:04}.xhtml"), xhtml.as_bytes())
                    .title(format!("Page {}", i + 1)),
            )
            .map_err(|e| ServiceError::Internal(format!("EPUB content: {e}")))?;
    }

    let mut output = Vec::new();
    builder
        .generate(&mut output)
        .map_err(|e| ServiceError::Internal(format!("EPUB generate: {e}")))?;
    Ok(output)
}

fn process_image(bytes: &[u8], profile: &DeviceProfile) -> Result<Vec<u8>> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| ServiceError::Internal(format!("Decode image: {e}")))?;

    let img = if let Some((max_w, max_h)) = profile.dimensions() {
        fit_within(img, max_w, max_h)
    } else {
        img
    };

    let img = if profile.grayscale() {
        img.grayscale()
    } else {
        img
    };

    let img = if let Some(gamma) = profile.gamma() {
        apply_gamma(img, gamma)
    } else {
        img
    };

    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)
        .map_err(|e| ServiceError::Internal(format!("Encode JPEG: {e}")))?;
    Ok(buf)
}

fn fit_within(img: DynamicImage, max_w: u32, max_h: u32) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    if w <= max_w && h <= max_h {
        return img;
    }
    let scale = (max_w as f32 / w as f32).min(max_h as f32 / h as f32);
    let nw = ((w as f32 * scale) as u32).max(1);
    let nh = ((h as f32 * scale) as u32).max(1);
    img.resize(nw, nh, image::imageops::FilterType::Lanczos3)
}

fn apply_gamma(img: DynamicImage, gamma: f32) -> DynamicImage {
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    let inv = 1.0 / gamma;
    let lut: [u8; 256] =
        std::array::from_fn(|i| ((i as f32 / 255.0).powf(inv) * 255.0).round() as u8);
    let mut out: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(w, h);
    for (x, y, px) in rgba.enumerate_pixels() {
        out.put_pixel(
            x,
            y,
            Rgba([
                lut[px[0] as usize],
                lut[px[1] as usize],
                lut[px[2] as usize],
                px[3],
            ]),
        );
    }
    DynamicImage::ImageRgba8(out)
}

fn kepub_transform(epub_bytes: Vec<u8>, profile: &DeviceProfile) -> Result<Vec<u8>> {
    use zip::{ZipArchive, ZipWriter, write::FileOptions};

    let viewport = profile
        .dimensions()
        .map(|(w, h)| format!(r#"<meta name="viewport" content="width={w},height={h}"/>"#))
        .unwrap_or_default();

    let mut archive = ZipArchive::new(Cursor::new(epub_bytes))
        .map_err(|e| ServiceError::Internal(format!("KEPUB read EPUB: {e}")))?;

    let mut out_buf: Vec<u8> = Vec::new();
    {
        let mut writer = ZipWriter::new(Cursor::new(&mut out_buf));
        let opts: FileOptions<'_, ()> =
            FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| ServiceError::Internal(format!("KEPUB entry: {e}")))?;
            let name = entry.name().to_owned();

            let mut raw = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut raw).map_err(ServiceError::Io)?;

            let patched = if name.ends_with(".opf") {
                patch_opf(&raw)
            } else if name.ends_with(".xhtml") || name.ends_with(".html") {
                patch_xhtml(&raw, &viewport)
            } else {
                raw
            };

            writer
                .start_file(&name, opts)
                .map_err(|e| ServiceError::Internal(format!("KEPUB write: {e}")))?;
            writer.write_all(&patched).map_err(ServiceError::Io)?;
        }
        writer
            .finish()
            .map_err(|e| ServiceError::Internal(format!("KEPUB finalise: {e}")))?;
    }
    Ok(out_buf)
}

fn patch_opf(bytes: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(bytes);
    let inject = concat!(
        "\n    <meta name=\"fixed-layout\" content=\"true\"/>",
        "\n    <meta property=\"rendition:layout\">pre-paginated</meta>",
        "\n    <meta property=\"rendition:spread\">none</meta>",
    );
    s.replacen("</metadata>", &format!("{inject}\n  </metadata>"), 1)
        .into_bytes()
}

fn patch_xhtml(bytes: &[u8], viewport_meta: &str) -> Vec<u8> {
    let s = String::from_utf8_lossy(bytes);
    let s = s.replacen(
        "<html xmlns=\"http://www.w3.org/1999/xhtml\">",
        "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">",
        1,
    );
    if !viewport_meta.is_empty() {
        s.replacen("</head>", &format!("{viewport_meta}\n</head>"), 1)
            .into_bytes()
    } else {
        s.into_bytes()
    }
}

fn is_image(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".jpg")
        || l.ends_with(".jpeg")
        || l.ends_with(".png")
        || l.ends_with(".webp")
        || l.ends_with(".gif")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn standard_has_no_dimensions() {
        assert_eq!(DeviceProfile::Standard.dimensions(), None);
    }

    #[test]
    fn standard_not_grayscale() {
        assert!(!DeviceProfile::Standard.grayscale());
    }

    #[test]
    fn standard_no_gamma() {
        assert_eq!(DeviceProfile::Standard.gamma(), None);
    }

    #[test]
    fn kindle_paperwhite_dimensions() {
        assert_eq!(
            DeviceProfile::KindlePaperwhite.dimensions(),
            Some((1236, 1648))
        );
    }

    #[test]
    fn kindle_paperwhite_grayscale() {
        assert!(DeviceProfile::KindlePaperwhite.grayscale());
    }

    #[test]
    fn kindle_paperwhite_gamma() {
        assert_eq!(DeviceProfile::KindlePaperwhite.gamma(), Some(1.5));
    }

    #[test]
    fn kobo_clara_dimensions() {
        assert_eq!(DeviceProfile::KoboClara.dimensions(), Some((1072, 1448)));
    }

    #[test]
    fn custom_profile_values() {
        let p = DeviceProfile::Custom {
            width: 800,
            height: 600,
            grayscale: false,
            gamma: 2.2,
        };
        assert_eq!(p.dimensions(), Some((800, 600)));
        assert!(!p.grayscale());
        assert!((p.gamma().unwrap() - 2.2).abs() < f32::EPSILON);
    }

    #[test]
    fn from_str_kindle_paperwhite() {
        let p: DeviceProfile = "kindle-pw".parse().unwrap();
        assert_eq!(p, DeviceProfile::KindlePaperwhite);
    }

    #[test]
    fn from_str_unknown_gives_standard() {
        let p: DeviceProfile = "not-a-device".parse().unwrap();
        assert_eq!(p, DeviceProfile::Standard);
    }

    #[test]
    fn kcc_epub_mime_and_ext() {
        assert_eq!(KccFormat::Epub.mime(), "application/epub+zip");
        assert_eq!(KccFormat::Epub.ext(), "epub");
    }

    #[test]
    fn kcc_mobi_mime_and_ext() {
        assert_eq!(KccFormat::Mobi.mime(), "application/x-mobipocket-ebook");
        assert_eq!(KccFormat::Mobi.ext(), "mobi");
    }

    #[test]
    fn kcc_cbz_mime_and_ext() {
        assert_eq!(KccFormat::Cbz.mime(), "application/x-cbz");
        assert_eq!(KccFormat::Cbz.ext(), "cbz");
    }

    #[test]
    fn kcc_as_str_variants() {
        assert_eq!(KccFormat::Epub.as_str(), "EPUB");
        assert_eq!(KccFormat::Mobi.as_str(), "MOBI");
        assert_eq!(KccFormat::Cbz.as_str(), "CBZ");
    }

    #[test]
    fn jpg_is_image() {
        assert!(is_image("cover.jpg"));
    }

    #[test]
    fn jpeg_is_image() {
        assert!(is_image("page.jpeg"));
    }

    #[test]
    fn png_is_image() {
        assert!(is_image("page.png"));
    }

    #[test]
    fn webp_is_image() {
        assert!(is_image("thumb.webp"));
    }

    #[test]
    fn gif_is_image() {
        assert!(is_image("anim.gif"));
    }

    #[test]
    fn case_insensitive_extension() {
        assert!(is_image("COVER.JPG"));
        assert!(is_image("Page.PNG"));
    }

    #[test]
    fn opf_not_image() {
        assert!(!is_image("content.opf"));
    }

    #[test]
    fn xml_not_image() {
        assert!(!is_image("ComicInfo.xml"));
    }

    #[test]
    fn empty_name_not_image() {
        assert!(!is_image(""));
    }

    #[test]
    fn patch_opf_injects_fixed_layout_meta() {
        let opf = b"<metadata></metadata>";
        let patched = patch_opf(opf);
        let s = std::str::from_utf8(&patched).unwrap();
        assert!(s.contains("fixed-layout"));
        assert!(s.contains("pre-paginated"));
        assert!(s.contains("rendition:spread"));
        assert!(s.contains("</metadata>"), "closing tag preserved");
    }

    #[test]
    fn patch_opf_no_metadata_tag_unchanged_length() {
        let opf = b"<package><manifest/></package>";
        let patched = patch_opf(opf);
        assert_eq!(patched, opf);
    }

    #[test]
    fn patch_xhtml_adds_epub_namespace() {
        let xhtml = b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><head></head></html>";
        let patched = patch_xhtml(xhtml, "");
        let s = std::str::from_utf8(&patched).unwrap();
        assert!(s.contains("xmlns:epub=\"http://www.idpf.org/2007/ops\""));
    }

    #[test]
    fn patch_xhtml_inserts_viewport_before_head_close() {
        let xhtml = b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><head></head></html>";
        let viewport = r#"<meta name="viewport" content="width=1072,height=1448"/>"#;
        let patched = patch_xhtml(xhtml, viewport);
        let s = std::str::from_utf8(&patched).unwrap();
        assert!(s.contains(viewport));
        assert!(s.contains("</head>"), "closing head tag preserved");
    }

    #[test]
    fn patch_xhtml_empty_viewport_skips_injection() {
        let xhtml = b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><head></head></html>";
        let patched = patch_xhtml(xhtml, "");
        let s = std::str::from_utf8(&patched).unwrap();
        assert!(!s.contains("viewport"));
    }

    #[test]
    fn parse_comic_info_extracts_writer() {
        let xml = b"<?xml version=\"1.0\"?><ComicInfo><Writer>Author Name</Writer></ComicInfo>";
        let info = parse_comic_info(xml);
        assert_eq!(info.writer.as_deref(), Some("Author Name"));
    }

    #[test]
    fn parse_comic_info_extracts_summary() {
        let xml =
            b"<?xml version=\"1.0\"?><ComicInfo><Summary>A great story.</Summary></ComicInfo>";
        let info = parse_comic_info(xml);
        assert_eq!(info.summary.as_deref(), Some("A great story."));
    }

    #[test]
    fn parse_comic_info_extracts_language() {
        let xml = b"<?xml version=\"1.0\"?><ComicInfo><LanguageISO>en</LanguageISO></ComicInfo>";
        let info = parse_comic_info(xml);
        assert_eq!(info.language_iso.as_deref(), Some("en"));
    }

    #[test]
    fn parse_comic_info_missing_fields_are_none() {
        let xml = b"<?xml version=\"1.0\"?><ComicInfo></ComicInfo>";
        let info = parse_comic_info(xml);
        assert!(info.writer.is_none());
        assert!(info.summary.is_none());
        assert!(info.language_iso.is_none());
    }

    #[test]
    fn parse_comic_info_empty_bytes_does_not_panic() {
        let info = parse_comic_info(b"");
        assert!(info.writer.is_none());
    }
}
