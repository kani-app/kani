//! Per-chapter reading, progress, bookmark & note routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/chapter/{id}/pages", get(get_chapter_page_manifest))
        .route("/chapter/{id}/progress", put(set_chapter_progress_handler))
        .route(
            "/chapter/{id}/bookmarks",
            get(get_bookmarks_handler).post(toggle_bookmark_handler),
        )
        .route(
            "/chapter/{id}/note",
            get(get_chapter_note_handler).put(set_chapter_note_handler),
        )
        .route(
            "/manga/{id}/chapter-notes",
            get(get_manga_chapter_notes_handler),
        )
        .route(
            "/chapters/read_status",
            put(set_chapter_read_status_handler),
        )
        .route(
            "/manga/{id}/continue_reading",
            get(get_continue_reading_handler),
        )
        .route(
            "/manga/{id}/chapters/mark_up_to",
            post(mark_chapters_up_to_handler),
        )
}

#[utoipa::path(
    get, path = "/rest/chapter/{id}/pages",
    params(("id" = i64, Path, description = "Chapter ID")),
    responses(
        (status = 200, description = "Page manifest: ordered list of page URLs and dimensions"),
        (status = 401, description = "Not authenticated"),
        (status = 404, description = "Chapter not found"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn get_chapter_page_manifest(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::LibraryView>,
    State(svc): State<Arc<dyn ChapterDomain>>,
    Path(id): Path<ChapterId>,
) -> Result<impl IntoResponse, AppError> {
    let manifest = svc.get_chapter_page_manifest(id, user.id).await?;
    Ok(Json(manifest))
}

#[utoipa::path(
    put, path = "/rest/chapter/{id}/progress",
    params(("id" = i64, Path, description = "Chapter ID")),
    request_body = SetChapterProgressRequest,
    responses(
        (status = 204, description = "Progress recorded"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn set_chapter_progress_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn ChapterDomain>>,
    Path(chapter_id): Path<ChapterId>,
    Json(body): Json<SetChapterProgressRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.set_chapter_progress(user.id, chapter_id, body.page)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/chapter/{id}/bookmarks",
    params(("id" = i64, Path, description = "Chapter ID")),
    responses(
        (status = 200, description = "List of bookmarked page indices"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn get_bookmarks_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn ChapterDomain>>,
    Path(chapter_id): Path<ChapterId>,
) -> Result<impl IntoResponse, AppError> {
    let pages = svc.get_bookmarks(user.id, chapter_id).await?;
    Ok(Json(pages))
}

#[utoipa::path(
    post, path = "/rest/chapter/{id}/bookmarks",
    params(("id" = i64, Path, description = "Chapter ID")),
    responses(
        (status = 200, description = "Bookmark toggled; returns new bookmarked state"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn toggle_bookmark_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn ChapterDomain>>,
    Path(chapter_id): Path<ChapterId>,
    Json(body): Json<crate::models::ToggleBookmarkRequest>,
) -> Result<impl IntoResponse, AppError> {
    let bookmarked = svc
        .toggle_bookmark(user.id, chapter_id, body.page_index)
        .await?;
    Ok(Json(json!({ "bookmarked": bookmarked })))
}

#[utoipa::path(
    get, path = "/rest/chapter/{id}/note",
    params(("id" = i64, Path, description = "Chapter ID")),
    responses(
        (status = 200, description = "Reader note for this chapter"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn get_chapter_note_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn ChapterDomain>>,
    Path(chapter_id): Path<ChapterId>,
) -> Result<impl IntoResponse, AppError> {
    let note = svc.get_chapter_note(user.id, chapter_id).await?;
    Ok(Json(json!({ "note": note })))
}

#[utoipa::path(
    put, path = "/rest/chapter/{id}/note",
    params(("id" = i64, Path, description = "Chapter ID")),
    request_body = SetChapterNoteRequest,
    responses(
        (status = 204, description = "Note saved"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn set_chapter_note_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn ChapterDomain>>,
    Path(chapter_id): Path<ChapterId>,
    Json(body): Json<crate::models::SetChapterNoteRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.set_chapter_note(user.id, chapter_id, &body.note)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/chapter-notes",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "All reader notes for chapters of this manga"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn get_manga_chapter_notes_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn ChapterDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    let notes = svc
        .get_manga_chapter_notes_with_text(user.id, manga_id)
        .await?;
    let items: Vec<_> = notes
        .into_iter()
        .map(|(chapter_id, chapter_number, note)| {
            json!({ "chapter_id": chapter_id, "chapter_number": chapter_number, "note": note })
        })
        .collect();
    Ok(Json(json!({ "notes": items })))
}

#[utoipa::path(
    put, path = "/rest/chapters/read_status",
    request_body = SetReadStatusRequest,
    responses(
        (status = 204, description = "Read status updated"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn set_chapter_read_status_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn ChapterDomain>>,
    Json(body): Json<SetReadStatusRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.set_chapter_read_status(user.id, body.chapter_ids, body.is_read)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/continue_reading",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Next chapter to read and current page, or null"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn get_continue_reading_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn ChapterDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    let info = svc.get_continue_reading_chapter(user.id, manga_id).await?;
    Ok(Json(info))
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/chapters/mark_up_to",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = MarkUpToRequest,
    responses(
        (status = 204, description = "Chapters marked read/unread up to the given chapter number"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "chapters"
)]
pub(super) async fn mark_chapters_up_to_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn ChapterDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<MarkUpToRequest>,
) -> Result<impl IntoResponse, AppError> {
    let ids = svc
        .get_chapters_up_to(manga_id, body.chapter_number)
        .await?;
    svc.set_chapter_read_status(user.id, ids, body.is_read)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use axum::extract::{Path, State};
    use kani_app::ids::{ChapterId, MangaId, UserId};
    use kani_app::models::ChapterPageManifest;
    use kani_app::service::traits::ChapterDomain;
    use kani_shared::types::ContinueReadingChapter;
    use std::sync::Arc;

    struct StubChapters;

    #[async_trait::async_trait]
    impl ChapterDomain for StubChapters {
        async fn get_bookmarks(
            &self,
            _: UserId,
            _: ChapterId,
        ) -> kani_app::error::Result<Vec<i64>> {
            Ok(vec![2, 5, 11])
        }
        async fn get_chapter_page_manifest(
            &self,
            _: ChapterId,
            _: UserId,
        ) -> kani_app::error::Result<ChapterPageManifest> {
            unimplemented!()
        }
        async fn set_chapter_progress(
            &self,
            _: UserId,
            _: ChapterId,
            _: i64,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn toggle_bookmark(
            &self,
            _: UserId,
            _: ChapterId,
            _: i64,
        ) -> kani_app::error::Result<bool> {
            unimplemented!()
        }
        async fn get_chapter_note(
            &self,
            _: UserId,
            _: ChapterId,
        ) -> kani_app::error::Result<Option<String>> {
            unimplemented!()
        }
        async fn set_chapter_note(
            &self,
            _: UserId,
            _: ChapterId,
            _: &str,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn get_manga_chapter_notes_with_text(
            &self,
            _: UserId,
            _: MangaId,
        ) -> kani_app::error::Result<Vec<(ChapterId, f64, String)>> {
            unimplemented!()
        }
        async fn set_chapter_read_status(
            &self,
            _: UserId,
            _: Vec<ChapterId>,
            _: bool,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn get_continue_reading_chapter(
            &self,
            _: UserId,
            _: MangaId,
        ) -> kani_app::error::Result<Option<ContinueReadingChapter>> {
            unimplemented!()
        }
        async fn get_chapters_up_to(
            &self,
            _: MangaId,
            _: f64,
        ) -> kani_app::error::Result<Vec<ChapterId>> {
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
    async fn bookmarks_returns_pages_without_appservice() {
        let svc: Arc<dyn ChapterDomain> = Arc::new(StubChapters);
        let response = get_bookmarks_handler(
            AuthGuard(stub_user(), PhantomData),
            State(svc),
            Path(ChapterId(42)),
        )
        .await
        .unwrap();
        let body = axum::response::IntoResponse::into_response(response);
        assert_eq!(body.status(), axum::http::StatusCode::OK);
    }
}
