//! Chapter download lifecycle & history routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/downloads/history", get(get_download_history))
        .route("/chapter/{id}/download", post(start_download))
        .route("/chapter/{id}/download/retry", post(retry_download))
        .route("/chapter/{id}/delete", delete(delete_downloaded))
        .route("/chapter/{id}/cancel", post(cancel_download))
        .route("/downloads/active", delete(cancel_all_global_downloads))
        .route(
            "/manga/{id}/download-status",
            get(get_manga_download_status),
        )
}

#[utoipa::path(
    get, path = "/rest/downloads/history",
    params(("limit" = Option<i64>, Query, description = "Max items to return (default 50)")),
    responses(
        (status = 200, description = "Recent download history entries"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn get_download_history(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn DownloadDomain>>,
    Query(q): Query<DownloadHistoryQuery>,
) -> Result<impl IntoResponse, AppError> {
    let items = svc.get_download_history(q.limit).await?;
    Ok(Json(items))
}

#[utoipa::path(
    post, path = "/rest/chapter/{id}/download",
    params(("id" = i64, Path, description = "Chapter ID")),
    responses(
        (status = 202, description = "Download queued"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn start_download(
    _: AuthGuard<crate::permissions::guards::ChapterDownload>,
    State(svc): State<Arc<dyn DownloadDomain>>,
    Path(id): Path<ChapterId>,
) -> Result<impl IntoResponse, AppError> {
    let job_id = svc.download_chapter(id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "job_id": job_id.to_string(), "chapter_id": id.0 })),
    ))
}

#[utoipa::path(
    post, path = "/rest/chapter/{id}/download/retry",
    params(("id" = i64, Path, description = "Chapter ID")),
    responses(
        (status = 202, description = "Download retry queued"),
        (status = 401, description = "Not authenticated"),
        (status = 409, description = "Chapter is missing from source — remove it from the library instead"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn retry_download(
    _: AuthGuard<crate::permissions::guards::ChapterDownload>,
    State(svc): State<Arc<dyn DownloadDomain>>,
    Path(id): Path<ChapterId>,
) -> Result<impl IntoResponse, AppError> {
    let job_id = svc.retry_chapter_download(id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "job_id": job_id.to_string(), "chapter_id": id.0 })),
    ))
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/download-status",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Download status counts and failed chapters"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn get_manga_download_status(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn DownloadDomain>>,
    Path(id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    let status = svc.get_manga_download_status(id).await?;
    Ok(Json(status))
}

#[utoipa::path(
    delete, path = "/rest/chapter/{id}/delete",
    params(("id" = i64, Path, description = "Chapter ID")),
    responses(
        (status = 200, description = "Downloaded files removed"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn delete_downloaded(
    _: AuthGuard<crate::permissions::guards::ChapterDelete>,
    State(svc): State<Arc<dyn DownloadDomain>>,
    Path(id): Path<ChapterId>,
) -> Result<impl IntoResponse, AppError> {
    svc.delete_downloaded(id).await?;
    Ok((StatusCode::OK, Json(json!({}))))
}

#[utoipa::path(
    post, path = "/rest/chapter/{id}/cancel",
    params(("id" = i64, Path, description = "Chapter ID")),
    responses(
        (status = 200, description = "Download cancelled"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn cancel_download(
    _: AuthGuard<crate::permissions::guards::ChapterDownload>,
    State(svc): State<Arc<dyn DownloadDomain>>,
    Path(chapter_id): Path<ChapterId>,
) -> Result<impl IntoResponse, AppError> {
    svc.cancel_download(chapter_id).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    delete, path = "/rest/downloads/active",
    responses(
        (status = 200, description = "All active downloads cancelled"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn cancel_all_global_downloads(
    _: AuthGuard<crate::permissions::guards::ServerManage>,
    State(svc): State<Arc<dyn DownloadDomain>>,
) -> Result<impl IntoResponse, AppError> {
    svc.cancel_all_global_downloads().await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use axum::extract::{Query, State};
    use kani_app::ids::{ChapterId, UserId};
    use kani_app::service::traits::DownloadDomain;
    use std::sync::Arc;

    struct StubDownloads;

    #[async_trait::async_trait]
    impl DownloadDomain for StubDownloads {
        async fn get_download_history(
            &self,
            _limit: i64,
        ) -> kani_app::error::Result<Vec<serde_json::Value>> {
            Ok(vec![serde_json::json!({ "chapter_id": 1 })])
        }
        async fn download_chapter(&self, _: ChapterId) -> kani_app::error::Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn retry_chapter_download(
            &self,
            _: ChapterId,
        ) -> kani_app::error::Result<uuid::Uuid> {
            unimplemented!()
        }
        async fn delete_downloaded(&self, _: ChapterId) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn cancel_download(&self, _: ChapterId) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn cancel_all_global_downloads(&self) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn get_manga_download_status(
            &self,
            _: kani_app::ids::MangaId,
        ) -> kani_app::error::Result<serde_json::Value> {
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
    async fn download_history_returns_items_without_appservice() {
        let svc: Arc<dyn DownloadDomain> = Arc::new(StubDownloads);
        let response = get_download_history(
            AuthGuard(stub_user(), PhantomData),
            State(svc),
            Query(DownloadHistoryQuery { limit: 10 }),
        )
        .await
        .unwrap();
        let body = axum::response::IntoResponse::into_response(response);
        assert_eq!(body.status(), axum::http::StatusCode::OK);
    }
}
