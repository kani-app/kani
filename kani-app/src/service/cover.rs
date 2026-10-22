use super::*;
use crate::ids::MangaId;

impl AppService {
    /// Returns the verified, canonicalised path to the manga's local cover image.
    ///
    /// `local_cover_path` is a raw DB string — NOT processed by `sanitize_filename` —
    /// so path traversal via a compromised DB row is a real risk. This method
    /// calls `assert_within_root` before returning.
    pub async fn get_manga_cover_path(&self, manga_id: MangaId) -> Result<std::path::PathBuf> {
        let row = sqlx::query!("SELECT local_cover_path FROM manga WHERE id = ?", manga_id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Manga {manga_id} not found")))?;

        let relative = row
            .local_cover_path
            .ok_or_else(|| ServiceError::NotFound("No local cover for this manga".into()))?;

        let library_path = self.settings.read().await.library_path.clone();
        let full_path = library_path.join(&relative);

        if !full_path.exists() {
            return Err(ServiceError::NotFound(
                "Cover file not found on disk".into(),
            ));
        }

        kani_core::utilities::assert_within_root(&library_path, &full_path)
            .map_err(|e| ServiceError::Internal(format!("Cover path traversal blocked: {e}")))
    }
}
