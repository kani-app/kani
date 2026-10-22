use super::*;
use kani_app::ids::ChapterId;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/manga/{id}/volumes", get(list_volumes).post(create_volume))
        .route(
            "/manga/{id}/volumes/{vid}",
            put(update_volume).delete(delete_volume),
        )
        .route(
            "/manga/{id}/chapters/{cid}/volume",
            put(assign_chapter_volume),
        )
}

#[derive(serde::Deserialize)]
pub(super) struct VolumeBody {
    pub name: Option<String>,
    pub volume_num: Option<f64>,
}

#[derive(serde::Deserialize)]
pub(super) struct AssignVolumeBody {
    pub volume_id: Option<i64>,
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/volumes",
    params(("id" = i64, Path, description = "Manga id")),
    responses(
        (status = 200, description = "Volumes defined for a manga"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn list_volumes(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(state.service.list_volumes(manga_id).await?))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/volumes",
    params(("id" = i64, Path, description = "Manga id")),
    request_body(content = inline(serde_json::Value), description = "Optional volume name and number"),
    responses(
        (status = 201, description = "Volume created"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn create_volume(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<VolumeBody>,
) -> Result<impl IntoResponse, AppError> {
    let vol = state
        .service
        .create_volume(manga_id, body.name, body.volume_num)
        .await?;
    Ok((StatusCode::CREATED, Json(vol)))
}

#[utoipa::path(
    put, path = "/rest/manga/{id}/volumes/{vid}",
    params(("id" = i64, Path, description = "Manga id"), ("vid" = i64, Path, description = "Volume id")),
    request_body(content = inline(serde_json::Value), description = "Replacement volume name and number"),
    responses(
        (status = 200, description = "Volume updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such volume for this manga"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn update_volume(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path((manga_id, volume_id)): Path<(MangaId, i64)>,
    Json(body): Json<VolumeBody>,
) -> Result<impl IntoResponse, AppError> {
    let vol = state
        .service
        .update_volume(volume_id, manga_id, body.name, body.volume_num)
        .await?;
    Ok(Json(vol))
}

#[utoipa::path(
    delete, path = "/rest/manga/{id}/volumes/{vid}",
    params(("id" = i64, Path, description = "Manga id"), ("vid" = i64, Path, description = "Volume id")),
    responses(
        (status = 204, description = "Volume deleted"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such volume for this manga"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn delete_volume(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path((manga_id, volume_id)): Path<(MangaId, i64)>,
) -> Result<impl IntoResponse, AppError> {
    state.service.delete_volume(volume_id, manga_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    put, path = "/rest/manga/{id}/chapters/{cid}/volume",
    params(("id" = i64, Path, description = "Manga id"), ("cid" = i64, Path, description = "Chapter id")),
    request_body(content = inline(serde_json::Value), description = "Target volume id, or null to unassign"),
    responses(
        (status = 200, description = "Chapter assigned to the volume"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such chapter or volume"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn assign_chapter_volume(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path((manga_id, chapter_id)): Path<(MangaId, ChapterId)>,
    Json(body): Json<AssignVolumeBody>,
) -> Result<impl IntoResponse, AppError> {
    state
        .service
        .assign_chapter_volume(chapter_id, manga_id, body.volume_id)
        .await?;
    Ok(Json(json!({})))
}
