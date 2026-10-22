//! Extension source management & browsing routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sources", get(list_sources))
        .route("/sources/health", get(get_sources_health))
        .route("/sources/active_ids", get(get_active_source_ids))
        .route("/sources/metadata-providers", get(list_metadata_providers))
        .route(
            "/sources/{id}",
            get(get_source).patch(update_source).delete(delete_source),
        )
        .route("/sources/{id}/metadata", get(get_metadata))
        .route("/sources/{id}/wasm", post(upload_wasm))
        .route("/sources/{id}/wasm/fetch", post(fetch_wasm))
        .route("/sources/wasm", post(install_wasm))
        .route("/sources/wasm/fetch", post(install_wasm_from_url))
        .route("/sources/yaml", post(install_yaml))
        .route("/sources/yaml/fetch", post(fetch_yaml))
        .route("/sources/{id}/reload", post(reload_source_handler))
        .route(
            "/sources/{id}/download-concurrency",
            put(set_download_concurrency),
        )
        .route("/sources/{id}/browser-enabled", put(set_browser_enabled))
        .route(
            "/sources/{id}/local-hosts",
            get(get_local_hosts).put(set_local_hosts),
        )
        .route(
            "/sources/{id}/popular/{page}/{page_size}",
            get(get_popular_manga),
        )
        .route("/sources/{id}/search/{page}/{page_size}", get(search_manga))
        .route("/sources/{id}/details/{manga_id}", get(get_manga_details))
        .route("/sources/{id}/url/{manga_id}", get(get_source_manga_url))
        .route("/sources/{id}/save/{manga_id}", post(save_to_library))
        .route(
            "/sources/{id}/chapters/{manga_id}/{page}/{page_size}",
            get(get_chapter_list),
        )
        .route(
            "/sources/{id}/chapter-sorts/{manga_id}",
            get(get_chapter_sort_list),
        )
        .route(
            "/sources/{id}/pages/{manga_id}/{chapter_id}",
            get(get_pages),
        )
        .route("/sources/{id}/in_library/{manga_id}", get(check_in_library))
        .route("/sources/{id}/toggle_enabled", patch(toggle_source_enabled))
        .route(
            "/sources/{id}/toggle_favourite",
            patch(toggle_source_favourite),
        )
        .route("/sources/{id}/filters", get(get_source_filters))
        .route("/sources/{id}/preference_schema", get(get_pref_schema))
        .route("/sources/{id}/preferences", get(get_source_preferences))
        .route(
            "/sources/{id}/preferences/{key}",
            put(set_source_preference),
        )
        .route(
            "/sources/{id}/preferences/{key}/append",
            post(append_pref_list_item),
        )
        .route(
            "/sources/{id}/preferences/{key}/remove_item",
            post(remove_pref_list_item),
        )
        .route(
            "/sources/{id}/preferences/{key}/toggle_select",
            post(toggle_pref_select_item),
        )
        .route("/sources/capabilities", get(get_all_capabilities))
        .route("/sources/{id}/capabilities", get(get_capabilities))
        .route(
            "/sources/repos",
            get(list_repos_handler).post(add_repo_handler),
        )
        .route(
            "/sources/repos/{id}",
            get(get_repo_handler).delete(remove_repo_handler),
        )
        .route("/sources/repos/{id}/refresh", post(refresh_repo_handler))
        .route(
            "/sources/repos/{id}/extensions",
            get(list_repo_extensions_handler),
        )
        .route("/sources/install", post(install_from_repo_handler))
        .route("/sources/{id}/update", post(update_from_repo_handler))
}

#[utoipa::path(
    get, path = "/rest/sources",
    responses(
        (status = 200, description = "All installed sources"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn list_sources(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(svc): State<Arc<dyn SourceDomain>>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(svc.list_sources().await?))
}

#[utoipa::path(
    get, path = "/rest/sources/health",
    responses(
        (status = 200, description = "Health status for all installed sources"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_sources_health(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(svc): State<Arc<dyn SourceDomain>>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(svc.get_source_health().await?))
}

#[utoipa::path(
    get, path = "/rest/sources/active_ids",
    responses(
        (status = 200, description = "IDs of all enabled (active) sources"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_active_source_ids(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(svc): State<Arc<dyn SourceDomain>>,
) -> Result<impl IntoResponse, AppError> {
    let ids = svc.list_active_source_ids().await?;
    Ok(Json(ids))
}

#[utoipa::path(
    get, path = "/rest/sources/metadata-providers",
    responses(
        (status = 200, description = "All registered metadata enrichment providers"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn list_metadata_providers(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let registry = state.metadata_provider_registry.read().await;
    Ok(Json(registry.list()))
}

#[derive(serde::Serialize)]
struct SourceDetail {
    #[serde(flatten)]
    source: kani_shared::types::Source,
    #[serde(skip_serializing_if = "Option::is_none")]
    backend: Option<&'static str>,
}

#[utoipa::path(
    get, path = "/rest/sources/{id}",
    params(("id" = i64, Path, description = "Source ID")),
    responses(
        (status = 200, description = "Source details"),
        (status = 401, description = "Not authenticated"),
        (status = 404, description = "Source not found"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_source(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let source = state.service.get_source(id).await?;
    let backend = if let Some(b) = state.service.sources.get_backend(id) {
        Some(b.backend_kind())
    } else {
        let storage = state
            .service
            .settings
            .read()
            .await
            .wasm_storage_path
            .clone();
        if storage.join(format!("{}.yaml", source.name)).exists() {
            Some("yaml")
        } else if storage.join(format!("{}.wasm", source.name)).exists() {
            Some("wasm")
        } else {
            None
        }
    };
    Ok(Json(SourceDetail { source, backend }))
}

#[utoipa::path(
    patch, path = "/rest/sources/{id}",
    params(("id" = i64, Path, description = "Source ID")),
    request_body = UpdateSource,
    responses(
        (status = 200, description = "Source updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn update_source(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path(id): Path<i64>,
    ValidatedJson(payload): ValidatedJson<UpdateSource>,
) -> Result<impl IntoResponse, AppError> {
    if payload.name.is_none() && payload.version.is_none() {
        return Ok(Json(json!({})));
    }
    svc.update_source(id, payload.name, payload.version).await?;
    Ok(Json(json!({})))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub(super) struct SetDownloadConcurrencyRequest {
    pub value: Option<i64>,
}

#[utoipa::path(
    put, path = "/rest/sources/{id}/download-concurrency",
    params(("id" = i64, Path, description = "Source ID")),
    request_body = SetDownloadConcurrencyRequest,
    responses(
        (status = 200, description = "Concurrency override updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn set_download_concurrency(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path(id): Path<i64>,
    Json(body): Json<SetDownloadConcurrencyRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.set_source_download_concurrency(id, body.value).await?;
    Ok(Json(json!({})))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub(super) struct SetBrowserEnabledRequest {
    pub enabled: bool,
}

#[utoipa::path(
    put, path = "/rest/sources/{id}/browser-enabled",
    params(("id" = i64, Path, description = "Source ID")),
    request_body = SetBrowserEnabledRequest,
    responses(
        (status = 200, description = "Browser capability gate updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn set_browser_enabled(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path(id): Path<i64>,
    Json(body): Json<SetBrowserEnabledRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.set_source_browser_enabled(id, body.enabled).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    delete, path = "/rest/sources/{id}",
    params(("id" = i64, Path, description = "Source ID")),
    responses(
        (status = 200, description = "Source deleted"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn delete_source(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::SourceDelete>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    svc.delete_source(id, user.id).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/metadata",
    params(("id" = i64, Path, description = "Source ID")),
    responses(
        (status = 200, description = "Raw source metadata JSON"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_metadata(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let result = svc.get_metadata(id).await?;
    Ok(result)
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/local-hosts",
    params(("id" = i64, Path, description = "Source ID")),
    responses(
        (status = 200, description = "Private hosts this source may reach"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Source not found"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_local_hosts(
    _: AuthGuard<crate::permissions::guards::AdminManage>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        json!({ "hosts": state.get_source_local_hosts(id).await? }),
    ))
}

#[utoipa::path(
    put, path = "/rest/sources/{id}/local-hosts",
    params(("id" = i64, Path, description = "Source ID")),
    request_body = SetLocalHostsRequest,
    responses(
        (status = 200, description = "Grant replaced; the source is reloaded with it"),
        (status = 400, description = "An entry is not a host or cannot be granted"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Source not found"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn set_local_hosts(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::AdminManage>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ValidatedJson(payload): ValidatedJson<SetLocalHostsRequest>,
) -> Result<impl IntoResponse, AppError> {
    let hosts = state
        .set_source_local_hosts(id, payload.hosts, user.id)
        .await?;
    Ok(Json(json!({ "hosts": hosts })))
}

#[utoipa::path(
    post, path = "/rest/sources/{id}/wasm",
    params(("id" = i64, Path, description = "Source ID")),
    request_body(content = inline(serde_json::Value), description = "Multipart form with WASM file field", content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "WASM installed successfully"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn upload_wasm(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    if !crate::SOURCE_INSTALL_ALLOWED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Forbidden(
            "Source installation is disabled by the administrator".into(),
        ));
    }
    state.get_source(id).await?;
    let bytes = read_wasm_field(&mut multipart).await?;
    state.update_wasm_source(id, bytes.as_ref()).await?;
    Ok(StatusCode::OK)
}

async fn read_wasm_field(multipart: &mut Multipart) -> Result<bytes::Bytes, AppError> {
    let field = multipart
        .next_field()
        .await?
        .ok_or_else(|| AppError::ValidationError("no file field in upload".into()))?;

    let content_length = field
        .headers()
        .get(rquest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());

    Ok(kani_core::http::collect_bytes_limited(
        Box::pin(field.map_err(|e| kani_core::error::Error::Other(e.to_string()))),
        content_length,
        MAX_WASM_BYTES,
    )
    .await?)
}

#[utoipa::path(
    post, path = "/rest/sources/wasm",
    request_body(content = inline(serde_json::Value), description = "Multipart form with WASM file field", content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "WASM extension installed; returns its source ID"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 400, description = "Invalid WASM extension"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn install_wasm(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    if !crate::SOURCE_INSTALL_ALLOWED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Forbidden(
            "Source installation is disabled by the administrator".into(),
        ));
    }
    let bytes = read_wasm_field(&mut multipart).await?;
    let id = state.install_wasm_source(bytes.as_ref()).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(
    post, path = "/rest/sources/wasm/fetch",
    request_body = FetchWasmRequest,
    responses(
        (status = 200, description = "WASM fetched from URL and installed; returns its source ID"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 400, description = "Invalid WASM extension"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn install_wasm_from_url(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<FetchWasmRequest>,
) -> Result<impl IntoResponse, AppError> {
    if !crate::SOURCE_INSTALL_ALLOWED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Forbidden(
            "Source installation is disabled by the administrator".into(),
        ));
    }
    let response = state.proxy_client.safe_get(&payload.url, None).await?;
    let bytes = response.bytes_limited(MAX_WASM_BYTES).await?;
    let id = state.install_wasm_source(&bytes).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(
    post, path = "/rest/sources/{id}/wasm/fetch",
    params(("id" = i64, Path, description = "Source ID")),
    request_body = FetchWasmRequest,
    responses(
        (status = 200, description = "WASM fetched from URL and installed"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn fetch_wasm(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ValidatedJson(payload): ValidatedJson<FetchWasmRequest>,
) -> Result<impl IntoResponse, AppError> {
    if !crate::SOURCE_INSTALL_ALLOWED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Forbidden(
            "Source installation is disabled by the administrator".into(),
        ));
    }
    state.get_source(id).await?;

    let response = state.proxy_client.safe_get(&payload.url, None).await?;

    let bytes = response.bytes_limited(MAX_WASM_BYTES).await?;

    state.update_wasm_source(id, &bytes).await?;

    Ok(StatusCode::OK)
}

#[utoipa::path(
    post, path = "/rest/sources/yaml",
    request_body = InstallYamlRequest,
    responses(
        (status = 200, description = "Interpreted-YAML extension installed"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 400, description = "Invalid YAML extension"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn install_yaml(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<InstallYamlRequest>,
) -> Result<impl IntoResponse, AppError> {
    if !crate::SOURCE_INSTALL_ALLOWED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Forbidden(
            "Source installation is disabled by the administrator".into(),
        ));
    }
    state
        .install_yaml_source(payload.content.as_bytes())
        .await?;
    Ok(StatusCode::OK)
}

#[utoipa::path(
    post, path = "/rest/sources/yaml/fetch",
    request_body = FetchYamlRequest,
    responses(
        (status = 200, description = "YAML fetched from URL and installed"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 400, description = "Invalid YAML extension"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn fetch_yaml(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<FetchYamlRequest>,
) -> Result<impl IntoResponse, AppError> {
    if !crate::SOURCE_INSTALL_ALLOWED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Forbidden(
            "Source installation is disabled by the administrator".into(),
        ));
    }
    let response = state.proxy_client.safe_get(&payload.url, None).await?;
    let bytes = response.bytes_limited(MAX_WASM_BYTES).await?;
    state.install_yaml_source(&bytes).await?;
    Ok(StatusCode::OK)
}

#[utoipa::path(
    post, path = "/rest/sources/{id}/reload",
    params(("id" = i64, Path, description = "Source ID")),
    responses(
        (status = 200, description = "Source reloaded into the runtime"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn reload_source_handler(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    state.reload_source(id).await?;
    Ok(StatusCode::OK)
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/popular/{page}/{page_size}",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("page" = i32, Path, description = "Page number"),
        ("page_size" = i32, Path, description = "Results per page"),
    ),
    responses(
        (status = 200, description = "Popular manga from this source"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_popular_manga(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(state): State<AppState>,
    Path((id, page, page_size)): Path<(i64, i32, i32)>,
    Query(query): Query<crate::models::PopularMangaQuery>,
) -> Result<impl IntoResponse, AppError> {
    let base_url = state.get_source_base_url(id).await?;
    let json_str = state
        .get_popular_manga(id, page, page_size, query.filters)
        .await?;
    let mut list: crate::types::MangaList = serde_json::from_str(&json_str)?;
    for item in &mut list.manga {
        if let Some(ref url) = item.cover_url.clone() {
            item.cover_url = Some(sign_image_url(url, &base_url, id, &state, None));
        }
    }
    Ok(Json(list))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/search/{page}/{page_size}",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("page" = i32, Path, description = "Page number"),
        ("page_size" = i32, Path, description = "Results per page"),
        ("query" = Option<String>, Query, description = "Search query"),
    ),
    responses(
        (status = 200, description = "Search results from this source"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn search_manga(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(state): State<AppState>,
    Path((id, page, page_size)): Path<(i64, i32, i32)>,
    ValidatedQuery(payload): ValidatedQuery<SearchMangaRequest>,
) -> Result<impl IntoResponse, AppError> {
    let base_url = state.get_source_base_url(id).await?;
    let json_str = state
        .search_manga(
            id,
            &payload.query.unwrap_or_default(),
            page,
            page_size,
            payload.filters,
        )
        .await?;
    let mut list: crate::types::MangaList = serde_json::from_str(&json_str)?;
    for item in &mut list.manga {
        if let Some(ref url) = item.cover_url.clone() {
            item.cover_url = Some(sign_image_url(url, &base_url, id, &state, None));
        }
    }
    Ok(Json(list))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/details/{manga_id}",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("manga_id" = String, Path, description = "Source manga ID"),
    ),
    responses(
        (status = 200, description = "Manga details from the source"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_manga_details(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(state): State<AppState>,
    Path((id, manga_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, AppError> {
    let base_url = state.get_source_base_url(id).await?;
    let json_str = state.get_manga_details(id, &manga_id).await?;
    let mut info: crate::types::MangaInfo = serde_json::from_str(&json_str)?;
    info.cover_url = info
        .cover_url
        .map(|url| sign_image_url(&url, &base_url, id, &state, None));
    info.description_html = info
        .description
        .as_deref()
        .map(crate::utils::render_description);
    Ok(Json(info))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/url/{manga_id}",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("manga_id" = String, Path, description = "Source manga ID"),
    ),
    responses(
        (status = 200, description = "Canonical source URL for this manga"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_source_manga_url(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path((id, manga_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, AppError> {
    let url = svc.get_source_url(id, &manga_id).await?;
    Ok(Json(serde_json::json!({ "url": url })))
}

#[utoipa::path(
    post, path = "/rest/sources/{id}/save/{manga_id}",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("manga_id" = String, Path, description = "Source manga ID"),
        ("force" = Option<bool>, Query, description = "Force add even if already in library"),
    ),
    responses(
        (status = 200, description = "Manga saved to library; returns db_id"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn save_to_library(
    _: AuthGuard<crate::permissions::guards::LibraryAdd>,
    State(state): State<AppState>,
    Path((id, manga_id)): Path<(i64, String)>,
    Query(q): Query<SaveToLibraryQuery>,
) -> Result<impl IntoResponse, AppError> {
    let manga_row_id = state
        .save_to_library(id, &manga_id, q.force.unwrap_or(false))
        .await?;
    Ok(Json(json!({ "db_id": manga_row_id })))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/chapters/{manga_id}/{page}/{page_size}",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("manga_id" = String, Path, description = "Source manga ID"),
        ("page" = i32, Path, description = "Page number"),
        ("page_size" = i32, Path, description = "Results per page"),
        ("sort" = Option<String>, Query, description = "Sort option"),
    ),
    responses(
        (status = 200, description = "Paged chapter list from source"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_chapter_list(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path((id, manga_id, page, page_size)): Path<(i64, String, i32, i32)>,
    Query(q): Query<ChapterListQuery>,
) -> Result<impl IntoResponse, AppError> {
    let json_str = svc
        .get_chapter_list_paged(id, &manga_id, page, page_size, q.sort)
        .await?;
    let list: crate::types::ChapterList = serde_json::from_str(&json_str)?;
    Ok(Json(list))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/chapter-sorts/{manga_id}",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("manga_id" = String, Path, description = "Source manga ID"),
    ),
    responses(
        (status = 200, description = "Available chapter sort options for this source"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_chapter_sort_list(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path((id, _manga_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, AppError> {
    let opts = svc.get_chapter_sort_list(id).await?;
    Ok(Json(opts))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/pages/{manga_id}/{chapter_id}",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("manga_id" = String, Path, description = "Source manga ID"),
        ("chapter_id" = String, Path, description = "Source chapter ID"),
    ),
    responses(
        (status = 200, description = "Page URLs for this chapter"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_pages(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(state): State<AppState>,
    Path((id, manga_id, chapter_id)): Path<(i64, String, String)>,
) -> Result<impl IntoResponse, AppError> {
    let base_url = state.get_source_base_url(id).await?;
    let json_str = state.get_pages(id, &manga_id, &chapter_id).await?;
    let mut contents: crate::types::ChapterContents = serde_json::from_str(&json_str)?;
    for page in &mut contents.pages {
        page.url = sign_image_url(&page.url, &base_url, id, &state, page.transform.as_deref());
    }
    Ok(Json(contents))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/in_library/{manga_id}",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("manga_id" = String, Path, description = "Source manga ID"),
    ),
    responses(
        (status = 200, description = "Returns db_id if in library, null otherwise"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn check_in_library(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
    Path((source_id, manga_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, AppError> {
    let db_id = state.check_in_library(source_id, &manga_id).await?;
    Ok(Json(json!({ "db_id": db_id })))
}

#[utoipa::path(
    patch, path = "/rest/sources/{id}/toggle_enabled",
    params(("id" = i64, Path, description = "Source ID")),
    request_body = ToggleEnabledRequest,
    responses(
        (status = 200, description = "Source enabled state toggled"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn toggle_source_enabled(
    _: AuthGuard<crate::permissions::guards::SourceToggleEnabled>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path(source_id): Path<i64>,
    Json(body): Json<ToggleEnabledRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.toggle_source_enabled(source_id, body.enabled).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    patch, path = "/rest/sources/{id}/toggle_favourite",
    params(("id" = i64, Path, description = "Source ID")),
    request_body = ToggleFavouritedRequest,
    responses(
        (status = 200, description = "Source favourite state toggled"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn toggle_source_favourite(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path(source_id): Path<i64>,
    Json(body): Json<ToggleFavouritedRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.toggle_source_favourite(source_id, body.favourited)
        .await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/filters",
    params(("id" = i64, Path, description = "Source ID")),
    responses(
        (status = 200, description = "Filter spec for this source's search"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_source_filters(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let filter_list = svc.get_filter_list(id).await?;
    Ok(Json(filter_list))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/preference_schema",
    params(("id" = i64, Path, description = "Source ID")),
    responses(
        (status = 200, description = "Preference schema for this source"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_pref_schema(
    _: AuthGuard<crate::permissions::guards::SourceConfigure>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path(source_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let schema = svc.get_source_pref_schema(source_id).await?;
    Ok(Json(schema))
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/preferences",
    params(("id" = i64, Path, description = "Source ID")),
    responses(
        (status = 200, description = "All preferences for this source as key-value pairs"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_source_preferences(
    _: AuthGuard<crate::permissions::guards::SourceConfigure>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path(source_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let rows = svc
        .get_all_preferences(source_id)
        .await?
        .into_iter()
        .map(|(k, v)| json!({ "key": k, "value": v }))
        .collect::<Vec<_>>();
    Ok(Json(rows))
}

#[utoipa::path(
    put, path = "/rest/sources/{id}/preferences/{key}",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("key" = String, Path, description = "Preference key"),
    ),
    request_body = SetPreferenceRequest,
    responses(
        (status = 200, description = "Preference updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn set_source_preference(
    _: AuthGuard<crate::permissions::guards::SourceConfigure>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path((source_id, key)): Path<(i64, String)>,
    Json(body): Json<SetPreferenceRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.set_preference(source_id, &key, &body.value).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    post, path = "/rest/sources/{id}/preferences/{key}/append",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("key" = String, Path, description = "Preference key"),
    ),
    request_body = ListItemRequest,
    responses(
        (status = 200, description = "Item appended to list preference"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn append_pref_list_item(
    _: AuthGuard<crate::permissions::guards::SourceConfigure>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path((source_id, key)): Path<(i64, String)>,
    Json(body): Json<ListItemRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.append_pref_list_item(source_id, &key, body.item)
        .await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    post, path = "/rest/sources/{id}/preferences/{key}/remove_item",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("key" = String, Path, description = "Preference key"),
    ),
    request_body = ListItemRequest,
    responses(
        (status = 200, description = "Item removed from list preference"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn remove_pref_list_item(
    _: AuthGuard<crate::permissions::guards::SourceConfigure>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path((source_id, key)): Path<(i64, String)>,
    Json(body): Json<ListItemRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.remove_pref_list_item(source_id, &key, &body.item)
        .await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    post, path = "/rest/sources/{id}/preferences/{key}/toggle_select",
    params(
        ("id" = i64, Path, description = "Source ID"),
        ("key" = String, Path, description = "Preference key"),
    ),
    request_body = ToggleSelectRequest,
    responses(
        (status = 200, description = "Select item toggled in preference"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn toggle_pref_select_item(
    _: AuthGuard<crate::permissions::guards::SourceConfigure>,
    State(svc): State<Arc<dyn SourceDomain>>,
    Path((source_id, key)): Path<(i64, String)>,
    Json(body): Json<ToggleSelectRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.toggle_pref_select_item(source_id, &key, body.item, body.selected)
        .await?;
    Ok(Json(json!({})))
}

#[derive(serde::Serialize)]
struct SourceCapabilities {
    streaming_chapters: bool,
}

#[derive(serde::Serialize)]
struct SourceCapabilityEntry {
    source_id: i64,
    #[serde(flatten)]
    capabilities: SourceCapabilities,
}

#[utoipa::path(
    get, path = "/rest/sources/{id}/capabilities",
    params(("id" = i64, Path, description = "Source ID")),
    responses(
        (status = 200, description = "Capability flags for this source"),
        (status = 401, description = "Not authenticated"),
        (status = 404, description = "Source not found"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_capabilities(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    // Host-side page polling gives every installed source incremental delivery.
    let exists: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM sources WHERE id = ? AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    exists.ok_or_else(|| AppError::NotFound(format!("Source {id} not found")))?;
    Ok(Json(SourceCapabilities {
        streaming_chapters: true,
    }))
}

/// Capabilities for every installed source in one round trip.
///
/// A client rendering a source list would otherwise issue one request per
/// source to `/sources/{id}/capabilities`, which is the shape this exists to
/// avoid — so it answers from a single query rather than looping internally
/// and reproducing the same N problem on the server.
#[utoipa::path(
    get, path = "/rest/sources/capabilities",
    responses(
        (status = 200, description = "Capability flags for every installed source"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_all_capabilities(
    _: AuthGuard<crate::permissions::guards::SourceBrowse>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let ids: Vec<(i64,)> =
        sqlx::query_as("SELECT id FROM sources WHERE deleted_at IS NULL ORDER BY id")
            .fetch_all(&state.db)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    // This bulk response must match the per-source capability contract.
    let out: Vec<SourceCapabilityEntry> = ids
        .into_iter()
        .map(|(source_id,)| SourceCapabilityEntry {
            source_id,
            capabilities: SourceCapabilities {
                streaming_chapters: true,
            },
        })
        .collect();

    Ok(Json(out))
}

#[utoipa::path(
    get, path = "/rest/sources/repos",
    responses(
        (status = 200, description = "All trusted repositories"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn list_repos_handler(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(state.list_repos().await?))
}

#[utoipa::path(
    post, path = "/rest/sources/repos",
    request_body = AddRepoRequest,
    responses(
        (status = 200, description = "Repository added or refreshed"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 409, description = "Repository key changed since last trust"),
        (status = 428, description = "TOFU confirmation required — re-submit with X-Confirm-Key-Fingerprint header"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn add_repo_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::RepoAdd>,
    State(state): State<AppState>,
    headers: HeaderMap,
    ValidatedJson(payload): ValidatedJson<AddRepoRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if !crate::SOURCE_INSTALL_ALLOWED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Forbidden(
            "Source installation is disabled by the administrator".into(),
        ));
    }
    use kani_app::service::repos::RepoAddResult;
    let confirm_fp = headers
        .get("X-Confirm-Key-Fingerprint")
        .and_then(|v| v.to_str().ok())
        .or(payload.confirm_fingerprint.as_deref());
    match state
        .add_repo(&payload.url, confirm_fp, Some(user.id))
        .await?
    {
        RepoAddResult::Added { id, name } => {
            Ok((StatusCode::OK, Json(json!({ "id": id, "name": name }))))
        }
        RepoAddResult::ConfirmationRequired {
            fingerprint,
            repo_url,
        } => Ok((
            StatusCode::PRECONDITION_REQUIRED,
            Json(json!({
                "error": "TOFU_CONFIRMATION_REQUIRED",
                "fingerprint": fingerprint,
                "repo_url": repo_url,
            })),
        )),
        RepoAddResult::KeyChanged {
            old_fingerprint,
            new_fingerprint,
            repo_url,
        } => Ok((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "REPO_KEY_CHANGED",
                "old_fingerprint": old_fingerprint,
                "new_fingerprint": new_fingerprint,
                "repo_url": repo_url,
            })),
        )),
    }
}

#[utoipa::path(
    get, path = "/rest/sources/repos/{id}",
    params(("id" = i64, Path, description = "Repository ID")),
    responses(
        (status = 200, description = "Repository details"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Repository not found"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn get_repo_handler(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(state.get_repo(id).await?))
}

#[utoipa::path(
    post, path = "/rest/sources/repos/{id}/refresh",
    params(("id" = i64, Path, description = "Repository ID")),
    responses(
        (status = 200, description = "Repository index refreshed"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Repository not found"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn refresh_repo_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::RepoRefresh>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    state.refresh_repo(id, Some(user.id)).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(
    delete, path = "/rest/sources/repos/{id}",
    params(("id" = i64, Path, description = "Repository ID")),
    responses(
        (status = 204, description = "Repository removed"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Repository not found"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn remove_repo_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::RepoRemove>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    state.remove_repo(id, Some(user.id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/sources/repos/{id}/extensions",
    params(("id" = i64, Path, description = "Repository ID")),
    responses(
        (status = 200, description = "Extensions available in the repository"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Repository not found"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn list_repo_extensions_handler(
    _: AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(state.list_repo_extensions(id).await?))
}

#[utoipa::path(
    post, path = "/rest/sources/install",
    request_body = InstallFromRepoRequest,
    responses(
        (status = 201, description = "Source installed from repository; returns new source ID"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions or repo blocked"),
        (status = 404, description = "Repository or extension not found"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn install_from_repo_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    Json(payload): Json<InstallFromRepoRequest>,
) -> Result<impl IntoResponse, AppError> {
    if !crate::SOURCE_INSTALL_ALLOWED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Forbidden(
            "Source installation is disabled by the administrator".into(),
        ));
    }
    let id = state
        .install_source_from_repo(payload.repo_id, &payload.extension_id, Some(user.id))
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

#[utoipa::path(
    post, path = "/rest/sources/{id}/update",
    params(("id" = i64, Path, description = "Source ID to update")),
    request_body = UpdateFromRepoRequest,
    responses(
        (status = 200, description = "Source updated to latest repository version"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Source, repository, or extension not found"),
    ),
    security(("session" = [])),
    tag = "sources"
)]
pub(super) async fn update_from_repo_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::SourceInstall>,
    State(state): State<AppState>,
    Path(source_id): Path<i64>,
    Json(payload): Json<UpdateFromRepoRequest>,
) -> Result<impl IntoResponse, AppError> {
    if !crate::SOURCE_INSTALL_ALLOWED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Forbidden(
            "Source installation is disabled by the administrator".into(),
        ));
    }
    state
        .update_source_from_repo(
            payload.repo_id,
            &payload.extension_id,
            source_id,
            Some(user.id),
        )
        .await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use axum::extract::State;
    use kani_app::ids::UserId;
    use kani_app::models::SourceHealthRow;
    use kani_app::service::traits::SourceDomain;
    use kani_shared::types::{SortOption, Source};
    use std::sync::Arc;

    struct StubSources;

    #[async_trait::async_trait]
    impl SourceDomain for StubSources {
        async fn list_sources(&self) -> kani_app::error::Result<Vec<Source>> {
            Ok(vec![Source {
                id: 1,
                name: "stub-ext".into(),
                version: "1.0".into(),
                base_url: "https://example.com".into(),
                enabled: true,
                favourited: false,
                unrestricted_http: false,
                browser_enabled: true,
                download_concurrency: None,
                circuit_state: None,
                icon: None,
                description: None,
                languages: None,
                schema_version: 1,
            }])
        }
        async fn get_source(&self, _: i64) -> kani_app::error::Result<Source> {
            unimplemented!()
        }
        async fn update_source(
            &self,
            _: i64,
            _: Option<String>,
            _: Option<String>,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn get_source_health(&self) -> kani_app::error::Result<Vec<SourceHealthRow>> {
            unimplemented!()
        }
        async fn get_metadata(&self, _: i64) -> kani_app::error::Result<String> {
            unimplemented!()
        }
        async fn toggle_source_enabled(&self, _: i64, _: bool) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn toggle_source_favourite(&self, _: i64, _: bool) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn get_filter_list(
            &self,
            _: i64,
        ) -> kani_app::error::Result<kani_core::WitFilterList> {
            unimplemented!()
        }
        async fn get_all_preferences(
            &self,
            _: i64,
        ) -> kani_app::error::Result<Vec<(String, String)>> {
            unimplemented!()
        }
        async fn set_preference(&self, _: i64, _: &str, _: &str) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn append_pref_list_item(
            &self,
            _: i64,
            _: &str,
            _: String,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn remove_pref_list_item(
            &self,
            _: i64,
            _: &str,
            _: &str,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn toggle_pref_select_item(
            &self,
            _: i64,
            _: &str,
            _: String,
            _: bool,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn get_source_url(&self, _: i64, _: &str) -> kani_app::error::Result<String> {
            unimplemented!()
        }
        async fn get_chapter_list_paged(
            &self,
            _: i64,
            _: &str,
            _: i32,
            _: i32,
            _: Option<String>,
        ) -> kani_app::error::Result<String> {
            unimplemented!()
        }
        async fn get_chapter_sort_list(&self, _: i64) -> kani_app::error::Result<Vec<SortOption>> {
            unimplemented!()
        }
        async fn set_source_download_concurrency(
            &self,
            _: i64,
            _: Option<i64>,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn set_source_browser_enabled(&self, _: i64, _: bool) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn delete_source(&self, _: i64, _: UserId) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn list_active_source_ids(&self) -> kani_app::error::Result<Vec<i64>> {
            unimplemented!()
        }
        async fn get_source_pref_schema(
            &self,
            _: i64,
        ) -> kani_app::error::Result<Vec<kani_core::PreferenceSpec>> {
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
    async fn list_sources_returns_items_without_appservice() {
        let svc: Arc<dyn SourceDomain> = Arc::new(StubSources);
        let response = list_sources(AuthGuard(stub_user(), PhantomData), State(svc))
            .await
            .unwrap();
        let body = axum::response::IntoResponse::into_response(response);
        assert_eq!(body.status(), axum::http::StatusCode::OK);
    }
}
