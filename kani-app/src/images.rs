use sha2::{Digest, Sha256};

pub(crate) const THUMBNAIL_SIZES: &[(&str, u32)] =
    &[("xs", 80), ("sm", 160), ("md", 320), ("lg", 640)];

pub(crate) fn parse_thumbnail_formats(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| s == "jpeg")
        .collect()
}

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

pub struct ThumbnailEntry {
    pub size: &'static str,
    pub format: String,
    pub path: String,
    pub file_size: i64,
}

pub fn generate_thumbnails_sync(
    source_bytes: &[u8],
    manga_id: i64,
    library_path: &std::path::Path,
    formats: &[String],
) -> Result<(String, Vec<ThumbnailEntry>), String> {
    use image::ImageReader;
    use std::io::Cursor;

    let hash = hex_sha256(source_bytes);

    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);

    let mut reader = ImageReader::new(Cursor::new(source_bytes))
        .with_guessed_format()
        .map_err(|e| format!("format guess failed: {e}"))?;
    reader.limits(limits);
    let source_img = reader.decode().map_err(|e| format!("decode failed: {e}"))?;

    let thumb_dir = library_path.join("covers").join(manga_id.to_string());
    std::fs::create_dir_all(&thumb_dir).map_err(|e| format!("mkdir failed: {e}"))?;

    let mut entries = Vec::new();

    for &(size_name, max_dim) in THUMBNAIL_SIZES {
        let img = if source_img.width() > max_dim || source_img.height() > max_dim {
            source_img.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3)
        } else {
            source_img.clone()
        };

        for fmt in formats {
            let encoded: Vec<u8> = match fmt.as_str() {
                "jpeg" => {
                    let mut out = Vec::new();
                    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
                    img.write_with_encoder(enc)
                        .map_err(|e| format!("jpeg encode: {e}"))?;
                    out
                }
                _ => continue,
            };

            let rel_path = format!("covers/{manga_id}/{size_name}.jpeg");
            let abs_path = thumb_dir.join(format!("{size_name}.jpeg"));
            let file_size = encoded.len() as i64;

            std::fs::write(&abs_path, &encoded).map_err(|e| format!("write {abs_path:?}: {e}"))?;

            entries.push(ThumbnailEntry {
                size: size_name,
                format: fmt.clone(),
                path: rel_path,
                file_size,
            });
        }
    }

    Ok((hash, entries))
}
