//! Library listing, backup/restore, import & duplicate routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/library",
            get(get_library_filtered)
                .route_layer(axum::middleware::from_fn(crate::etag::etag_middleware)),
        )
        .route("/library/scan-all", post(scan_all_library))
        .route("/manga/scan", post(scan_manga_multiple))
        .route("/library/continue_reading", get(get_continue_reading_shelf))
        .route(
            "/library/{page}/{order}",
            get(get_library).route_layer(axum::middleware::from_fn(crate::etag::etag_middleware)),
        )
        .route("/library/backup", get(library_backup))
        .route(
            "/library/backup/preview",
            post(library_backup_preview).route_layer(DefaultBodyLimit::max(MAX_BACKUP_BYTES)),
        )
        .route(
            "/library/restore",
            post(library_restore).route_layer(DefaultBodyLimit::max(MAX_BACKUP_BYTES)),
        )
        .route(
            "/library/import/tachiyomi/preview",
            post(library_tachiyomi_preview).route_layer(DefaultBodyLimit::max(MAX_TACHI_BYTES)),
        )
        .route(
            "/library/import/tachiyomi",
            post(library_import_tachiyomi).route_layer(DefaultBodyLimit::max(MAX_TACHI_BYTES)),
        )
        .route("/library/pending-imports", get(library_pending_imports))
        .route(
            "/library/pending-imports/{id}",
            delete(library_delete_pending_import),
        )
        .route(
            "/library/pending-imports/{id}/resolve",
            post(library_resolve_pending_import),
        )
        .route("/library/orphaned", get(library_orphaned))
        .route("/library/duplicates", get(library_duplicates))
        .route("/library/duplicates/merge", post(library_merge_duplicate))
        .route("/library/duplicates/scan", post(library_duplicates_scan))
        .route(
            "/library/duplicates/{a}/{b}/dismiss",
            post(library_dismiss_duplicate),
        )
        .route("/recent_updates", get(get_recent_updates))
        .route("/global_search", get(global_search_handler))
}

#[utoipa::path(
    get,
    path = "/rest/library",
    params(
        ("page" = Option<u32>, Query, description = "Page number (default 1)"),
        ("page_size" = Option<u32>, Query, description = "Items per page (default 20)"),
        ("search" = Option<String>, Query, description = "Title filter"),
        ("status_filter" = Option<String>, Query, description = "Manga status filter"),
        ("sort_by" = Option<String>, Query, description = "Sort order"),
    ),
    responses(
        (status = 200, description = "Paginated library listing"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn get_library_filtered(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
    ValidatedQuery(q): ValidatedQuery<LibraryQuery>,
) -> Result<impl IntoResponse, AppError> {
    let manga_id_filter = if let Some(cid) = q.collection_id {
        let col = state.service.get_collection(cid).await?;
        let rule: kani_app::service::smart_collections::SmartCollectionRule =
            serde_json::from_str(&col.rule_json)
                .map_err(|e| AppError::InternalServerError(format!("Invalid rule JSON: {e}")))?;
        let ids = state.service.evaluate_collection(&rule, user.id).await?;
        Some(ids.into_iter().map(|id| id.0).collect::<Vec<_>>())
    } else {
        None
    };
    let filter = kani_app::service::library::LibraryFilter {
        page: q.page,
        page_size: q.page_size,
        search: q.search,
        status_filter: q.status_filter,
        tag_filter: q.tag_filter,
        author_filter: q.author_filter,
        artist_filter: q.artist_filter,
        category_filter: q.category_filter,
        reading_status_filter: q.reading_status_filter,
        hide_no_unread: q.hide_no_unread,
        hide_completed_status: q.hide_completed_status,
        source_id: q.source_id,
        sort_by: q.sort_by,
        include_trashed: false,
        manga_id_filter,
    };
    let (records, has_next_page, total_pages) =
        state.get_library_filtered(user.id, &filter).await?;

    let items = records
        .into_iter()
        .map(|r| {
            let cover_url = if r.local_cover_path.is_some() {
                Some(local_cover_url(r.id, "sm", r.cover_hash.as_deref()))
            } else {
                r.cover_url
                    .map(|url| sign_image_url(&url, &r.base_url, r.source_id, &state, None))
            };
            let resume =
                r.resume_chapter_id
                    .map(|chapter_id| crate::types::ContinueReadingChapter {
                        chapter_id,
                        chapter_number: r.resume_chapter_number.unwrap_or(0.0),
                        last_page: r.resume_last_page.unwrap_or(0),
                        page_count: r.resume_page_count.unwrap_or(0),
                    });
            crate::types::MangaListItem {
                id: r.id.to_string(),
                title: r.name,
                cover_url,
                new_chapter_count: r.new_chapter_count,
                resume,
                match_field: r.match_field,
                match_text: r.match_text,
                import_link_status: r.import_link_status,
                is_orphaned: r.is_orphaned,
            }
        })
        .collect();

    Ok(Json(crate::types::LibraryPage {
        items,
        has_next_page,
        total_pages,
    }))
}

#[utoipa::path(
    post, path = "/rest/library/scan-all",
    responses(
        (status = 200, description = "All manga queued for chapter scan; returns count"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn scan_all_library(
    _: AuthGuard<crate::permissions::guards::LibraryRefresh>,
    State(svc): State<Arc<dyn LibraryDomain>>,
) -> Result<impl IntoResponse, AppError> {
    let job_id = svc.scan_all_manga().await?;
    Ok(Json(json!({ "job_id": job_id })))
}

/// Unified scan endpoint: scan all library manga or a specific list of IDs.
/// Both paths emit `Started` / `MangaRefreshed` / `Completed` SSE events,
/// so the frontend can use identical progress handling for both cases.
///
/// Body: `{ "ids": "all" }` or `{ "ids": [1, 2, 3] }`
#[utoipa::path(
    post, path = "/rest/manga/scan",
    request_body = ScanMangaRequest,
    responses(
        (status = 202, description = "Scan started for specified manga or entire library"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn scan_manga_multiple(
    _: AuthGuard<crate::permissions::guards::LibraryRefresh>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    Json(body): Json<ScanMangaRequest>,
) -> Result<impl IntoResponse, AppError> {
    match body {
        ScanMangaRequest::All { .. } => {
            let job_id = svc.scan_all_manga().await?;
            Ok(Json(json!({ "job_id": job_id })))
        }
        ScanMangaRequest::Ids { ids } => {
            let job_id = svc.scan_manga_ids(ids).await?;
            Ok(Json(json!({ "job_id": job_id })))
        }
    }
}

#[utoipa::path(
    get, path = "/rest/library/continue_reading",
    params(("limit" = Option<i64>, Query, description = "Max items to return (default 12)")),
    responses(
        (status = 200, description = "In-progress manga with next chapter and current page"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn get_continue_reading_shelf(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
    Query(q): Query<ContinueReadingShelfQuery>,
) -> Result<impl IntoResponse, AppError> {
    let items = state.get_continue_reading_shelf(user.id, q.limit).await?;
    let response: Vec<_> = items
        .into_iter()
        .map(|item| {
            let cover_url = if item.local_cover_path.is_some() {
                Some(local_cover_url(
                    item.manga_id,
                    "sm",
                    item.cover_hash.as_deref(),
                ))
            } else {
                item.cover_url
                    .map(|url| sign_image_url(&url, &item.base_url, item.source_id, &state, None))
            };
            json!({
                "manga_id": item.manga_id,
                "manga_name": item.manga_name,
                "cover_url": cover_url,
                "chapter_id": item.chapter_id,
                "chapter_number": item.chapter_number,
                "last_page": item.last_page,
                "page_count": item.page_count,
            })
        })
        .collect();
    Ok(Json(response))
}

#[utoipa::path(
    get, path = "/rest/library/{page}/{order}",
    params(
        ("page" = i32, Path, description = "Page number"),
        ("order" = i32, Path, description = "Sort order"),
    ),
    responses(
        (status = 200, description = "Library listing (legacy paged endpoint)"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn get_library(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    Path((page, order)): Path<(i32, i32)>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(svc.get_library(page, order).await?))
}

#[utoipa::path(
    get, path = "/rest/library/backup",
    params(("include_chapter_progress" = Option<bool>, Query, description = "Include chapter read progress in backup")),
    responses(
        (status = 200, description = "Backup ZIP or encrypted backup download"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_backup(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LibraryBackupQuery>,
) -> Result<impl IntoResponse, AppError> {
    let passphrase = headers
        .get("x-backup-passphrase")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let include_progress = q.include_chapter_progress.unwrap_or(false);
    let bytes = svc
        .export_backup(user.id, include_progress, passphrase)
        .await?;

    let now = time::OffsetDateTime::now_utc();
    let filename = format!(
        "kani-backup-{}-{:02}-{:02}.zip",
        now.year(),
        now.month() as u8,
        now.day()
    );
    let disposition = format!("attachment; filename=\"{filename}\"");

    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(Body::from(bytes))
        .map_err(|e| AppError::InternalServerError(e.to_string()))
}

#[utoipa::path(
    post, path = "/rest/library/backup/preview",
    request_body(content = inline(serde_json::Value), description = "Multipart form with backup ZIP file", content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Backup preview showing what would be restored"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_backup_preview(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let mut file_bytes: Option<bytes::Bytes> = None;
    let mut passphrase: Option<String> = None;

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("file") => {
                let content_length = field
                    .headers()
                    .get(rquest::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<usize>().ok());
                file_bytes = Some(
                    kani_core::http::collect_bytes_limited(
                        Box::pin(field.map_err(|e| kani_core::error::Error::Other(e.to_string()))),
                        content_length,
                        MAX_BACKUP_BYTES,
                    )
                    .await?,
                );
            }
            Some("passphrase") => {
                passphrase = field.text().await.ok().filter(|s| !s.is_empty());
            }
            _ => {}
        }
    }

    let bytes = file_bytes.ok_or_else(|| AppError::ValidationError("No file uploaded".into()))?;
    let preview = svc.preview_backup(&bytes, passphrase).await?;
    Ok(Json(preview))
}

#[utoipa::path(
    post, path = "/rest/library/restore",
    request_body(content = inline(serde_json::Value), description = "Multipart form with backup ZIP and restore option fields", content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Backup restored; returns result summary"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_restore(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let mut file_bytes: Option<bytes::Bytes> = None;
    let mut opts = kani_app::RestoreOptions::default();
    let mut passphrase: Option<String> = None;

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("file") => {
                let content_length = field
                    .headers()
                    .get(rquest::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<usize>().ok());
                file_bytes = Some(
                    kani_core::http::collect_bytes_limited(
                        Box::pin(field.map_err(|e| kani_core::error::Error::Other(e.to_string()))),
                        content_length,
                        MAX_BACKUP_BYTES,
                    )
                    .await?,
                );
            }
            Some("passphrase") => {
                passphrase = field.text().await.ok().filter(|s| !s.is_empty());
            }
            Some("merge") => {
                opts.merge = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(false);
            }
            Some("import_manga") => {
                opts.import_manga = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(true);
            }
            Some("import_categories") => {
                opts.import_categories = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(true);
            }
            Some("import_download_rules") => {
                opts.import_download_rules = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(true);
            }
            Some("import_tracking") => {
                opts.import_tracking = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(true);
            }
            Some("import_chapter_progress") => {
                opts.import_chapter_progress = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(false);
            }
            Some("import_settings") => {
                opts.import_settings = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(false);
            }
            _ => {}
        }
    }

    let data = file_bytes.ok_or_else(|| AppError::ValidationError("No file uploaded".into()))?;
    let result = svc.restore_backup(user.id, &data, opts, passphrase).await?;
    Ok(Json(result))
}

#[utoipa::path(
    post, path = "/rest/library/import/tachiyomi/preview",
    request_body(content = inline(serde_json::Value), description = "Multipart form with Tachiyomi backup file", content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Tachiyomi backup preview"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_tachiyomi_preview(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let bytes = collect_file_field(&mut multipart, MAX_TACHI_BYTES).await?;
    let preview = svc.preview_tachiyomi_backup(&bytes).await?;
    Ok(Json(preview))
}

#[utoipa::path(
    post, path = "/rest/library/import/tachiyomi",
    request_body(content = inline(serde_json::Value), description = "Multipart form with Tachiyomi backup and import option fields", content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Tachiyomi backup imported; returns result summary"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_import_tachiyomi(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let mut file_bytes: Option<bytes::Bytes> = None;
    let mut opts = kani_app::TachiyomiImportOptions::default();

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("file") => {
                let content_length = field
                    .headers()
                    .get(rquest::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<usize>().ok());
                file_bytes = Some(
                    kani_core::http::collect_bytes_limited(
                        Box::pin(field.map_err(|e| kani_core::error::Error::Other(e.to_string()))),
                        content_length,
                        MAX_TACHI_BYTES,
                    )
                    .await?,
                );
            }
            Some("import_manga") => {
                opts.import_manga = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(true);
            }
            Some("import_categories") => {
                opts.import_categories = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(true);
            }
            Some("import_tracking") => {
                opts.import_tracking = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(true);
            }
            Some("import_chapter_progress") => {
                opts.import_chapter_progress = field
                    .text()
                    .await
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(false);
            }
            _ => {}
        }
    }

    let data = file_bytes.ok_or_else(|| AppError::ValidationError("No file uploaded".into()))?;
    let result = svc.import_tachiyomi_backup(user.id, &data, opts).await?;
    Ok(Json(result))
}

#[utoipa::path(
    get, path = "/rest/library/pending-imports",
    responses(
        (status = 200, description = "Pending import items awaiting resolution"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_pending_imports(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn LibraryDomain>>,
) -> Result<impl IntoResponse, AppError> {
    let items = svc.list_pending_imports(user.id).await?;
    Ok(Json(items))
}

#[utoipa::path(
    delete, path = "/rest/library/pending-imports/{id}",
    params(("id" = i64, Path, description = "Pending import ID")),
    responses(
        (status = 204, description = "Pending import deleted"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_delete_pending_import(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    svc.delete_pending_import(user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/rest/library/pending-imports/{id}/resolve",
    params(("id" = i64, Path, description = "Pending import ID")),
    request_body(content = inline(serde_json::Value), description = r#"{"source_id":1,"source_manga_id":"..."}"#),
    responses(
        (status = 200, description = "Import resolved; returns manga db_id"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_resolve_pending_import(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    Path(id): Path<i64>,
    Json(body): Json<ResolvePendingImportBody>,
) -> Result<impl IntoResponse, AppError> {
    let manga_id = svc
        .resolve_pending_import(user.id, id, body.source_id, &body.source_manga_id)
        .await?;
    Ok(Json(json!({ "manga_id": manga_id })))
}

#[utoipa::path(
    get, path = "/rest/library/orphaned",
    responses(
        (status = 200, description = "Manga whose source extension has been removed"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_orphaned(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn LibraryDomain>>,
) -> Result<impl IntoResponse, AppError> {
    let items = svc.list_orphaned_manga().await?;
    Ok(Json(items))
}

#[utoipa::path(
    get, path = "/rest/library/duplicates",
    responses(
        (status = 200, description = "Detected duplicate manga pairs"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_duplicates(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn LibraryDomain>>,
) -> Result<impl IntoResponse, AppError> {
    let pairs = svc.list_duplicates().await?;
    Ok(Json(pairs))
}

#[utoipa::path(
    post, path = "/rest/library/duplicates/merge",
    request_body(content = inline(serde_json::Value), description = r#"{"keep_id":1,"discard_id":2}"#),
    responses(
        (status = 204, description = "Duplicate merged; discard entry removed"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_merge_duplicate(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    Json(body): Json<MergeDuplicateBody>,
) -> Result<impl IntoResponse, AppError> {
    svc.merge_duplicate(body.keep_id, body.discard_id, user.id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/rest/library/duplicates/scan",
    responses(
        (status = 200, description = "Scan complete; returns count of new duplicate pairs found"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_duplicates_scan(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn LibraryDomain>>,
) -> Result<impl IntoResponse, AppError> {
    let new_pairs = svc.scan_duplicates(user.id).await?;
    Ok(Json(serde_json::json!({ "new_pairs": new_pairs })))
}

#[utoipa::path(
    post, path = "/rest/library/duplicates/{a}/{b}/dismiss",
    params(
        ("a" = i64, Path, description = "First manga ID"),
        ("b" = i64, Path, description = "Second manga ID"),
    ),
    responses(
        (status = 204, description = "Duplicate pair dismissed"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn library_dismiss_duplicate(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn LibraryDomain>>,
    axum::extract::Path(path): axum::extract::Path<DismissDuplicatePath>,
) -> Result<impl IntoResponse, AppError> {
    svc.dismiss_duplicate(path.a, path.b).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/recent_updates",
    params(
        ("page" = Option<u32>, Query, description = "Page number"),
        ("page_size" = Option<u32>, Query, description = "Results per page"),
    ),
    responses(
        (status = 200, description = "Recently updated chapters across all library manga"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn get_recent_updates(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
    ValidatedQuery(q): ValidatedQuery<PageQuery>,
) -> Result<impl IntoResponse, AppError> {
    let (mut items, has_next_page, total_pages) = state.get_recent_updates(q.page).await?;
    for u in &mut items {
        u.cover_url = if u.local_cover_path.is_some() {
            Some(local_cover_url(u.manga_id, "sm", u.cover_hash.as_deref()))
        } else if let Some(ref url) = u.cover_url.clone() {
            Some(sign_image_url(url, &u.base_url, u.source_id, &state, None))
        } else {
            None
        };
    }
    Ok(Json(crate::types::RecentUpdate {
        recent_updates: items,
        has_next_page,
        total_pages,
    }))
}

#[utoipa::path(
    get, path = "/rest/global_search",
    params(
        ("query" = String, Query, description = "Search query"),
        ("scope" = Option<String>, Query, description = "library or sources"),
        ("page" = Option<u32>, Query, description = "Page number"),
        ("page_size" = Option<u32>, Query, description = "Results per page"),
    ),
    responses(
        (status = 200, description = "Search results across library and/or sources"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn global_search_handler(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(state): State<AppState>,
    Query(q): Query<GlobalSearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    let mut list = state
        .global_search(&q.query, q.scope, q.page, q.page_size)
        .await?;

    let source_ids: Vec<i64> = list
        .iter()
        .map(|i| i.source_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let source_ids_json = serde_json::to_string(&source_ids)?;
    let base_urls: std::collections::HashMap<i64, String> = sqlx::query!(
        "SELECT id, base_url FROM sources WHERE id IN (SELECT value FROM json_each(?))",
        source_ids_json
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|r| (r.id, r.base_url))
    .collect();

    for result in &mut list {
        let referer = base_urls
            .get(&result.source_id)
            .map(String::as_str)
            .unwrap_or("");
        for item in &mut result.manga {
            if let Some(ref url) = item.cover_url.clone() {
                item.cover_url = Some(sign_image_url(url, referer, result.source_id, &state, None));
            }
        }
    }
    Ok(Json(list))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use axum::extract::State;
    use kani_app::ids::{MangaId, UserId};
    use kani_app::models::Manga;
    use kani_app::service::dedup::DuplicatePair;
    use kani_app::service::traits::LibraryDomain;
    use kani_app::{
        BackupPreview, RestoreOptions, RestoreResult, TachiyomiImportOptions,
        TachiyomiImportResult, TachiyomiPreview,
    };
    use kani_app::{OrphanedManga, models::PendingImportRow};
    use std::sync::Arc;

    struct StubLibrary;

    #[async_trait::async_trait]
    impl LibraryDomain for StubLibrary {
        async fn list_orphaned_manga(&self) -> kani_app::error::Result<Vec<OrphanedManga>> {
            Ok(vec![OrphanedManga {
                id: 7,
                name: "Gone Source Manga".into(),
                cover_url: None,
                local_cover_path: None,
                source_name: "removed-ext".into(),
            }])
        }
        async fn scan_all_manga(&self) -> kani_app::error::Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn scan_manga_ids(&self, _: Vec<MangaId>) -> kani_app::error::Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn get_library(&self, _: i32, _: i32) -> kani_app::error::Result<Vec<Manga>> {
            unimplemented!()
        }
        async fn export_backup(
            &self,
            _: UserId,
            _: bool,
            _: Option<String>,
        ) -> kani_app::error::Result<Vec<u8>> {
            unimplemented!()
        }
        async fn preview_backup(
            &self,
            _: &[u8],
            _: Option<String>,
        ) -> kani_app::error::Result<BackupPreview> {
            unimplemented!()
        }
        async fn restore_backup(
            &self,
            _: UserId,
            _: &[u8],
            _: RestoreOptions,
            _: Option<String>,
        ) -> kani_app::error::Result<RestoreResult> {
            unimplemented!()
        }
        async fn preview_tachiyomi_backup(
            &self,
            _: &[u8],
        ) -> kani_app::error::Result<TachiyomiPreview> {
            unimplemented!()
        }
        async fn import_tachiyomi_backup(
            &self,
            _: UserId,
            _: &[u8],
            _: TachiyomiImportOptions,
        ) -> kani_app::error::Result<TachiyomiImportResult> {
            unimplemented!()
        }
        async fn list_pending_imports(
            &self,
            _: UserId,
        ) -> kani_app::error::Result<Vec<PendingImportRow>> {
            unimplemented!()
        }
        async fn delete_pending_import(&self, _: UserId, _: i64) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn resolve_pending_import(
            &self,
            _: UserId,
            _: i64,
            _: i64,
            _: &str,
        ) -> kani_app::error::Result<MangaId> {
            unimplemented!()
        }
        async fn list_duplicates(&self) -> kani_app::error::Result<Vec<DuplicatePair>> {
            unimplemented!()
        }
        async fn merge_duplicate(&self, _: i64, _: i64, _: UserId) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn scan_duplicates(&self, _: UserId) -> kani_app::error::Result<u32> {
            unimplemented!()
        }
        async fn dismiss_duplicate(&self, _: MangaId, _: MangaId) -> kani_app::error::Result<()> {
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
    async fn library_orphaned_returns_items_without_appservice() {
        let svc: Arc<dyn LibraryDomain> = Arc::new(StubLibrary);
        let response = library_orphaned(AuthGuard(stub_user(), PhantomData), State(svc))
            .await
            .unwrap();
        let body = axum::response::IntoResponse::into_response(response);
        assert_eq!(body.status(), axum::http::StatusCode::OK);
    }
}
