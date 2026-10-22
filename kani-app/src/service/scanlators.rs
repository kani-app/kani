use super::*;
use crate::ids::MangaId;

impl AppService {
    pub async fn get_scanlator_prefs(
        &self,
        manga_id: MangaId,
    ) -> Result<Vec<kani_shared::types::ScanlatorPreference>> {
        // SQLite stores bool as INTEGER; we map manually to avoid trait bound issues.
        let rows = sqlx::query!(
            "SELECT id, manga_id, scanlator, priority, blocked FROM scanlator_preferences \
             WHERE manga_id=? ORDER BY priority DESC",
            manga_id
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(ServiceError::Db)?;
        let prefs = rows
            .into_iter()
            .map(|r| kani_shared::types::ScanlatorPreference {
                id: r.id,
                manga_id: r.manga_id,
                scanlator: r.scanlator,
                priority: r.priority,
                blocked: r.blocked,
            })
            .collect();
        Ok(prefs)
    }

    pub async fn set_scanlator_pref(
        &self,
        manga_id: MangaId,
        scanlator: &str,
        priority: i64,
        blocked: bool,
    ) -> Result<()> {
        let blocked_int = blocked as i64;
        sqlx::query!(
            "INSERT INTO scanlator_preferences (manga_id, scanlator, priority, blocked) VALUES (?,?,?,?) \
             ON CONFLICT (manga_id, scanlator) DO UPDATE SET priority = excluded.priority, blocked = excluded.blocked",
            manga_id,
            scanlator,
            priority,
            blocked_int
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// The library-wide defaults, highest priority first.
    pub async fn get_global_scanlator_prefs(
        &self,
    ) -> Result<Vec<kani_shared::types::ScanlatorPreference>> {
        let rows = sqlx::query!(
            "SELECT id, manga_id, scanlator, priority, blocked FROM scanlator_preferences \
             WHERE manga_id IS NULL ORDER BY blocked ASC, priority DESC, scanlator ASC"
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(ServiceError::Db)?;
        Ok(rows
            .into_iter()
            .map(|r| kani_shared::types::ScanlatorPreference {
                id: r.id,
                manga_id: r.manga_id,
                scanlator: r.scanlator,
                priority: r.priority,
                blocked: r.blocked,
            })
            .collect())
    }

    pub async fn set_global_scanlator_pref(
        &self,
        scanlator: &str,
        priority: i64,
        blocked: bool,
    ) -> Result<()> {
        if scanlator.trim().is_empty() {
            return Err(ServiceError::Validation(
                "scanlator name is required".into(),
            ));
        }
        let blocked_int = blocked as i64;
        // NULL never conflicts in SQLite, so ON CONFLICT cannot be used for the
        // global row; the partial unique index is enforced by hand.
        let existing: Option<i64> = sqlx::query_scalar!(
            "SELECT id FROM scanlator_preferences WHERE manga_id IS NULL AND scanlator = ?",
            scanlator
        )
        .fetch_optional(&self.db)
        .await
        .map_err(ServiceError::Db)?;

        match existing {
            Some(id) => {
                sqlx::query!(
                    "UPDATE scanlator_preferences SET priority = ?, blocked = ? WHERE id = ?",
                    priority,
                    blocked_int,
                    id
                )
                .execute(&self.db)
                .await
                .map_err(ServiceError::Db)?;
            }
            None => {
                sqlx::query!(
                    "INSERT INTO scanlator_preferences (manga_id, scanlator, priority, blocked) \
                     VALUES (NULL, ?, ?, ?)",
                    scanlator,
                    priority,
                    blocked_int
                )
                .execute(&self.db)
                .await
                .map_err(ServiceError::Db)?;
            }
        }
        Ok(())
    }

    /// Per-manga preferences, with the library-wide defaults filling any gaps.
    ///
    /// This is what every consumer should ask for: a global default is useless
    /// if the code that picks which version to download only ever looks at the
    /// per-manga rows.
    pub async fn effective_scanlator_prefs(
        &self,
        manga_id: MangaId,
    ) -> Result<Vec<kani_shared::types::ScanlatorPreference>> {
        let rows = sqlx::query!(
            "SELECT id, manga_id, scanlator, priority, blocked FROM scanlator_preferences \
             WHERE manga_id = ?1 \
             UNION ALL \
             SELECT id, manga_id, scanlator, priority, blocked FROM scanlator_preferences \
             WHERE manga_id IS NULL AND scanlator NOT IN \
               (SELECT scanlator FROM scanlator_preferences WHERE manga_id = ?1) \
             ORDER BY priority DESC",
            manga_id
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(ServiceError::Db)?;
        Ok(rows
            .into_iter()
            .map(|r| kani_shared::types::ScanlatorPreference {
                id: r.id,
                manga_id: r.manga_id,
                scanlator: r.scanlator,
                priority: r.priority,
                blocked: r.blocked,
            })
            .collect())
    }

    /// Scanlator names across the whole library, commonest first, so the
    /// settings UI can offer real choices instead of a free-text box.
    pub async fn scanlators_by_usage(&self) -> Result<Vec<(String, i64)>> {
        let rows = sqlx::query!(
            "SELECT COALESCE(c.scanlator, 'Unknown') AS scanlator_name, COUNT(*) AS n \
             FROM chapters c JOIN manga m ON m.id = c.manga_id \
             WHERE m.deleted_at IS NULL \
             GROUP BY scanlator_name ORDER BY n DESC, scanlator_name ASC LIMIT 200"
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(ServiceError::Db)?;
        Ok(rows.into_iter().map(|r| (r.scanlator_name, r.n)).collect())
    }

    pub async fn delete_scanlator_pref(&self, id: i64) -> Result<()> {
        sqlx::query!("DELETE FROM scanlator_preferences WHERE id=?", id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn get_chapter_scanlators(&self, manga_id: MangaId) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar!(
            "SELECT DISTINCT scanlator FROM chapters WHERE manga_id = ? AND scanlator IS NOT NULL ORDER BY scanlator",
            manga_id
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows.into_iter().flatten().collect())
    }

    pub async fn get_chapter_languages(&self, manga_id: MangaId) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar!(
            "SELECT DISTINCT language FROM chapters WHERE manga_id = ? ORDER BY language",
            manga_id
        )
        .fetch_all(&self.db_read)
        .await?;
        Ok(rows)
    }

    /// Returns the scanlator mode for a manga, defaulting to `'priority'`.
    pub(crate) async fn get_scanlator_mode(&self, manga_id: MangaId) -> Result<String> {
        let mode = sqlx::query_scalar!(
            "SELECT COALESCE(scanlator_mode, 'priority') FROM manga WHERE id = ?",
            manga_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .unwrap_or_else(|| "priority".into());
        Ok(mode)
    }

    /// Sets the scanlator mode for a manga ('priority' or 'whitelist').
    pub async fn set_scanlator_mode(&self, manga_id: MangaId, mode: &str) -> Result<()> {
        if mode != "priority" && mode != "whitelist" {
            return Err(ServiceError::Validation(
                "scanlator_mode must be 'priority' or 'whitelist'".into(),
            ));
        }
        sqlx::query!(
            "UPDATE manga SET scanlator_mode = ? WHERE id = ?",
            mode,
            manga_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }
}
