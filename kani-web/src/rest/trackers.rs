//! Tracker linking, OAuth, mapping & sync routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/manga/{id}/tracking",
            get(get_manga_tracking_handler).put(set_manga_tracking_handler),
        )
        .route("/trackers", get(list_trackers))
        .route("/trackers/{id}/auth_url", get(get_tracker_auth_url))
        .route("/trackers/{id}/callback", get(tracker_oauth_callback))
        .route("/trackers/{id}/unlink", post(unlink_tracker))
        .route("/trackers/{id}/search", get(search_tracker_manga))
        .route(
            "/trackers/{id}/config",
            get(get_tracker_config)
                .put(set_tracker_config)
                .delete(delete_tracker_config),
        )
        .route(
            "/manga/{id}/tracker_mappings",
            get(get_tracker_mappings).put(set_tracker_mapping),
        )
        .route(
            "/manga/{id}/tracker_mappings/{tracker_id}",
            delete(delete_tracker_mapping),
        )
        .route("/trackers/sync", post(sync_all_trackers))
        .route("/manga/{id}/sync", post(sync_manga_trackers))
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/tracking",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "User tracking state for this manga across all trackers"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_manga_tracking_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    let tracking = svc.get_manga_tracking(user.id, manga_id).await?;
    Ok(Json(tracking))
}

#[utoipa::path(
    put, path = "/rest/manga/{id}/tracking",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = SetMangaTrackingRequest,
    responses(
        (status = 204, description = "Tracking state updated"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn set_manga_tracking_handler(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<SetMangaTrackingRequest>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(status) = body.status {
        svc.set_manga_status(user.id, manga_id, status).await?;
    }
    if let Some(score) = body.score {
        svc.set_manga_score(user.id, manga_id, score).await?;
    }
    if let Some(enabled) = body.tracking_enabled {
        svc.set_manga_tracking_enabled(user.id, manga_id, enabled)
            .await?;
    }
    if let Some(notify) = body.notify_new_chapters {
        svc.set_manga_notify(user.id, manga_id, notify).await?;
    }
    if let Some(dir) = body.reading_direction {
        let dir = dir.to_lowercase();
        if dir == "rtl" || dir == "ltr" {
            svc.set_reading_direction(user.id, manga_id, &dir).await?;
        }
    }
    if let Some(prefs) = body.reader_prefs {
        svc.set_reader_prefs(user.id, manga_id, &prefs)
            .await
            .map_err(|_| AppError::ValidationError("reader_prefs must be a JSON object".into()))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/trackers",
    responses(
        (status = 200, description = "All available trackers with their link status for the current user"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn list_trackers(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
) -> Result<impl IntoResponse, AppError> {
    let items = svc.list_trackers_status(user.id).await?;
    let trackers: Vec<_> = items
        .into_iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "configured": t.configured,
                "linked": t.linked,
                "needs_reauth": t.needs_reauth,
            })
        })
        .collect();
    Ok(Json(trackers))
}

#[utoipa::path(
    get, path = "/rest/trackers/{id}/auth_url",
    params(
        ("id" = i64, Path, description = "Tracker ID"),
        ("redirect_uri" = String, Query, description = "OAuth redirect URI"),
    ),
    responses(
        (status = 200, description = "OAuth authorization URL for this tracker"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_tracker_auth_url(
    _: AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(tracker_id): Path<i64>,
    Query(q): Query<TrackerAuthUrlQuery>,
) -> Result<impl IntoResponse, AppError> {
    let url = svc
        .get_tracker_auth_url(tracker_id, &q.redirect_uri)
        .await?;
    Ok(Json(json!({ "url": url })))
}

#[utoipa::path(
    get, path = "/rest/trackers/{id}/callback",
    params(
        ("id" = i64, Path, description = "Tracker ID"),
        ("code" = String, Query, description = "OAuth authorization code"),
        ("state" = String, Query, description = "OAuth state token"),
    ),
    responses(
        (status = 200, description = "OAuth callback handled; renders success HTML"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn tracker_oauth_callback(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(tracker_id): Path<i64>,
    Query(q): Query<TrackerCallbackQuery>,
) -> Result<impl IntoResponse, AppError> {
    svc.complete_tracker_oauth(user.id, tracker_id, &q.code, &q.state)
        .await?;
    Ok((
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        OAUTH_SUCCESS_HTML,
    ))
}

#[utoipa::path(
    post, path = "/rest/trackers/{id}/unlink",
    params(("id" = i64, Path, description = "Tracker ID")),
    responses(
        (status = 204, description = "Tracker unlinked"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn unlink_tracker(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(tracker_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    svc.unlink_tracker(user.id, tracker_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/trackers/{id}/search",
    params(
        ("id" = i64, Path, description = "Tracker ID"),
        ("query" = String, Query, description = "Search query"),
    ),
    responses(
        (status = 200, description = "Manga search results from the tracker"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn search_tracker_manga(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(tracker_id): Path<i64>,
    Query(q): Query<TrackerSearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    let results = svc
        .search_tracker_manga(user.id, tracker_id, &q.query)
        .await?;
    Ok(Json(results))
}

#[utoipa::path(
    get, path = "/rest/trackers/{id}/config",
    params(("id" = i64, Path, description = "Tracker ID")),
    responses(
        (status = 200, description = "Tracker OAuth client configuration"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Admin permission required"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_tracker_config(
    _: AuthGuard<crate::permissions::guards::SettingsEditAdvanced>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(tracker_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let config = svc.get_tracker_config(tracker_id).await?;
    match config {
        Some((client_id, secret_configured)) => Ok(Json(json!({
            "client_id": client_id,
            "secret_configured": secret_configured,
        }))),
        None => Ok(Json(json!({
            "client_id": null,
            "secret_configured": false,
        }))),
    }
}

#[utoipa::path(
    put, path = "/rest/trackers/{id}/config",
    params(("id" = i64, Path, description = "Tracker ID")),
    request_body = SetTrackerConfigRequest,
    responses(
        (status = 204, description = "Tracker OAuth config updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Admin permission required"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn set_tracker_config(
    _: AuthGuard<crate::permissions::guards::SettingsEditAdvanced>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(tracker_id): Path<i64>,
    Json(body): Json<SetTrackerConfigRequest>,
) -> Result<impl IntoResponse, AppError> {
    let secret = body.client_secret.as_deref().filter(|s| !s.is_empty());
    svc.set_tracker_config(tracker_id, &body.client_id, secret)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete, path = "/rest/trackers/{id}/config",
    params(("id" = i64, Path, description = "Tracker ID")),
    responses(
        (status = 204, description = "Tracker OAuth config deleted"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Admin permission required"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn delete_tracker_config(
    _: AuthGuard<crate::permissions::guards::SettingsEditAdvanced>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(tracker_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    svc.delete_tracker_config(tracker_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/tracker_mappings",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "External tracker mappings for this manga"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_tracker_mappings(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    let mappings = svc.get_tracker_mappings(user.id, manga_id).await?;
    let response: Vec<_> = mappings
        .into_iter()
        .map(|m| {
            json!({
                "tracker_id": m.tracker_id,
                "tracker_name": m.tracker_name,
                "tracker_manga_id": m.tracker_manga_id,
                // RFC 3339, not time's default array form, which `new Date()`
                // cannot parse.
                "last_synced_at": m.last_synced_at.and_then(|t| {
                    t.format(&time::format_description::well_known::Rfc3339).ok()
                }),
                "suggested_manga_id": m.suggested_manga_id,
            })
        })
        .collect();
    Ok(Json(response))
}

#[utoipa::path(
    put, path = "/rest/manga/{id}/tracker_mappings",
    params(("id" = i64, Path, description = "Manga ID")),
    request_body = SetTrackerMappingRequest,
    responses(
        (status = 204, description = "Tracker mapping set"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn set_tracker_mapping(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(manga_id): Path<MangaId>,
    Json(body): Json<SetTrackerMappingRequest>,
) -> Result<impl IntoResponse, AppError> {
    svc.set_tracker_mapping(user.id, body.tracker_id, manga_id, &body.tracker_manga_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete, path = "/rest/manga/{id}/tracker_mappings/{tracker_id}",
    params(
        ("id" = i64, Path, description = "Manga ID"),
        ("tracker_id" = i64, Path, description = "Tracker ID"),
    ),
    responses(
        (status = 204, description = "Tracker mapping deleted"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn delete_tracker_mapping(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path((manga_id, tracker_id)): Path<(MangaId, i64)>,
) -> Result<impl IntoResponse, AppError> {
    svc.delete_tracker_mapping(user.id, tracker_id, manga_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/rest/trackers/sync",
    responses(
        (status = 204, description = "All tracker states synced for the current user"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn sync_all_trackers(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
) -> Result<impl IntoResponse, AppError> {
    svc.sync_all_trackers(user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/rest/manga/{id}/sync",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 204, description = "Tracker states synced for this manga"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn sync_manga_trackers(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(svc): State<Arc<dyn TrackerDomain>>,
    Path(manga_id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    svc.sync_manga_trackers(user.id, manga_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use kani_app::service::trackers::TrackerStatusItem;
    use kani_shared::types::{MangaTracking, MangaTrackingStatus};

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

    struct StubTrackers;

    #[async_trait::async_trait]
    impl TrackerDomain for StubTrackers {
        async fn list_trackers_status(
            &self,
            _user_id: UserId,
        ) -> kani_app::error::Result<Vec<TrackerStatusItem>> {
            Ok(vec![TrackerStatusItem {
                id: 42,
                name: "AniList".into(),
                configured: false,
                linked: false,
                needs_reauth: false,
            }])
        }
        async fn get_tracker_auth_url(
            &self,
            _tracker_id: i64,
            _redirect_uri: &str,
        ) -> kani_app::error::Result<String> {
            unimplemented!()
        }
        async fn complete_tracker_oauth(
            &self,
            _user_id: UserId,
            _tracker_id: i64,
            _code: &str,
            _state: &str,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn unlink_tracker(
            &self,
            _user_id: UserId,
            _tracker_id: i64,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn search_tracker_manga(
            &self,
            _user_id: UserId,
            _tracker_id: i64,
            _query: &str,
        ) -> kani_app::error::Result<Vec<kani_app::service::trackers::TrackerMangaResult>> {
            unimplemented!()
        }
        async fn get_tracker_mappings(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
        ) -> kani_app::error::Result<Vec<kani_app::service::trackers::TrackerMappingItem>> {
            unimplemented!()
        }
        async fn get_tracker_config(
            &self,
            _tracker_id: i64,
        ) -> kani_app::error::Result<Option<(String, bool)>> {
            unimplemented!()
        }
        async fn set_tracker_config(
            &self,
            _tracker_id: i64,
            _client_id: &str,
            _client_secret: Option<&str>,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn delete_tracker_config(&self, _tracker_id: i64) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn set_tracker_mapping(
            &self,
            _user_id: UserId,
            _tracker_id: i64,
            _manga_id: MangaId,
            _tracker_manga_id: &str,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn delete_tracker_mapping(
            &self,
            _user_id: UserId,
            _tracker_id: i64,
            _manga_id: MangaId,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn sync_all_trackers(&self, _user_id: UserId) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn sync_manga_trackers(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn get_manga_tracking(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
        ) -> kani_app::error::Result<MangaTracking> {
            unimplemented!()
        }
        async fn set_manga_status(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _status: MangaTrackingStatus,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn set_manga_score(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _score: f64,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn set_manga_tracking_enabled(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _enabled: bool,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn set_manga_notify(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _notify: bool,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn set_reading_direction(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _direction: &str,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
        async fn set_reader_prefs(
            &self,
            _user_id: UserId,
            _manga_id: MangaId,
            _prefs: &str,
        ) -> kani_app::error::Result<()> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn list_trackers_returns_ok_without_appservice() {
        let svc: Arc<dyn TrackerDomain> = Arc::new(StubTrackers);
        let response = list_trackers(AuthGuard(stub_user(), PhantomData), State(svc))
            .await
            .unwrap();
        let resp = axum::response::IntoResponse::into_response(response);
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }
}
