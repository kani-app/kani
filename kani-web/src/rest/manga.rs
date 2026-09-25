//! Per-manga operations, metadata, covers & download-rule routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/manga/{id}", get(get_manga).delete(delete_manga))
        .route("/manga/{id}/untrash", post(untrash_manga_handler))
        .route("/manga/untrash", post(untrash_manga_by_token_handler))
        .route(
            "/trash",
            get(list_trash_handler).delete(purge_trash_all_handler),
        )
        .route("/trash/{id}", delete(purge_trash_one_handler))
        .route(
            "/manga/{id}/cover",
            post(upload_manga_cover_handler).delete(clear_manga_cover_handler),
        )
        .route("/manga/{id}/details", get(get_local_manga_details))
        .route(
            "/manga/{id}/chapters",
            get(get_local_chapters)
                .route_layer(axum::middleware::from_fn(crate::etag::etag_middleware)),
        )
        .route("/manga/{id}/chapter_ids", get(get_chapter_ids))
        .route(
            "/manga/{id}/upgrade-auto-replace",
            put(set_upgrade_auto_replace),
        )
        .route("/me/upgrades", get(get_all_upgrades))
        .route("/me/notify-prefs", get(get_notify_prefs))
        .route("/chapters/{id}/upgrade", post(apply_chapter_upgrade))
        .route(
            "/chapters/{id}/upgrade/dismiss",
            post(dismiss_chapter_upgrade),
        )
        .route("/manga/{id}/download_all", post(download_all))
        .route("/manga/{id}/cancel_all", post(cancel_all_downloads))
        .route("/manga/{id}/refresh", post(refresh_manga))
        .route("/manga/{id}/scan", post(scan_manga))
        .route(
            "/manga/{id}/dismiss-suppressed",
            post(dismiss_suppressed_chapters),
        )
        .route(
            "/manga/{id}/toggle_auto_download",
            post(toggle_auto_download),
        )
        .route("/manga/{id}/toggle_auto_scan", post(toggle_auto_scan_manga))
        .route("/manga/{id}/relink", post(relink_manga))
        .route(
            "/manga/{id}/toggle_download_all_preferred",
            post(toggle_download_all_preferred),
        )
        .route("/manga/{id}/notes", patch(update_manga_notes))
        .route(
            "/manga/{id}/local_metadata",
            patch(update_local_metadata_handler),
        )
        .route("/manga/{id}/seen", patch(mark_manga_seen))
        .route("/manga/{id}/preview_migration", post(preview_migration))
        .route("/manga/{id}/migrate", post(migrate_manga_handler))
        .route("/migrations/match", post(match_migration_targets))
        .route("/migrations/bulk", post(submit_bulk_migration))
        .route("/migrations/status", post(migration_statuses))
        .route(
            "/manga/{id}/download_rules",
            get(get_download_rules).post(add_download_rule),
        )
        .route(
            "/download_rules/{id}",
            delete(delete_download_rule).patch(update_download_rule),
        )
        .route(
            "/manga/{id}/download_rules/order",
            put(reorder_download_rules),
        )
        .route(
            "/manga/{id}/download_rules/preview",
            post(preview_download_rules),
        )
        .route("/manga/{id}/enrich-metadata", post(enrich_metadata_handler))
}

#[utoipa::path(
    get, path = "/rest/manga/{id}",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Manga row with source and tracking metadata"),
        (status = 401, description = "Not authenticated"),
        (status = 404, description = "Manga not found"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_manga(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(svc.get_manga_by_id(id).await?))
}

#[utoipa::path(
    delete, path = "/rest/manga/{id}",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Manga moved to trash; returns undo_token"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Manga not found"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn delete_manga(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryDelete>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    let undo_token = svc.trash_manga(id, user.id).await?;
    Ok(Json(json!({ "undo_token": undo_token })))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/untrash",
    params(("id" = i64, Path, description = "Manga id")),
    responses(
        (status = 200, description = "Manga restored from the trash"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such manga in the trash"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn untrash_manga_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryDelete>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    svc.untrash_manga(id, user.id).await?;
    Ok(Json(json!({})))
}

#[derive(serde::Deserialize)]
pub(super) struct UndoTokenBody {
    token: uuid::Uuid,
}

#[utoipa::path(
    post, path = "/rest/manga/untrash",
    request_body(content = inline(serde_json::Value), description = "The undo token returned when the manga was trashed"),
    responses(
        (status = 200, description = "Manga restored from the trash by undo token"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Unknown or expired undo token"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn untrash_manga_by_token_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryDelete>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Json(body): Json<UndoTokenBody>,
) -> Result<impl IntoResponse, AppError> {
    svc.untrash_by_token(body.token, user.id).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    get, path = "/rest/trash",
    responses(
        (status = 200, description = "Manga currently in the trash, with their retention deadlines"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn list_trash_handler(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn MangaDomain>>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(svc.list_trash().await?))
}

#[utoipa::path(
    delete, path = "/rest/trash/{id}",
    params(("id" = i64, Path, description = "Manga id")),
    responses(
        (status = 200, description = "Manga permanently deleted"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such manga in the trash"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn purge_trash_one_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryDelete>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    svc.delete_manga(id, user.id).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    delete, path = "/rest/trash",
    responses(
        (status = 200, description = "The trash was emptied; responds with the number purged"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn purge_trash_all_handler(
    _: AuthGuard<crate::permissions::guards::LibraryDelete>,
    State(svc): State<Arc<dyn MangaDomain>>,
) -> Result<impl IntoResponse, AppError> {
    let purged = svc.purge_all_trash().await?;
    Ok(Json(json!({ "purged": purged })))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/cover",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body(content = inline(serde_json::Value), description = "Multipart form with image file", content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Cover uploaded"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn upload_manga_cover_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let field = multipart
        .next_field()
        .await?
        .ok_or_else(|| AppError::Other("No file field provided".into()))?;
    let content_type = field.content_type().unwrap_or("image/jpeg").to_string();
    const MAX_COVER_BYTES: usize = 10 * 1024 * 1024;
    let bytes = field.bytes().await?;
    if bytes.len() > MAX_COVER_BYTES {
        return Err(AppError::Other("Cover image exceeds 10 MB limit".into()));
    }
    svc.upload_manga_cover(manga_id, bytes.to_vec(), &content_type, user.id)
        .await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    delete, path = "/rest/manga/{id}/cover",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 204, description = "Cover override cleared"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn clear_manga_cover_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    svc.clear_manga_cover_override(manga_id, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/details",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Full manga detail: display info, source info, local overrides, chapter stats"),
        (status = 401, description = "Not authenticated"),
        (status = 404, description = "Manga not found"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_local_manga_details(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
    Path(id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    use crate::types::{MangaInfo, MangaStatus};
    let d = state.get_local_manga_details(id).await?;
    let cover_url = if d.manga.local_cover_path.is_some() {
        Some(local_cover_url(id, "lg", d.manga.cover_hash.as_deref()))
    } else {
        d.manga
            .cover_url
            .map(|url| sign_image_url(&url, &d.source.base_url, d.source.id, &state, None))
    };
    let display_name = d
        .manga
        .local_name
        .as_deref()
        .unwrap_or(&d.manga.name)
        .to_owned();
    let display_description = d
        .manga
        .local_description
        .as_ref()
        .or(d.manga.description.as_ref())
        .cloned();
    let display_description_html = display_description
        .as_ref()
        .map(|s| crate::utils::render_description(s));
    let display_status = MangaStatus::from(
        d.manga
            .local_status
            .unwrap_or_else(|| i64::from(d.manga.status)),
    );
    let info = MangaInfo {
        id: d.manga.source_manga_id,
        title: display_name,
        cover_url,
        description: display_description,
        description_html: display_description_html,
        status: display_status,
        authors: d.authors,
        artists: d.artists,
        tags: d.tags,
    };
    Ok(Json(json!({
        "info":                        info,
        "source":                      d.source,
        "auto_download":               d.manga.auto_download,
        "auto_scan":                   d.auto_scan,
        "scanlator_mode":              d.manga.scanlator_mode,
        "download_all_preferred_only": d.manga.download_all_preferred_only,
        "upgrade_auto_replace":        d.manga.upgrade_auto_replace,
        "notes":                       d.manga.notes,
        "cover_overridden":            d.manga.cover_overridden,
        "suppressed_chapter_count":    d.manga.suppressed_chapter_count,
        "import_link_status":          d.import_link_status,
        "local_name":                  d.manga.local_name,
        "local_description":           d.manga.local_description,
        "local_status":                d.manga.local_status,
        "local_authors":               d.local_authors,
        "local_artists":               d.local_artists,
        "local_tags":                  d.local_tags,
        "has_local_people":            d.has_local_people,
        "has_local_tags":              d.has_local_tags,
        "source_name":                 d.manga.name,
        "source_description":          d.manga.description,
        "source_status":               i64::from(d.manga.status),
        "source_authors":              d.source_authors,
        "source_artists":              d.source_artists,
        "source_tags":                 d.source_tags,
        "chapter_count":               d.chapter_count,
        // RFC 3339 — time's default serde emits an array `new Date()` cannot parse.
        "added_at":                    d.manga.created_at
            .format(&time::format_description::well_known::Rfc3339).ok(),
    })))
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/chapters",
    params(
        ("id" = i64, Path, description = "Manga ID"),
        ("page" = Option<i32>, Query, description = "Page number (default 1)"),
        ("page_size" = Option<i32>, Query, description = "Items per page (default 50, max 200)"),
        ("sort_order" = Option<String>, Query, description = "asc or desc"),
        ("filter_downloaded" = Option<bool>, Query, description = "true = downloaded only, false = undownloaded only"),
        ("filter_unread" = Option<bool>, Query, description = "true = unread only"),
        ("filter_scanlator" = Option<String>, Query, description = "Scanlator name"),
        ("filter_orphaned" = Option<bool>, Query, description = "true = only chapters a migration orphaned; absent hides them"),
    ),
    responses(
        (status = 200, description = "Paginated chapter list"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_local_chapters(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    ValidatedQuery(q): ValidatedQuery<LocalChaptersQuery>,
) -> Result<impl IntoResponse, AppError> {
    let (chapters, has_next_page, total_pages, total) = svc
        .get_local_chapters(
            manga_id,
            q.page,
            q.page_size,
            q.sort_order,
            user.id,
            q.filter_downloaded,
            q.filter_unread,
            q.filter_scanlator,
            q.filter_orphaned,
        )
        .await?;
    Ok(Json(crate::types::ChapterList {
        chapters,
        has_next_page,
        total_pages,
        total: Some(total),
    }))
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/chapter_ids",
    params(
        ("id" = i64, Path, description = "Manga ID"),
        ("sort_order" = Option<String>, Query, description = "asc or desc"),
        ("filter_downloaded" = Option<bool>, Query, description = "Downloaded only"),
        ("filter_unread" = Option<bool>, Query, description = "Unread only"),
        ("filter_scanlator" = Option<String>, Query, description = "Scanlator name"),
        ("filter_orphaned" = Option<bool>, Query, description = "true = only chapters a migration orphaned; absent hides them"),
        ("preferred_only" = Option<bool>, Query, description = "Preferred scanlator only"),
    ),
    responses(
        (status = 200, description = "All matching chapter IDs"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_chapter_ids(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    ValidatedQuery(q): ValidatedQuery<crate::models::ChapterIdsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let ids = svc
        .get_chapter_ids(
            manga_id,
            user.id,
            q.sort_order,
            q.filter_downloaded,
            q.filter_unread,
            q.filter_scanlator,
            q.preferred_only,
            q.filter_orphaned,
        )
        .await?;
    Ok(Json(json!({ "ids": ids })))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/download_all",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Aggregate download job queued; returns job_id"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn download_all(
    _: AuthGuard<crate::permissions::guards::ChapterDownload>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    let job_id = svc.download_all_chapters(manga_id).await?;
    Ok(Json(json!({ "job_id": job_id })))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/cancel_all",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "All pending downloads for this manga cancelled"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn cancel_all_downloads(
    _: AuthGuard<crate::permissions::guards::ChapterDownload>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    svc.cancel_all_downloads(manga_id).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/relink",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Linking job queued; returns job_id"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn relink_manga(
    _: AuthGuard<crate::permissions::guards::LibraryRefresh>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    let job_id = svc.queue_import_relink(id).await?;
    Ok(Json(json!({ "job_id": job_id })))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/refresh",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Manga metadata refreshed from source"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn refresh_manga(
    _: AuthGuard<crate::permissions::guards::LibraryRefresh>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(id): Path<MangaId>,
    body: Option<Json<crate::models::RefreshMangaRequest>>,
) -> Result<impl IntoResponse, AppError> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let opts = map_refresh_request(req)?;
    let job_id = svc.queue_manga_refresh(id, opts).await?;
    Ok(Json(json!({ "job_id": job_id })))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/scan",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Scan job queued; returns job_id"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn scan_manga(
    _: AuthGuard<crate::permissions::guards::LibraryRefresh>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    let job_id = svc.queue_manga_scan(id, "manual".to_string()).await?;
    Ok(Json(json!({ "job_id": job_id })))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/toggle_auto_download",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = ToggleAutoDownloadRequest,
    responses(
        (status = 200, description = "Auto-download setting updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn toggle_auto_download(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<ToggleAutoDownloadRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.toggle_auto_download(manga_id, body.enabled).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/toggle_auto_scan",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = ToggleAutoDownloadRequest,
    responses(
        (status = 200, description = "Auto-scan setting updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn toggle_auto_scan_manga(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<ToggleAutoDownloadRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.toggle_auto_scan_manga(manga_id, body.enabled).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/toggle_download_all_preferred",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = ToggleAutoDownloadRequest,
    responses(
        (status = 200, description = "Download-all-preferred setting updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn toggle_download_all_preferred(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<ToggleAutoDownloadRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.toggle_download_all_preferred(manga_id, body.enabled)
        .await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    patch, path = "/rest/manga/{id}/notes",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body(content = inline(serde_json::Value), description = r#"{"notes":"text or null"}"#),
    responses(
        (status = 200, description = "Notes updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn update_manga_notes(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, AppError> {
    let notes = body
        .get("notes")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    svc.update_manga_notes(manga_id, notes).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    patch, path = "/rest/manga/{id}/local_metadata",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = crate::models::UpdateLocalMetadataRequest,
    responses(
        (status = 200, description = "Local metadata overrides updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn update_local_metadata_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<crate::models::UpdateLocalMetadataRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.update_local_metadata(
        manga_id,
        kani_app::models::LocalMetadataUpdate {
            local_name: body.local_name,
            local_description: body.local_description,
            local_status: body.local_status,
            authors: body.authors,
            artists: body.artists,
            tags: body.tags,
        },
        user.id,
    )
    .await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    patch, path = "/rest/manga/{id}/seen",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 204, description = "Manga marked as seen (clears new-chapter badge)"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn mark_manga_seen(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    svc.mark_manga_seen(user.id, manga_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/preview_migration",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = PreviewMigrationRequest,
    responses(
        (status = 200, description = "Migration preview: chapters that would be matched/orphaned"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn preview_migration(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<PreviewMigrationRequest>,
) -> Result<impl IntoResponse, AppError> {
    let preview = svc
        .preview_migration(manga_id, body.target_source_id, body.target_source_manga_id)
        .await?;
    Ok(Json(preview))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/migrate",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = MigrateMangaRequest,
    responses(
        (status = 202, description = "Migration queued; poll GET /rest/jobs/{job_id} for the result"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 409, description = "A migration for this manga is already in progress"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn migrate_manga_handler(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<MigrateMangaRequest>,
) -> Result<impl IntoResponse, AppError> {
    let job_id = svc
        .submit_migration(
            manga_id,
            body.target_source_id,
            body.target_source_manga_id,
            body.keep_orphaned_downloads,
        )
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "job_id": job_id }))))
}

#[utoipa::path(
    post, path = "/rest/migrations/match",
    request_body = MatchMigrationTargetsRequest,
    responses(
        (status = 200, description = "A proposed target series for each title, with ranked candidates"),
        (status = 400, description = "Too many titles in one request"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn match_migration_targets(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Json(body): Json<MatchMigrationTargetsRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.items.len() > MAX_MIGRATION_MATCH_BATCH {
        return Err(AppError::ValidationError(format!(
            "At most {MAX_MIGRATION_MATCH_BATCH} titles can be matched per request"
        )));
    }
    let queries = body
        .items
        .into_iter()
        .map(|i| kani_app::service::migration::MigrationMatchQuery {
            manga_id: i.manga_id,
            query: i.query,
        })
        .collect();
    let matches = svc
        .match_migration_targets(body.target_source_id, queries)
        .await?;
    Ok(Json(matches))
}

#[utoipa::path(
    post, path = "/rest/migrations/bulk",
    request_body = BulkMigrationRequest,
    responses(
        (status = 202, description = "One outcome per title: a job id to follow, or why it was refused"),
        (status = 400, description = "Too many titles in one request"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn submit_bulk_migration(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Json(body): Json<BulkMigrationRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.items.len() > MAX_BULK_MIGRATION {
        return Err(AppError::ValidationError(format!(
            "At most {MAX_BULK_MIGRATION} titles can be migrated per request"
        )));
    }
    let items = body
        .items
        .into_iter()
        .map(|i| kani_app::service::migration::BulkMigrationItem {
            manga_id: i.manga_id,
            target_source_manga_id: i.target_source_manga_id,
        })
        .collect();
    let outcomes = svc
        .submit_bulk_migration(body.target_source_id, items, body.keep_orphaned_downloads)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(outcomes)))
}

#[utoipa::path(
    post, path = "/rest/migrations/status",
    request_body = MigrationStatusRequest,
    responses(
        (status = 200, description = "Status of each requested migration job; other job types are omitted"),
        (status = 400, description = "Too many job ids in one request"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn migration_statuses(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Json(body): Json<MigrationStatusRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.job_ids.len() > MAX_BULK_MIGRATION {
        return Err(AppError::ValidationError(format!(
            "At most {MAX_BULK_MIGRATION} jobs can be read per request"
        )));
    }
    let statuses = svc.migration_statuses(body.job_ids).await?;
    Ok(Json(statuses))
}

const MAX_MIGRATION_MATCH_BATCH: usize = 20;
const MAX_BULK_MIGRATION: usize = 500;

#[utoipa::path(
    get, path = "/rest/manga/{id}/download_rules",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Download rules for this manga"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_download_rules(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(svc.get_download_rules(manga_id).await?))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/download_rules",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = AddDownloadRuleRequest,
    responses(
        (status = 201, description = "Download rule created"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn add_download_rule(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<AddDownloadRuleRequest>,
) -> Result<impl IntoResponse, AppError> {
    let id = svc.add_download_rule(manga_id, body.kind.clone()).await?;
    Ok((
        StatusCode::CREATED,
        Json(kani_shared::types::DownloadRule {
            id,
            manga_id: manga_id.0,
            kind: body.kind,
        }),
    ))
}

#[utoipa::path(
    delete, path = "/rest/download_rules/{id}",
    params(("id" = i64, Path, description = "Download rule ID")),
    responses(
        (status = 200, description = "Download rule deleted"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn delete_download_rule(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(rule_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    svc.delete_download_rule(rule_id).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    patch, path = "/rest/download_rules/{id}",
    params(("id" = i64, Path, description = "Download rule ID")),
    request_body = UpdateDownloadRuleRequest,
    responses(
        (status = 200, description = "Download rule updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn update_download_rule(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(rule_id): Path<i64>,
    Json(body): Json<UpdateDownloadRuleRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.update_download_rule(rule_id, body.kind).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    put, path = "/rest/manga/{id}/download_rules/order",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = ReorderDownloadRulesRequest,
    responses(
        (status = 200, description = "Download rules reordered"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn reorder_download_rules(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<ReorderDownloadRulesRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.reorder_download_rules(manga_id, body.ordered_ids)
        .await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/download_rules/preview",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = PreviewDownloadRulesRequest,
    responses(
        (status = 200, description = "Count of chapters matching the given rules"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn preview_download_rules(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn MangaDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<PreviewDownloadRulesRequest>,
) -> Result<impl IntoResponse, AppError> {
    let (matching, total) = svc.preview_download_rules(manga_id, body.kinds).await?;
    Ok(Json(json!({ "matching": matching, "total": total })))
}

#[derive(serde::Deserialize)]
pub(super) struct EnrichMetadataBody {
    pub(crate) provider: String,
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/enrich-metadata",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body(content = inline(serde_json::Value), description = r#"{"provider":"provider-name"}"#),
    responses(
        (status = 200, description = "Metadata enriched from the named provider"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn enrich_metadata_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<EnrichMetadataBody>,
) -> Result<impl IntoResponse, AppError> {
    let result = state
        .enrich_manga_metadata(manga_id, &body.provider, user.id)
        .await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use axum::extract::{Path, State};
    use kani_app::ids::{MangaId, UserId};
    use kani_app::service::traits::MangaDomain;
    use kani_shared::types::{
        Chapter, ChapterSortOrder, DownloadRule, DownloadRuleKind, MigrationPreview,
    };
    use std::sync::Arc;

    struct StubManga;

    #[async_trait::async_trait]
    impl MangaDomain for StubManga {
        async fn get_download_rules(
            &self,
            _manga_id: MangaId,
        ) -> kani_app::error::Result<Vec<DownloadRule>> {
            Ok(vec![])
        }
        async fn get_manga_by_id(
            &self,
            _id: MangaId,
        ) -> kani_app::error::Result<kani_app::models::Manga> {
            unimplemented!()
        }
        async fn delete_manga(
            &self,
            _id: MangaId,
            _user_id: UserId,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn upload_manga_cover(
            &self,
            _manga_id: MangaId,
            _bytes: Vec<u8>,
            _content_type: &str,
            _user_id: UserId,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn clear_manga_cover_override(
            &self,
            _manga_id: MangaId,
            _user_id: UserId,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        #[allow(clippy::too_many_arguments)]
        async fn get_local_chapters(
            &self,
            _manga_id: MangaId,
            _page: i32,
            _page_size: i32,
            _sort_order: ChapterSortOrder,
            _user_id: UserId,
            _filter_downloaded: Option<bool>,
            _filter_unread: Option<bool>,
            _filter_scanlator: Option<String>,
            _filter_orphaned: Option<bool>,
        ) -> kani_app::error::Result<(Vec<Chapter>, bool, Option<u32>, u32)> {
            unimplemented!()
        }
        #[allow(clippy::too_many_arguments)]
        async fn get_chapter_ids(
            &self,
            _manga_id: MangaId,
            _user_id: UserId,
            _sort_order: ChapterSortOrder,
            _filter_downloaded: Option<bool>,
            _filter_unread: Option<bool>,
            _filter_scanlator: Option<String>,
            _preferred_only: bool,
            _filter_orphaned: Option<bool>,
        ) -> kani_app::error::Result<Vec<kani_app::ids::ChapterId>> {
            unimplemented!()
        }
        async fn download_all_chapters(
            &self,
            _manga_id: MangaId,
        ) -> kani_app::error::Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn queue_manga_scan(
            &self,
            _manga_id: MangaId,
            _trigger: String,
        ) -> kani_app::error::Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn cancel_all_downloads(&self, _manga_id: MangaId) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn refresh_manga_with_options(
            &self,
            _manga_id: MangaId,
            _opts: kani_app::models::RefreshOptions,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn scan_for_new_chapters(
            &self,
            _manga_id: MangaId,
        ) -> kani_app::error::Result<Vec<i64>> {
            unimplemented!()
        }
        async fn toggle_auto_download(
            &self,
            _manga_id: MangaId,
            _enabled: bool,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn toggle_auto_scan_manga(
            &self,
            _manga_id: MangaId,
            _enabled: bool,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn toggle_download_all_preferred(
            &self,
            _manga_id: MangaId,
            _enabled: bool,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn update_manga_notes(
            &self,
            _manga_id: MangaId,
            _notes: Option<String>,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn update_local_metadata(
            &self,
            _manga_id: MangaId,
            _update: kani_app::models::LocalMetadataUpdate,
            _user_id: UserId,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn mark_manga_seen(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn preview_migration(
            &self,
            _manga_id: MangaId,
            _target_source_id: i64,
            _target_source_manga_id: String,
        ) -> kani_app::error::Result<MigrationPreview> {
            unimplemented!()
        }
        async fn submit_migration(
            &self,
            _manga_id: MangaId,
            _target_source_id: i64,
            _target_source_manga_id: String,
            _keep_orphaned_downloads: bool,
        ) -> kani_app::error::Result<kani_app::jobs::JobId> {
            unimplemented!()
        }
        async fn match_migration_targets(
            &self,
            _target_source_id: i64,
            _queries: Vec<kani_app::service::migration::MigrationMatchQuery>,
        ) -> kani_app::error::Result<Vec<kani_app::service::migration::MigrationMatch>> {
            unimplemented!()
        }
        async fn submit_bulk_migration(
            &self,
            _target_source_id: i64,
            _items: Vec<kani_app::service::migration::BulkMigrationItem>,
            _keep_orphaned_downloads: bool,
        ) -> kani_app::error::Result<Vec<kani_app::service::migration::BulkMigrationSubmission>>
        {
            unimplemented!()
        }
        async fn migration_statuses(
            &self,
            _job_ids: Vec<kani_app::jobs::JobId>,
        ) -> kani_app::error::Result<Vec<kani_app::service::traits::JobStatus>> {
            unimplemented!()
        }
        async fn add_download_rule(
            &self,
            _manga_id: MangaId,
            _kind: DownloadRuleKind,
        ) -> kani_app::error::Result<i64> {
            unimplemented!()
        }
        async fn delete_download_rule(&self, _rule_id: i64) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn update_download_rule(
            &self,
            _rule_id: i64,
            _kind: DownloadRuleKind,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn reorder_download_rules(
            &self,
            _manga_id: MangaId,
            _ordered_ids: Vec<i64>,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn preview_download_rules(
            &self,
            _manga_id: MangaId,
            _kinds: Vec<DownloadRuleKind>,
        ) -> kani_app::error::Result<(usize, usize)> {
            unimplemented!()
        }
        async fn trash_manga(
            &self,
            _id: MangaId,
            _user_id: UserId,
        ) -> kani_app::error::Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn untrash_manga(
            &self,
            _id: MangaId,
            _user_id: UserId,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn untrash_by_token(
            &self,
            _token: uuid::Uuid,
            _user_id: UserId,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn queue_import_relink(
            &self,
            _manga_id: MangaId,
        ) -> kani_app::error::Result<uuid::Uuid> {
            unimplemented!()
        }

        async fn queue_manga_refresh(
            &self,
            _manga_id: MangaId,
            _opts: kani_app::models::RefreshOptions,
        ) -> kani_app::error::Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn list_trash(&self) -> kani_app::error::Result<Vec<kani_app::models::Manga>> {
            unimplemented!()
        }
        async fn purge_all_trash(&self) -> kani_app::error::Result<u64> {
            unimplemented!()
        }
    }

    fn stub_user() -> crate::auth::User {
        crate::auth::User {
            id: UserId(1),
            username: "test".into(),
            email: "test@example.com".into(),
            is_active: true,
            created_at: None,
            roles: vec![],
            password_hash: String::new(),
            change_id: vec![],
        }
    }

    #[tokio::test]
    async fn download_rules_returns_empty_without_appservice() {
        let svc: Arc<dyn MangaDomain> = Arc::new(StubManga);
        let response = get_download_rules(
            AuthGuard(stub_user(), PhantomData),
            State(svc),
            Path(MangaId(1)),
        )
        .await
        .unwrap();
        let body = axum::response::IntoResponse::into_response(response);
        assert_eq!(body.status(), axum::http::StatusCode::OK);
    }
}

/// The manga this user has muted, so the client can honour the setting for
/// every series rather than only those it happens to have loaded.
#[utoipa::path(
    get, path = "/rest/me/notify-prefs",
    responses(
        (status = 200, description = "The manga the caller has muted, so the client can suppress their notifications"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_notify_prefs(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let muted = state.service.muted_manga_ids(user.id).await?;
    Ok(Json(json!({ "muted": muted })))
}

#[utoipa::path(
    get, path = "/rest/me/upgrades",
    responses(
        (status = 200, description = "Every available chapter upgrade across the library"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_all_upgrades(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(state.service.all_upgrades().await?))
}

#[derive(serde::Deserialize)]
pub(super) struct AutoReplaceBody {
    pub enabled: bool,
}

#[utoipa::path(
    put, path = "/rest/manga/{id}/upgrade-auto-replace",
    params(("id" = i64, Path, description = "Manga id")),
    request_body(content = inline(serde_json::Value), description = "Whether better versions replace the downloaded chapter automatically"),
    responses(
        (status = 200, description = "Auto-replace setting saved for this manga"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn set_upgrade_auto_replace(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<AutoReplaceBody>,
) -> Result<impl IntoResponse, AppError> {
    state
        .service
        .set_upgrade_auto_replace(manga_id, body.enabled)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Replacing a held chapter moves the old file aside and re-queues the
/// download, so this returns the download job id rather than a finished result.
#[utoipa::path(
    post, path = "/rest/chapters/{id}/upgrade",
    params(("id" = i64, Path, description = "Chapter id")),
    responses(
        (status = 200, description = "Upgrade accepted; responds with the id of the job that performs it"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No upgrade is available for this chapter"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn apply_chapter_upgrade(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path(chapter_id): Path<ChapterId>,
) -> Result<impl IntoResponse, AppError> {
    let job_id = state
        .service
        .apply_upgrade(chapter_id, Some(user.id))
        .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "job_id": job_id })),
    ))
}

#[utoipa::path(
    post, path = "/rest/chapters/{id}/upgrade/dismiss",
    params(("id" = i64, Path, description = "Chapter id")),
    responses(
        (status = 200, description = "Upgrade dismissed; it will not be offered again"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn dismiss_chapter_upgrade(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path(chapter_id): Path<ChapterId>,
) -> Result<impl IntoResponse, AppError> {
    state.service.dismiss_upgrade(chapter_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/dismiss-suppressed",
    params(("id" = i64, Path, description = "Manga id")),
    responses(
        (status = 200, description = "The suppressed-chapter notice was dismissed for this manga"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn dismiss_suppressed_chapters(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    state.service.dismiss_suppressed_chapters(manga_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
