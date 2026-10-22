//! Webhook CRUD, delivery & per-manga notify routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/webhooks", get(list_webhooks).post(create_webhook))
        .route(
            "/webhooks/{id}",
            patch(update_webhook).delete(delete_webhook),
        )
        .route("/webhooks/{id}/test", post(test_webhook))
        .route("/webhooks/{id}/deliveries", get(list_webhook_deliveries))
        .route(
            "/manga/{id}/webhook-notify",
            get(get_manga_webhook_notify).put(set_manga_webhook_notify),
        )
}

#[utoipa::path(
    get, path = "/rest/webhooks",
    responses(
        (status = 200, description = "All configured webhooks"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn list_webhooks(
    _: AuthGuard<crate::permissions::guards::SettingsEditAdvanced>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let webhooks = state.webhook_service.list_webhooks().await?;
    Ok(Json(webhooks))
}

#[utoipa::path(
    post, path = "/rest/webhooks",
    request_body(content = inline(serde_json::Value), description = "Webhook configuration (url, events, enabled)"),
    responses(
        (status = 201, description = "Webhook created"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn create_webhook(
    _: AuthGuard<crate::permissions::guards::SettingsEditAdvanced>,
    State(state): State<AppState>,
    Json(body): Json<kani_app::service::webhooks::CreateWebhookBody>,
) -> Result<impl IntoResponse, AppError> {
    let webhook = state.webhook_service.create_webhook(body).await?;
    Ok((StatusCode::CREATED, Json(webhook)))
}

#[utoipa::path(
    patch, path = "/rest/webhooks/{id}",
    params(("id" = i64, Path, description = "Webhook ID")),
    request_body(content = inline(serde_json::Value), description = "Fields to update"),
    responses(
        (status = 200, description = "Webhook updated"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn update_webhook(
    _: AuthGuard<crate::permissions::guards::SettingsEditAdvanced>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<kani_app::service::webhooks::UpdateWebhookBody>,
) -> Result<impl IntoResponse, AppError> {
    let webhook = state.webhook_service.update_webhook(id, body).await?;
    Ok(Json(webhook))
}

#[utoipa::path(
    delete, path = "/rest/webhooks/{id}",
    params(("id" = i64, Path, description = "Webhook ID")),
    responses(
        (status = 200, description = "Webhook deleted"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn delete_webhook(
    _: AuthGuard<crate::permissions::guards::SettingsEditAdvanced>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    state.webhook_service.delete_webhook(id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(
    post, path = "/rest/webhooks/{id}/test",
    params(("id" = i64, Path, description = "Webhook ID")),
    responses(
        (status = 200, description = "Test delivery result"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn test_webhook(
    _: AuthGuard<crate::permissions::guards::SettingsEditAdvanced>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    match state.webhook_service.send_test(id).await {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(e) => Ok(Json(json!({ "ok": false, "error": e.to_string() }))),
    }
}

#[utoipa::path(
    get, path = "/rest/webhooks/{id}/deliveries",
    params(("id" = i64, Path, description = "Webhook ID")),
    responses(
        (status = 200, description = "Recent delivery attempts for this webhook"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn list_webhook_deliveries(
    _: AuthGuard<crate::permissions::guards::SettingsEditAdvanced>,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let rows = state.webhook_service.list_deliveries(id).await?;
    Ok(Json(rows))
}

#[utoipa::path(
    get, path = "/rest/manga/{id}/webhook-notify",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Whether webhook notifications are enabled for this manga"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn get_manga_webhook_notify(
    _: AuthGuard<crate::permissions::guards::LibraryView>,
    State(state): State<AppState>,
    Path(id): Path<MangaId>,
) -> Result<impl IntoResponse, AppError> {
    let enabled = state.webhook_service.get_manga_notify(id).await?;
    Ok(Json(json!({ "enabled": enabled })))
}

#[utoipa::path(
    put, path = "/rest/manga/{id}/webhook-notify",
    params(("id" = i64, Path, description = "Manga ID")),
    responses(
        (status = 200, description = "Webhook notify setting updated"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "manga"
)]
pub(super) async fn set_manga_webhook_notify(
    _: AuthGuard<crate::permissions::guards::LibraryManage>,
    State(state): State<AppState>,
    Path(id): Path<MangaId>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, AppError> {
    let enabled = body
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| AppError::ValidationError("Missing 'enabled' boolean field".into()))?;
    state.webhook_service.set_manga_notify(id, enabled).await?;
    Ok(Json(json!({ "ok": true })))
}
