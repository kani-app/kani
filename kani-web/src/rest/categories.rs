//! Category CRUD & per-manga category routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/categories", get(list_categories).post(create_category))
        .route("/categories/reorder", put(reorder_categories))
        .route(
            "/categories/{id}",
            patch(rename_category).delete(delete_category_handler),
        )
        .route(
            "/manga/{id}/categories",
            get(get_manga_categories).put(set_manga_categories),
        )
}

#[utoipa::path(
    get, path = "/rest/categories",
    responses(
        (status = 200, description = "All categories"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn list_categories(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn CategoryDomain>>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(svc.list_categories().await?))
}

#[utoipa::path(
    post, path = "/rest/categories",
    request_body = CreateCategoryRequest,
    responses(
        (status = 201, description = "Category created; returns new ID"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn create_category(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn CategoryDomain>>,
    Json(body): Json<CreateCategoryRequest>,
) -> Result<impl IntoResponse, AppError> {
    let id = svc.create_category(&body.name, body.sort_order).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

#[utoipa::path(
    put, path = "/rest/categories/reorder",
    request_body = ReorderCategoriesRequest,
    responses(
        (status = 200, description = "Categories reordered"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn reorder_categories(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn CategoryDomain>>,
    Json(body): Json<ReorderCategoriesRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.reorder_categories(body.ordered_ids).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    patch, path = "/rest/categories/{id}",
    params(("id" = i64, Path, description = "Category ID")),
    request_body = RenameCategoryRequest,
    responses(
        (status = 200, description = "Category renamed"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn rename_category(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn CategoryDomain>>,
    Path(category_id): Path<i64>,
    Json(body): Json<RenameCategoryRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.rename_category(category_id, &body.name).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    delete, path = "/rest/categories/{id}",
    params(("id" = i64, Path, description = "Category ID")),
    responses(
        (status = 200, description = "Category deleted"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn delete_category_handler(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn CategoryDomain>>,
    Path(category_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    svc.delete_category(category_id).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/categories",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Categories assigned to this manga"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn get_manga_categories(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn CategoryDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(svc.get_manga_categories(manga_id).await?))
}

#[utoipa::path(
    put, path = "/rest/manga/{id}/categories",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = SetMangaCategoriesRequest,
    responses(
        (status = 200, description = "Categories updated for this manga"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn set_manga_categories(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(svc): State<Arc<dyn CategoryDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<SetMangaCategoriesRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.set_manga_categories(manga_id, body.category_ids)
        .await?;
    Ok(Json(json!({})))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use kani_shared::types::Category;

    fn stub_user() -> crate::auth::User {
        crate::auth::User {
            id: UserId(1),
            username: "stub".into(),
            email: "stub@test.com".into(),
            is_active: true,
            created_at: None,
            roles: vec![],
            password_hash: String::new(),
            change_id: vec![],
        }
    }

    struct StubCategories;

    #[async_trait::async_trait]
    impl CategoryDomain for StubCategories {
        async fn list_categories(&self) -> kani_app::error::Result<Vec<Category>> {
            Ok(vec![Category {
                id: 1,
                name: "Action".into(),
                sort_order: 0,
            }])
        }
        async fn create_category(
            &self,
            _name: &str,
            _sort_order: i64,
        ) -> kani_app::error::Result<i64> {
            unimplemented!()
        }
        async fn reorder_categories(&self, _ordered_ids: Vec<i64>) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn rename_category(&self, _id: i64, _name: &str) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn delete_category(&self, _id: i64) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn get_manga_categories(
            &self,
            _manga_id: MangaId,
        ) -> kani_app::error::Result<Vec<Category>> {
            unimplemented!()
        }
        async fn set_manga_categories(
            &self,
            _manga_id: MangaId,
            _category_ids: Vec<i64>,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn list_categories_returns_ok_without_appservice() {
        let svc: Arc<dyn CategoryDomain> = Arc::new(StubCategories);
        let response = list_categories(AuthGuard(stub_user(), PhantomData), State(svc))
            .await
            .unwrap();
        let resp = axum::response::IntoResponse::into_response(response);
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }
}
