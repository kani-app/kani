//! Library filter facet routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/filters/tags", get(get_filter_tags))
        .route("/filters/authors", get(get_filter_authors))
        .route("/filters/artists", get(get_filter_artists))
}

#[utoipa::path(
    get, path = "/rest/filters/tags",
    responses(
        (status = 200, description = "Every tag present in the library, for filter facets"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn get_filter_tags(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(state.get_filter_tags().await?))
}

#[utoipa::path(
    get, path = "/rest/filters/authors",
    responses(
        (status = 200, description = "Every author present in the library, for filter facets"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn get_filter_authors(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(state.get_filter_authors().await?))
}

#[utoipa::path(
    get, path = "/rest/filters/artists",
    responses(
        (status = 200, description = "Every artist present in the library, for filter facets"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn get_filter_artists(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(state.get_filter_artists().await?))
}
