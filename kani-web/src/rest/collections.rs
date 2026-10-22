use super::*;
use kani_app::service::smart_collections::SmartCollectionRule;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/collections",
            get(list_collections).post(create_collection),
        )
        .route(
            "/collections/{id}",
            put(update_collection).delete(delete_collection),
        )
        .route("/collections/{id}/manga", get(get_collection_manga))
}

#[derive(serde::Deserialize)]
pub(super) struct CollectionBody {
    pub name: String,
    pub rule: SmartCollectionRule,
    #[serde(default)]
    pub sort_order: i64,
}

#[utoipa::path(
    get, path = "/rest/collections",
    responses(
        (status = 200, description = "Smart collections, with their rules"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn list_collections(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(state.service.list_collections().await?))
}

#[utoipa::path(
    post, path = "/rest/collections",
    request_body(content = inline(serde_json::Value), description = "Name, smart-collection rule and sort order"),
    responses(
        (status = 201, description = "Collection created"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 422, description = "Malformed rule"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn create_collection(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Json(body): Json<CollectionBody>,
) -> Result<impl IntoResponse, AppError> {
    let col = state
        .service
        .create_collection(body.name, &body.rule, body.sort_order)
        .await?;
    Ok((StatusCode::CREATED, Json(col)))
}

#[utoipa::path(
    put, path = "/rest/collections/{id}",
    params(("id" = i64, Path, description = "Collection id")),
    request_body(content = inline(serde_json::Value), description = "Replacement name, rule and sort order"),
    responses(
        (status = 200, description = "Collection updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such collection"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn update_collection(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<CollectionBody>,
) -> Result<impl IntoResponse, AppError> {
    let col = state
        .service
        .update_collection(id, body.name, &body.rule, body.sort_order)
        .await?;
    Ok(Json(col))
}

#[utoipa::path(
    delete, path = "/rest/collections/{id}",
    params(("id" = i64, Path, description = "Collection id")),
    responses(
        (status = 204, description = "Collection deleted"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn delete_collection(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    state.service.delete_collection(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/collections/{id}/manga",
    params(("id" = i64, Path, description = "Collection id")),
    responses(
        (status = 200, description = "Manga ids the collection's rule currently selects"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "No such collection"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn get_collection_manga(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let col = state.service.get_collection(id).await?;
    let rule: SmartCollectionRule = serde_json::from_str(&col.rule_json)
        .map_err(|e| AppError::InternalServerError(format!("Invalid rule JSON: {e}")))?;
    let ids = state.service.evaluate_collection(&rule, user.id).await?;
    Ok(Json(json!({ "manga_ids": ids })))
}
