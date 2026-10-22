use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/saved-searches",
            get(list_saved_searches).post(create_saved_search),
        )
        .route(
            "/saved-searches/{id}",
            put(update_saved_search).delete(delete_saved_search),
        )
}

#[derive(serde::Deserialize)]
pub(super) struct SavedSearchBody {
    pub name: String,
    pub query_json: String,
}

#[utoipa::path(
    get, path = "/rest/saved-searches",
    responses(
        (status = 200, description = "The caller's saved searches"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn list_saved_searches(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(state.service.list_saved_searches(user.id).await?))
}

#[utoipa::path(
    post, path = "/rest/saved-searches",
    request_body(content = inline(serde_json::Value), description = "Name and serialised query"),
    responses(
        (status = 201, description = "Saved search created"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn create_saved_search(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
    Json(body): Json<SavedSearchBody>,
) -> Result<impl IntoResponse, AppError> {
    let item = state
        .service
        .create_saved_search(user.id, body.name, body.query_json)
        .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

#[utoipa::path(
    put, path = "/rest/saved-searches/{id}",
    params(("id" = i64, Path, description = "Saved search id")),
    request_body(content = inline(serde_json::Value), description = "Replacement name and serialised query"),
    responses(
        (status = 200, description = "Saved search updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such saved search, or it belongs to another user"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn update_saved_search(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<SavedSearchBody>,
) -> Result<impl IntoResponse, AppError> {
    let item = state
        .service
        .update_saved_search(id, user.id, body.name, body.query_json)
        .await?;
    Ok(Json(item))
}

#[utoipa::path(
    delete, path = "/rest/saved-searches/{id}",
    params(("id" = i64, Path, description = "Saved search id")),
    responses(
        (status = 204, description = "Saved search deleted"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such saved search, or it belongs to another user"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn delete_saved_search(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    state.service.delete_saved_search(id, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
