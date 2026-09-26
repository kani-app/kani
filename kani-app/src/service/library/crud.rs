use super::super::*;
use super::filter::LibraryFilter;
use crate::ids::UserId;
use std::sync::Arc;

fn compute_filter_hash(f: &LibraryFilter) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f.search.hash(&mut h);
    f.status_filter.hash(&mut h);
    f.tag_filter.hash(&mut h);
    f.author_filter.hash(&mut h);
    f.artist_filter.hash(&mut h);
    f.category_filter.hash(&mut h);
    f.reading_status_filter.hash(&mut h);
    f.hide_no_unread.hash(&mut h);
    f.hide_completed_status.hash(&mut h);
    f.source_id.hash(&mut h);
    f.sort_by.to_sql_order().hash(&mut h);
    f.include_trashed.hash(&mut h);
    f.manga_id_filter.hash(&mut h);
    h.finish()
}

impl AppService {
    pub async fn get_manga_by_id(&self, id: MangaId) -> Result<crate::models::Manga> {
        sqlx::query_as!(crate::models::Manga, "SELECT * FROM manga WHERE id = ?", id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Manga {id} not found")))
    }

    /// Clears the "new chapters were filtered out by your rules" signal that
    /// drives the dismissable banner on the manga-details page.
    pub async fn dismiss_suppressed_chapters(&self, id: MangaId) -> Result<()> {
        sqlx::query!(
            "UPDATE manga SET suppressed_chapter_count = 0 WHERE id = ?",
            id
        )
        .execute(&self.db)
        .await?;
        self.invalidate_library();
        Ok(())
    }

    /// Returns one page (20 rows) of the library ordered by id ASC (order=0) or DESC (order=1).
    pub async fn get_library(&self, page: i32, order: i32) -> Result<Vec<crate::models::Manga>> {
        let order_sql = if order == 1 { "id DESC" } else { "id ASC" };
        let offset = (page - 1).max(0) * 20;
        // sqlx macro can't bind ORDER BY; use query_as with a runtime-built SQL.
        let sql = format!("SELECT * FROM manga ORDER BY {order_sql} LIMIT 20 OFFSET {offset}");
        sqlx::query_as::<_, crate::models::Manga>(&sql)
            .fetch_all(&self.db_read)
            .await
            .map_err(Into::into)
    }

    /// Deletes a manga entry together with its directory on disk (best-effort) and
    /// its cover file. Succeeds even if the files are already absent.
    pub async fn delete_manga(&self, id: MangaId, user_id: UserId) -> Result<()> {
        let row = sqlx::query!("SELECT name, local_cover_path FROM manga WHERE id = ?", id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Manga {id} not found")))?;

        let library_path = self.settings.read().await.library_path.clone();

        let safe_name = format!(
            "{} - {}",
            kani_core::utilities::sanitize_filename(&row.name),
            id
        );
        let dir_path = library_path.join(&safe_name);
        match tokio::fs::remove_dir_all(&dir_path).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(ServiceError::Io(e)),
        }

        if let Some(cover_rel) = row.local_cover_path {
            let cover_path = library_path.join(&cover_rel);
            match kani_core::utilities::assert_within_root(&library_path, &cover_path) {
                Ok(safe_path) => match tokio::fs::remove_file(&safe_path).await {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => tracing::warn!("Failed to remove cover {:?}: {e}", safe_path),
                },
                Err(e) => tracing::warn!("Cover path traversal blocked: {e}"),
            }
        }

        sqlx::query!("DELETE FROM manga WHERE id = ?", id)
            .execute(&self.db)
            .await?;

        self.invalidate_library();
        self.audit(Some(user_id), "manga.delete", Some(&row.name), None)
            .await;
        self.fire_webhooks(crate::service::webhooks::WebhookPayload::MangaDeleted {
            manga_id: id,
            manga_name: row.name.clone(),
        })
        .await;
        Ok(())
    }

    /// Filtered/paginated library query. Returns (rows, has_next_page, total_pages).
    pub async fn get_library_filtered(
        &self,
        user_id: UserId,
        f: &LibraryFilter,
    ) -> Result<(Vec<crate::models::LibraryManga>, bool, Option<u32>)> {
        use kani_shared::types::MangaSortOrder;

        let filter_hash = compute_filter_hash(f);

        if let Some(cached) = self
            .cache
            .get_library_listing(user_id.0, filter_hash, f.page, f.page_size)
            .await
        {
            let (rows, has_next, total) = (*cached).clone();
            return Ok((rows, has_next, total));
        }

        let page = f.page;
        let page_size = f.page_size;
        let offset = (page - 1).max(0) * page_size;
        let use_fts = f.search.is_some();

        let need_umt = f.sort_by.needs_tracking_join()
            || f.reading_status_filter.is_some()
            || f.hide_completed_status;

        let mut qb = sqlx::QueryBuilder::new(
            "SELECT m.id, m.source_id, COALESCE(m.local_name, m.name) AS name, m.cover_url, m.local_cover_path, m.cover_hash, s.base_url, \
             m.is_orphaned, \
             (SELECT il.status FROM manga_import_links il WHERE il.manga_id = m.id) \
               AS import_link_status, \
             COUNT(*) OVER() AS total_count, \
             (SELECT COUNT(*) FROM chapters c2 \
              LEFT JOIN user_manga_tracking umt2 ON umt2.manga_id = m.id AND umt2.user_id = ",
        );
        qb.push_bind(user_id);
        qb.push(
            " WHERE c2.manga_id = m.id \
              AND c2.discovered_at IS NOT NULL \
              AND c2.discovered_at > COALESCE(umt2.last_seen_at, m.created_at)) AS new_chapter_count, \
             resume.resume_chapter_id, resume.resume_chapter_number, \
             resume.resume_last_page, resume.resume_page_count \
             FROM manga m JOIN sources s ON m.source_id = s.id \
             LEFT JOIN (\
               SELECT c.manga_id, c.id AS resume_chapter_id, c.chapter_number AS resume_chapter_number, \
                      uct.last_page_read AS resume_last_page, COALESCE(c.page_count, 0) AS resume_page_count, \
                      ROW_NUMBER() OVER (PARTITION BY c.manga_id ORDER BY c.chapter_number ASC) AS rn \
               FROM chapters c \
               JOIN user_chapter_tracking uct ON uct.chapter_id = c.id \
               WHERE uct.user_id = ",
        );
        qb.push_bind(user_id);
        qb.push(
            " AND uct.is_read = 0 AND uct.last_page_read > 0 AND c.download_status = 2\
             ) resume ON resume.manga_id = m.id AND resume.rn = 1",
        );

        if use_fts {
            qb.push(" JOIN manga_fts ON manga_fts.manga_id = m.id");
        }

        if need_umt {
            qb.push(" LEFT JOIN user_manga_tracking umt ON umt.manga_id = m.id AND umt.user_id = ");
            qb.push_bind(user_id);
        }

        qb.push(" WHERE 1=1");

        if !f.include_trashed {
            qb.push(" AND m.deleted_at IS NULL");
        }

        if let Some(ref s) = f.search {
            let fts_term = format!("\"{}\"*", s.replace('"', "\"\""));
            qb.push(" AND manga_fts MATCH ");
            qb.push_bind(fts_term);
        }
        if let Some(v) = f.status_filter {
            qb.push(" AND m.status = ");
            qb.push_bind(v);
        }
        if let Some(v) = f.tag_filter {
            qb.push(" AND EXISTS (SELECT 1 FROM manga_tags mt WHERE mt.manga_id = m.id AND mt.tag_id = ");
            qb.push_bind(v);
            qb.push(")");
        }
        if let Some(v) = f.author_filter {
            qb.push(" AND EXISTS (SELECT 1 FROM manga_people mp WHERE mp.manga_id = m.id AND mp.role = 'author' AND mp.person_id = ");
            qb.push_bind(v);
            qb.push(")");
        }
        if let Some(v) = f.artist_filter {
            qb.push(" AND EXISTS (SELECT 1 FROM manga_people mp WHERE mp.manga_id = m.id AND mp.role = 'artist' AND mp.person_id = ");
            qb.push_bind(v);
            qb.push(")");
        }
        if let Some(v) = f.category_filter {
            qb.push(" AND EXISTS (SELECT 1 FROM manga_categories mc WHERE mc.manga_id = m.id AND mc.category_id = ");
            qb.push_bind(v);
            qb.push(")");
        }
        if let Some(ids) = &f.manga_id_filter {
            if ids.is_empty() {
                qb.push(" AND 1=0");
            } else {
                qb.push(" AND m.id IN (");
                let mut sep = qb.separated(", ");
                for id in ids {
                    sep.push_bind(*id);
                }
                qb.push(")");
            }
        }
        if let Some(v) = f.reading_status_filter {
            qb.push(" AND umt.status = ");
            qb.push_bind(v);
        }
        if f.hide_completed_status {
            qb.push(" AND (umt.status IS NULL OR umt.status != 4)");
        }
        if f.hide_no_unread {
            qb.push(
                " AND EXISTS (\
                   SELECT 1 FROM chapters c WHERE c.manga_id = m.id \
                   AND NOT EXISTS (\
                       SELECT 1 FROM chapters c2 \
                       JOIN user_chapter_tracking uct ON uct.chapter_id = c2.id \
                       WHERE c2.manga_id = c.manga_id \
                         AND c2.chapter_number = c.chapter_number \
                         AND uct.user_id = ",
            );
            qb.push_bind(user_id);
            qb.push(" AND uct.is_read = true))");
        }

        if let Some(v) = f.source_id {
            qb.push(" AND m.source_id = ");
            qb.push_bind(v);
        }

        let limit = page_size + 1;

        if use_fts {
            qb.push(" ORDER BY manga_fts.rank");
        } else if matches!(f.sort_by, MangaSortOrder::LastReadDesc) {
            qb.push(
                " ORDER BY (SELECT MAX(uct2.last_read_at) \
                   FROM user_chapter_tracking uct2 \
                   JOIN chapters lrc ON lrc.id = uct2.chapter_id \
                   WHERE lrc.manga_id = m.id AND uct2.user_id = ",
            );
            qb.push_bind(user_id);
            qb.push(") DESC NULLS LAST, m.name ASC");
        } else {
            qb.push(format!(" ORDER BY {}", f.sort_by.to_sql_order()));
        }

        qb.push(format!(" LIMIT {} OFFSET ", limit));
        qb.push_bind(offset);

        let mut records = qb
            .build_query_as::<crate::models::LibraryManga>()
            .fetch_all(&self.db_read)
            .await?;

        let has_next_page = records.len() == limit as usize;
        let total_count = records.first().map(|r| r.total_count).unwrap_or(0);
        records.truncate(page_size as usize);

        if let Some(ref term) = f.search {
            self.attribute_search_matches(term, &mut records).await;
        }
        let ps = page_size as i64;
        let total_pages = Some(((total_count + ps - 1) / ps).max(0) as u32);

        self.cache
            .insert_library_listing(
                user_id.0,
                filter_hash,
                page,
                page_size,
                Arc::new((records.clone(), has_next_page, total_pages)),
            )
            .await;

        Ok((records, has_next_page, total_pages))
    }

    /// Records which indexed field each row matched on, for rows the search did
    /// not match by title.
    ///
    /// Runs as its own query because FTS5's `highlight`/`snippet` are refused in
    /// the listing query, which uses a window function for its total count.
    /// Failure leaves the captions unset rather than failing the listing.
    async fn attribute_search_matches(
        &self,
        term: &str,
        records: &mut [crate::models::LibraryManga],
    ) {
        if records.is_empty() {
            return;
        }
        let fts_term = format!("\"{}\"*", term.replace('"', "\"\""));

        let mut qb = sqlx::QueryBuilder::new(
            "SELECT manga_id, \
               CASE WHEN instr(highlight(manga_fts, 1, char(2), char(3)), char(2)) > 0 \
                      OR instr(highlight(manga_fts, 2, char(2), char(3)), char(2)) > 0 THEN NULL \
                    WHEN instr(highlight(manga_fts, 4, char(2), char(3)), char(2)) > 0 THEN 'author' \
                    WHEN instr(highlight(manga_fts, 3, char(2), char(3)), char(2)) > 0 THEN 'description' \
                    ELSE NULL END AS match_field, \
               CASE WHEN instr(highlight(manga_fts, 1, char(2), char(3)), char(2)) > 0 \
                      OR instr(highlight(manga_fts, 2, char(2), char(3)), char(2)) > 0 THEN NULL \
                    WHEN instr(highlight(manga_fts, 4, char(2), char(3)), char(2)) > 0 \
                      THEN snippet(manga_fts, 4, '', '', '…', 6) \
                    WHEN instr(highlight(manga_fts, 3, char(2), char(3)), char(2)) > 0 \
                      THEN snippet(manga_fts, 3, '', '', '…', 12) \
                    ELSE NULL END AS match_text \
             FROM manga_fts WHERE manga_fts MATCH ",
        );
        qb.push_bind(fts_term);
        qb.push(" AND manga_id IN (");
        let mut sep = qb.separated(", ");
        for record in records.iter() {
            sep.push_bind(record.id.0);
        }
        qb.push(")");

        let attributed: Vec<(i64, Option<String>, Option<String>)> =
            match qb.build_query_as().fetch_all(&self.db_read).await {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::warn!(%e, "could not attribute library search matches");
                    return;
                }
            };

        for (manga_id, field, text) in attributed {
            if let Some(record) = records.iter_mut().find(|r| r.id.0 == manga_id) {
                record.match_field = field;
                record.match_text = text;
            }
        }
    }

    /// Returns full manga details including parsed authors, artists, and tags.
    /// URL signing and markdown rendering are the caller's responsibility.
    pub async fn get_local_manga_details(
        &self,
        id: MangaId,
    ) -> Result<crate::models::LocalMangaDetails> {
        use kani_shared::types::NamedItem;

        let manga = sqlx::query_as!(crate::models::Manga, "SELECT * FROM manga WHERE id = ?", id)
            .fetch_optional(&self.db_read)
            .await?
            .ok_or_else(|| ServiceError::NotFound(format!("Manga {id} not found")))?;

        let source = sqlx::query_as!(
            kani_shared::types::Source,
            "SELECT s.id, s.name, s.version, s.base_url, s.enabled, s.favourited, \
             s.unrestricted_http, s.browser_enabled, s.download_concurrency, \
             s.icon, s.description, s.languages, s.schema_version, \
             scb.state as circuit_state \
             FROM sources s \
             LEFT JOIN source_circuit_breakers scb ON scb.source_id = s.id \
             WHERE s.id = ?",
            manga.source_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound("Source not found".into()))?;

        let record = sqlx::query!(
            r#"SELECT
                (SELECT json_group_array(json_object('id', p.id, 'name', p.name))
                 FROM manga_people mp JOIN people p ON mp.person_id = p.id
                 WHERE mp.manga_id = m.id AND role = 'author') as "authors!: String",
                (SELECT json_group_array(json_object('id', p.id, 'name', p.name))
                 FROM manga_people mp JOIN people p ON mp.person_id = p.id
                 WHERE mp.manga_id = m.id AND role = 'artist') as "artists!: String",
                (SELECT json_group_array(json_object('id', t.id, 'name', t.name))
                 FROM manga_tags mt JOIN tags t ON mt.tag_id = t.id
                 WHERE mt.manga_id = m.id) as "tags!: String",
                (SELECT json_group_array(name) FROM manga_local_authors
                 WHERE manga_id = m.id AND role = 'author') as "local_authors: String",
                (SELECT json_group_array(name) FROM manga_local_authors
                 WHERE manga_id = m.id AND role = 'artist') as "local_artists: String",
                (SELECT json_group_array(name) FROM manga_local_tags
                 WHERE manga_id = m.id) as "local_tags: String",
                EXISTS(SELECT 1 FROM manga_local_authors WHERE manga_id = m.id) as "has_local_people: bool",
                EXISTS(SELECT 1 FROM manga_local_tags WHERE manga_id = m.id) as "has_local_tags: bool"
               FROM manga m WHERE m.id = ?"#,
            id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("Manga {id} not found")))?;

        let src_authors =
            serde_json::from_str::<Vec<NamedItem>>(&record.authors).unwrap_or_default();
        let src_artists =
            serde_json::from_str::<Vec<NamedItem>>(&record.artists).unwrap_or_default();
        let src_tags = serde_json::from_str::<Vec<NamedItem>>(&record.tags).unwrap_or_default();

        let loc_authors: Vec<String> = record
            .local_authors
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let loc_artists: Vec<String> = record
            .local_artists
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let loc_tags: Vec<String> = record
            .local_tags
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        let eff_authors = if record.has_local_people {
            loc_authors
                .iter()
                .map(|n| NamedItem {
                    id: 0,
                    name: n.clone(),
                })
                .collect()
        } else {
            src_authors.clone()
        };
        let eff_artists = if record.has_local_people {
            loc_artists
                .iter()
                .map(|n| NamedItem {
                    id: 0,
                    name: n.clone(),
                })
                .collect()
        } else {
            src_artists.clone()
        };
        let eff_tags = if record.has_local_tags {
            loc_tags
                .iter()
                .map(|n| NamedItem {
                    id: 0,
                    name: n.clone(),
                })
                .collect()
        } else {
            src_tags.clone()
        };

        Ok(crate::models::LocalMangaDetails {
            auto_scan: manga.auto_scan,
            manga,
            source,
            authors: eff_authors,
            artists: eff_artists,
            tags: eff_tags,
            source_authors: src_authors,
            source_artists: src_artists,
            source_tags: src_tags,
            local_authors: loc_authors,
            local_artists: loc_artists,
            local_tags: loc_tags,
            has_local_people: record.has_local_people,
            has_local_tags: record.has_local_tags,
            import_link_status: sqlx::query_scalar!(
                "SELECT status FROM manga_import_links WHERE manga_id = ?",
                id
            )
            .fetch_optional(&self.db_read)
            .await
            .ok()
            .flatten(),
            chapter_count: sqlx::query_scalar!(
                "SELECT COUNT(*) FROM chapters WHERE manga_id = ? AND is_orphaned = FALSE",
                id
            )
            .fetch_one(&self.db_read)
            .await
            .unwrap_or(0) as i64,
        })
    }

    /// Returns the library DB id of a manga from a given source, if it is in the library.
    pub async fn check_in_library(&self, source_id: i64, manga_id: &str) -> Result<Option<i64>> {
        sqlx::query_scalar!(
            "SELECT id FROM manga WHERE source_manga_id = ? AND source_id = ?",
            manga_id,
            source_id
        )
        .fetch_optional(&self.db_read)
        .await
        .map_err(Into::into)
    }

    /// Returns the 50 most recent chapter updates (paginated). Returns (items, has_next_page, total_pages).
    pub async fn get_recent_updates(
        &self,
        page: i32,
    ) -> Result<(Vec<kani_shared::types::RecentUpdateItem>, bool, Option<u32>)> {
        let offset = (page - 1) * 50;
        let mut items = sqlx::query_as!(
            kani_shared::types::RecentUpdateItem,
            "SELECT m.id as manga_id, m.source_id, COALESCE(m.local_name, m.name) as manga_name, m.cover_url, m.local_cover_path, m.cover_hash,
                    s.base_url, c.id as chapter_id, c.chapter_number,
                    c.name as chapter_name, c.discovered_at,
                    (c.download_status = 2) as \"is_downloaded: bool\"
             FROM chapters c
             JOIN manga m ON c.manga_id = m.id
             JOIN sources s ON m.source_id = s.id
             WHERE c.discovered_at IS NOT NULL
             ORDER BY DATE(c.discovered_at) DESC, c.chapter_number DESC LIMIT 51 OFFSET ?",
            offset
        )
        .fetch_all(&self.db_read)
        .await?;

        let has_next_page = items.len() > 50;
        if has_next_page {
            items.truncate(50);
        }

        let total_count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM chapters c WHERE c.discovered_at IS NOT NULL"
        )
        .fetch_one(&self.db_read)
        .await?;
        let total_pages = Some(((total_count as i64 + 49) / 50).max(0) as u32);

        Ok((items, has_next_page, total_pages))
    }

    /// Updates the local metadata override columns for a manga.
    /// Pass `None` for any field to leave it unchanged.
    /// For people/tags, pass `Some(vec![])` to clear the override entirely.
    pub async fn update_local_metadata(
        &self,
        manga_id: MangaId,
        update: crate::models::LocalMetadataUpdate,
        user_id: UserId,
    ) -> Result<()> {
        sqlx::query!(
            "UPDATE manga SET local_name = ?, local_description = ?, local_status = ? WHERE id = ?",
            update.local_name,
            update.local_description,
            update.local_status,
            manga_id,
        )
        .execute(&self.db)
        .await?;

        if update.authors.is_some() || update.artists.is_some() {
            sqlx::query!(
                "DELETE FROM manga_local_authors WHERE manga_id = ?",
                manga_id
            )
            .execute(&self.db)
            .await?;

            if let Some(authors) = &update.authors {
                for name in authors {
                    sqlx::query!(
                        "INSERT INTO manga_local_authors (manga_id, name, role) VALUES (?, ?, 'author')",
                        manga_id,
                        name,
                    )
                    .execute(&self.db)
                    .await?;
                }
            }
            if let Some(artists) = &update.artists {
                for name in artists {
                    sqlx::query!(
                        "INSERT INTO manga_local_authors (manga_id, name, role) VALUES (?, ?, 'artist')",
                        manga_id,
                        name,
                    )
                    .execute(&self.db)
                    .await?;
                }
            }
        }

        if let Some(tags) = &update.tags {
            sqlx::query!("DELETE FROM manga_local_tags WHERE manga_id = ?", manga_id)
                .execute(&self.db)
                .await?;
            for name in tags {
                sqlx::query!(
                    "INSERT INTO manga_local_tags (manga_id, name) VALUES (?, ?)",
                    manga_id,
                    name,
                )
                .execute(&self.db)
                .await?;
            }
        }

        self.update_manga_fts(manga_id).await.unwrap_or_else(|e| {
            tracing::warn!("FTS update failed for manga {manga_id}: {e}");
        });
        self.invalidate_library();
        self.audit(Some(user_id), "manga.local_metadata.update", None, None)
            .await;
        Ok(())
    }

    pub async fn update_manga_fts(&self, manga_id: MangaId) -> Result<()> {
        sqlx::query("DELETE FROM manga_fts WHERE manga_id = ?")
            .bind(manga_id)
            .execute(&self.db)
            .await?;
        sqlx::query(
            "INSERT INTO manga_fts(manga_id, name, local_name, description, authors) \
             SELECT m.id, m.name, m.local_name, m.description, \
                    COALESCE((SELECT GROUP_CONCAT(n, ' ') FROM (\
                        SELECT p.name AS n FROM manga_people mp \
                        JOIN people p ON mp.person_id = p.id \
                        WHERE mp.manga_id = m.id \
                        UNION ALL \
                        SELECT name AS n FROM manga_local_authors WHERE manga_id = m.id\
                    )), '') \
             FROM manga m WHERE m.id = ?",
        )
        .bind(manga_id)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub async fn trash_manga(&self, id: MangaId, user_id: UserId) -> Result<()> {
        let exists = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM manga WHERE id = ? AND deleted_at IS NULL",
            id
        )
        .fetch_one(&self.db_read)
        .await?;
        if exists == 0 {
            return Err(ServiceError::NotFound(format!("Manga {id} not found")));
        }
        let now = time::OffsetDateTime::now_utc();
        sqlx::query!("UPDATE manga SET deleted_at = ? WHERE id = ?", now, id)
            .execute(&self.db)
            .await?;
        self.invalidate_library();
        self.audit(Some(user_id), "manga.trash", None, None).await;
        Ok(())
    }

    pub async fn untrash_manga(&self, id: MangaId, user_id: UserId) -> Result<()> {
        let exists = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM manga WHERE id = ? AND deleted_at IS NOT NULL",
            id
        )
        .fetch_one(&self.db_read)
        .await?;
        if exists == 0 {
            return Err(ServiceError::NotFound(format!(
                "Manga {id} not found in trash"
            )));
        }
        sqlx::query!("UPDATE manga SET deleted_at = NULL WHERE id = ?", id)
            .execute(&self.db)
            .await?;
        self.invalidate_library();
        self.audit(Some(user_id), "manga.untrash", None, None).await;
        Ok(())
    }

    pub async fn list_trash(&self) -> Result<Vec<crate::models::Manga>> {
        sqlx::query_as!(
            crate::models::Manga,
            "SELECT * FROM manga WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC"
        )
        .fetch_all(&self.db_read)
        .await
        .map_err(Into::into)
    }

    pub async fn purge_all_trash(&self) -> Result<u64> {
        let rows = sqlx::query!(
            "SELECT id, name, local_cover_path FROM manga WHERE deleted_at IS NOT NULL"
        )
        .fetch_all(&self.db_read)
        .await?;

        let count = rows.len() as u64;
        let library_path = self.settings.read().await.library_path.clone();

        for row in rows {
            let safe_name = format!(
                "{} - {}",
                kani_core::utilities::sanitize_filename(&row.name),
                row.id
            );
            let dir = library_path.join(&safe_name);
            match tokio::fs::remove_dir_all(&dir).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => tracing::warn!("purge_all_trash: remove_dir_all {:?}: {e}", dir),
            }
            if let Some(cover_rel) = row.local_cover_path {
                let cover_path = library_path.join(&cover_rel);
                match tokio::fs::remove_file(&cover_path).await {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => tracing::warn!("purge_all_trash: remove cover {:?}: {e}", cover_path),
                }
            }
        }

        sqlx::query!("DELETE FROM manga WHERE deleted_at IS NOT NULL")
            .execute(&self.db)
            .await?;

        self.invalidate_library();
        Ok(count)
    }

    pub(crate) async fn purge_expired_trash(&self, days: u32) -> crate::error::Result<u64> {
        let days_i64 = days as i64;
        let cutoff = time::OffsetDateTime::now_utc() - time::Duration::days(days_i64);
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM manga WHERE deleted_at IS NOT NULL AND deleted_at < ?",
            cutoff
        )
        .fetch_one(&self.db_read)
        .await?;

        let expired = sqlx::query!(
            "SELECT id, name, local_cover_path FROM manga \
             WHERE deleted_at IS NOT NULL AND deleted_at < ?",
            cutoff
        )
        .fetch_all(&self.db_read)
        .await?;

        let library_path = self.settings.read().await.library_path.clone();

        for row in expired {
            let id = row.id;
            let name = row.name;
            let cover_path = row.local_cover_path;
            let safe_name = format!(
                "{} - {}",
                kani_core::utilities::sanitize_filename(&name),
                id
            );
            let dir = library_path.join(&safe_name);
            match tokio::fs::remove_dir_all(&dir).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => tracing::warn!("purge_expired_trash: remove_dir_all {:?}: {e}", dir),
            }
            if let Some(cover_rel) = cover_path {
                let cover_path = library_path.join(&cover_rel);
                match tokio::fs::remove_file(&cover_path).await {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        tracing::warn!("purge_expired_trash: remove cover {:?}: {e}", cover_path)
                    }
                }
            }
        }

        sqlx::query!(
            "DELETE FROM manga WHERE deleted_at IS NOT NULL AND deleted_at < ?",
            cutoff
        )
        .execute(&self.db)
        .await?;

        self.invalidate_library();
        Ok(count as u64)
    }
}
