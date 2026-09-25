use super::super::*;
use crate::ids::{MangaId, UserId};

impl AppService {
    /// Resize and JPEG-compress cover bytes. Used for downloaded (non-user) covers.
    fn compress_cover_bytes(bytes: &[u8], max_dim: u32) -> Vec<u8> {
        let img = match image::load_from_memory(bytes) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!("Cover decode failed, storing raw bytes: {e}");
                return bytes.to_vec();
            }
        };
        let img = if img.width() > max_dim || img.height() > max_dim {
            img.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };
        let mut out = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
        match img.write_with_encoder(encoder) {
            Ok(()) => out,
            Err(e) => {
                tracing::warn!("Cover JPEG encode failed, storing raw bytes: {e}");
                bytes.to_vec()
            }
        }
    }

    /// Resize and JPEG-compress a user-uploaded cover image with decoder limits to prevent
    /// decompression bombs. Falls back to raw bytes if decode/encode fails.
    fn compress_cover_bytes_with_limits(bytes: &[u8], max_dim: u32) -> Vec<u8> {
        let cursor = std::io::Cursor::new(bytes);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(256 * 1024 * 1024);

        let img = match image::ImageReader::new(cursor).with_guessed_format() {
            Ok(reader) => {
                let mut reader = reader;
                reader.limits(limits);
                match reader.decode() {
                    Ok(img) => img,
                    Err(e) => {
                        tracing::warn!(
                            "Cover decode failed (limits applied), storing raw bytes: {e}"
                        );
                        return bytes.to_vec();
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Cover format guess failed, storing raw bytes: {e}");
                return bytes.to_vec();
            }
        };
        let img = if img.width() > max_dim || img.height() > max_dim {
            img.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };
        let mut out = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
        match img.write_with_encoder(encoder) {
            Ok(()) => out,
            Err(e) => {
                tracing::warn!("Cover JPEG encode failed, storing raw bytes: {e}");
                bytes.to_vec()
            }
        }
    }

    pub(super) async fn download_and_store_cover(
        &self,
        manga_row_id: MangaId,
        cover_url: &str,
        base_url: &str,
    ) -> Result<()> {
        let library_path = self.settings.read().await.library_path.clone();
        let covers_dir = library_path.join("covers");
        tokio::fs::create_dir_all(&covers_dir).await?;

        let mut headers = rquest::header::HeaderMap::new();
        if let Ok(v) = rquest::header::HeaderValue::from_str(base_url) {
            headers.insert(rquest::header::REFERER, v);
        }

        let source_id: Option<i64> = sqlx::query_scalar("SELECT source_id FROM manga WHERE id = ?")
            .bind(manga_row_id.0)
            .fetch_optional(&self.db_read)
            .await?;
        let client = match source_id {
            Some(id) => self.proxy_client_for_source(id).await,
            None => self.proxy_client.clone(),
        };
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            client.safe_get(cover_url, Some(headers)),
        )
        .await
        .map_err(|_| ServiceError::Internal("Cover download timed out".into()))??;

        if !response.status().is_success() {
            return Err(ServiceError::Internal(format!(
                "Cover download returned {}",
                response.status().as_u16()
            )));
        }

        let content_type = response
            .headers()
            .get(rquest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/jpeg")
            .to_string();

        // An HTML body is worth refusing before it is read; everything else is
        // judged on its magic number below, because CDNs serve perfectly good
        // covers as `application/octet-stream` and a label can lie either way.
        if content_type.starts_with("text/html") {
            return Err(ServiceError::Internal(format!(
                "Expected image for cover, got Content-Type: {}",
                content_type
            )));
        }

        const MAX_COVER_BYTES: usize = 10 * 1024 * 1024;
        let bytes = response
            .bytes_limited(MAX_COVER_BYTES)
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;

        // The cover is re-encoded to JPEG below, so the sniff is purely a gate:
        // it is what stops an error or challenge page being stored as a cover.
        if kani_core::probe::sniff_image_mime(&bytes).is_none() {
            return Err(ServiceError::Internal(format!(
                "Cover is not an image (upstream declared Content-Type: {content_type})"
            )));
        }

        let max_dim = self
            .settings
            .read()
            .await
            .cover_max_dimension
            .unwrap_or(800) as u32;

        let final_bytes =
            tokio::task::spawn_blocking(move || Self::compress_cover_bytes(&bytes, max_dim))
                .await
                .map_err(|e| {
                    ServiceError::Internal(format!("Cover compress task panicked: {e}"))
                })?;

        let filename = format!("{}.jpg", manga_row_id);
        let cover_path = covers_dir.join(&filename);
        let relative = format!("covers/{}", filename);

        self.clear_thumbnails(manga_row_id).await;
        tokio::fs::write(&cover_path, &final_bytes).await?;

        sqlx::query!(
            "UPDATE manga SET local_cover_path = ? WHERE id = ?",
            relative,
            manga_row_id
        )
        .execute(&self.db)
        .await?;

        self.spawn_thumbnail_generation(manga_row_id).await;
        Ok(())
    }

    /// Retries downloading the cover for a single manga. Only attempts if
    /// `local_cover_path IS NULL` (already downloaded covers are skipped).
    pub async fn retry_single_cover(&self, manga_id: MangaId) -> Result<()> {
        struct Row {
            cover_url: String,
            base_url: String,
        }

        let row = sqlx::query_as!(
            Row,
            r#"SELECT m.cover_url as "cover_url!", s.base_url
               FROM manga m JOIN sources s ON s.id = m.source_id
               WHERE m.id = ? AND m.local_cover_path IS NULL
                 AND m.cover_url IS NOT NULL AND m.cover_overridden = FALSE"#,
            manga_id
        )
        .fetch_optional(&self.db_read)
        .await?;

        if let Some(row) = row {
            self.download_and_store_cover(manga_id, &row.cover_url, &row.base_url)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn retry_missing_covers(&self) {
        struct Row {
            id: MangaId,
            cover_url: String,
            base_url: String,
        }

        let rows = match sqlx::query_as!(
            Row,
            r#"SELECT m.id, m.cover_url as "cover_url!", s.base_url
               FROM manga m
               JOIN sources s ON s.id = m.source_id
               WHERE m.local_cover_path IS NULL
                 AND m.cover_url IS NOT NULL
                 AND m.cover_overridden = FALSE"#
        )
        .fetch_all(&self.db_read)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("retry_missing_covers: failed to query manga: {e}");
                return;
            }
        };

        for row in rows {
            match self
                .download_and_store_cover(row.id, &row.cover_url, &row.base_url)
                .await
            {
                Ok(()) => tracing::info!(
                    "retry_missing_covers: downloaded cover for manga {}",
                    row.id
                ),
                Err(e) => tracing::warn!("retry_missing_covers: failed for manga {}: {e}", row.id),
            }
        }
    }

    /// Accepts a user-uploaded cover image, validates it, compresses it, and stores it as the
    /// local cover. Sets `cover_overridden = TRUE` so scans won't replace it.
    pub async fn upload_manga_cover(
        &self,
        manga_id: MangaId,
        bytes: Vec<u8>,
        content_type: &str,
        user_id: UserId,
    ) -> Result<()> {
        if content_type.contains("svg") || content_type.contains("xml") {
            return Err(ServiceError::Validation(
                "SVG images are not permitted as covers".into(),
            ));
        }
        if bytes.first().copied() == Some(b'<') {
            return Err(ServiceError::Validation(
                "SVG images are not permitted as covers".into(),
            ));
        }

        let inferred_mime = infer::get(&bytes).map(|t| t.mime_type()).unwrap_or("");
        if !matches!(
            inferred_mime,
            "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/avif"
        ) {
            return Err(ServiceError::Validation(format!(
                "Unsupported image type: {}",
                if inferred_mime.is_empty() {
                    "unknown"
                } else {
                    inferred_mime
                }
            )));
        }

        let library_path = self.settings.read().await.library_path.clone();
        let covers_dir = library_path.join("covers");
        tokio::fs::create_dir_all(&covers_dir).await?;

        let max_dim = self
            .settings
            .read()
            .await
            .cover_max_dimension
            .unwrap_or(800) as u32;

        let final_bytes = tokio::task::spawn_blocking(move || {
            Self::compress_cover_bytes_with_limits(&bytes, max_dim)
        })
        .await
        .map_err(|e| ServiceError::Internal(format!("Cover compress task panicked: {e}")))?;

        let filename = format!("{manga_id}_local.jpg");
        let cover_path = covers_dir.join(&filename);
        let relative = format!("covers/{filename}");

        self.clear_thumbnails(manga_id).await;
        tokio::fs::write(&cover_path, &final_bytes).await?;

        sqlx::query!(
            "UPDATE manga SET local_cover_path = ?, cover_overridden = TRUE WHERE id = ?",
            relative,
            manga_id,
        )
        .execute(&self.db)
        .await?;

        self.spawn_thumbnail_generation(manga_id).await;
        self.invalidate_library();
        self.audit(Some(user_id), "manga.cover.upload", None, None)
            .await;
        Ok(())
    }

    /// Clears the user-uploaded cover override. If the source cover file still exists on disk
    /// it is restored immediately; otherwise `local_cover_path` is set to NULL so the next
    /// refresh re-downloads it.
    pub async fn clear_manga_cover_override(
        &self,
        manga_id: MangaId,
        user_id: UserId,
    ) -> Result<()> {
        let library_path = self.settings.read().await.library_path.clone();

        let current_rel =
            sqlx::query_scalar!("SELECT local_cover_path FROM manga WHERE id = ?", manga_id)
                .fetch_optional(&self.db_read)
                .await?
                .flatten();

        if let Some(ref rel) = current_rel
            && rel.contains("_local")
        {
            let full = library_path.join(rel);
            match tokio::fs::remove_file(&full).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => tracing::warn!("Failed to delete local cover {full:?}: {e}"),
            }
        }

        let source_rel = format!("covers/{manga_id}.jpg");
        let source_path = library_path.join(&source_rel);
        let restored_path: Option<String> = match tokio::fs::metadata(&source_path).await {
            Ok(_) => Some(source_rel),
            Err(_) => None,
        };

        self.clear_thumbnails(manga_id).await;

        sqlx::query!(
            "UPDATE manga SET local_cover_path = ?, cover_overridden = FALSE WHERE id = ?",
            restored_path,
            manga_id,
        )
        .execute(&self.db)
        .await?;

        if restored_path.is_some() {
            self.spawn_thumbnail_generation(manga_id).await;
        }
        self.invalidate_library();
        self.audit(Some(user_id), "manga.cover.override_cleared", None, None)
            .await;
        Ok(())
    }
}

#[cfg(test)]
mod cover_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::error::ServiceError;

    /// Minimal valid 1×1 JPEG bytes.
    fn tiny_jpeg() -> Vec<u8> {
        vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06,
            0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D,
            0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D,
            0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28,
            0x37, 0x29, 0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32,
            0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01,
            0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01,
            0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02,
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x10,
            0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00,
            0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06,
            0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42,
            0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16,
            0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37,
            0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55,
            0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73,
            0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
            0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5,
            0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA,
            0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6,
            0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
            0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x08,
            0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0xFB, 0xD9,
        ]
    }

    /// Helper: call only the validation-and-limits path, not the full service method.
    fn validate_cover(bytes: Vec<u8>, content_type: &str) -> crate::error::Result<()> {
        if content_type.contains("svg") || content_type.contains("xml") {
            return Err(ServiceError::Validation(
                "SVG images are not permitted as covers".into(),
            ));
        }
        if bytes.first().copied() == Some(b'<') {
            return Err(ServiceError::Validation(
                "SVG images are not permitted as covers".into(),
            ));
        }
        let inferred_mime = infer::get(&bytes).map(|t| t.mime_type()).unwrap_or("");
        if !matches!(
            inferred_mime,
            "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/avif"
        ) {
            return Err(ServiceError::Validation(format!(
                "Unsupported image type: {}",
                if inferred_mime.is_empty() {
                    "unknown"
                } else {
                    inferred_mime
                }
            )));
        }
        Ok(())
    }

    #[test]
    fn svg_rejected_by_content_type() {
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>".to_vec();
        let err = validate_cover(svg, "image/svg+xml").unwrap_err();
        assert!(
            matches!(err, ServiceError::Validation(_)),
            "expected Validation, got {err:?}"
        );
    }

    #[test]
    fn svg_rejected_by_magic_bytes_even_with_jpeg_content_type() {
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>".to_vec();
        let err = validate_cover(svg, "image/jpeg").unwrap_err();
        assert!(matches!(err, ServiceError::Validation(_)));
    }

    #[test]
    fn random_bytes_rejected_as_unknown_mime() {
        let random = vec![0x00, 0xFF, 0x42, 0x17, 0x99, 0xAB, 0xCD];
        let err = validate_cover(random, "image/jpeg").unwrap_err();
        assert!(matches!(err, ServiceError::Validation(_)));
    }

    #[test]
    fn valid_jpeg_magic_bytes_accepted() {
        validate_cover(tiny_jpeg(), "image/jpeg").unwrap();
    }

    #[test]
    fn compress_cover_bytes_with_limits_accepts_jpeg() {
        let out = AppService::compress_cover_bytes_with_limits(&tiny_jpeg(), 800);
        assert!(!out.is_empty(), "compressed output should not be empty");
        assert_eq!(&out[..2], &[0xFF, 0xD8], "output must be a JPEG");
    }
}
