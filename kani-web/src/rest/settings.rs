//! Server settings, auto-scan toggle & refresh routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings", get(get_settings).patch(update_settings))
        .route("/refresh/start", post(start_refresh_all_rest))
        .route("/refresh/status", get(get_refresh_status))
        .route("/settings/solver/test", post(test_solver))
}

#[utoipa::path(
    get, path = "/rest/settings",
    responses(
        (status = 200, description = "Server settings (download, scan, advanced, tracking, email)"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn get_settings(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::SettingsView>,
    auth: crate::auth::AuthSession,
    State(svc): State<Arc<dyn SettingsDomain>>,
) -> Result<impl IntoResponse, AppError> {
    let mut settings = svc.get_settings().await;

    let may_see_infrastructure = auth
        .backend
        .has_perm(
            &user,
            crate::permissions::Permission::Settings(crate::permissions::Settings::EditAdvanced),
        )
        .await
        .unwrap_or(false);

    if !may_see_infrastructure {
        settings.flaresolverr_url = String::new();
        settings.email_provider_config = String::new();
        settings.email_from_address = String::new();
        settings.app_url = String::new();
        settings.library_path = String::new();
        settings.wasm_storage_path = String::new();
    }

    Ok(Json(settings))
}

#[utoipa::path(
    patch, path = "/rest/settings",
    request_body(content = inline(serde_json::Value), description = "Partial settings update; variant determines which permission is required"),
    responses(
        (status = 200, description = "Settings updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions for the given settings category"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn update_settings(
    auth: AuthSession,
    State(svc): State<Arc<dyn SettingsDomain>>,
    Json(update): Json<crate::types::SettingsUpdate>,
) -> Result<impl IntoResponse, AppError> {
    use crate::types::SettingsUpdate;

    let user = auth
        .user
        .ok_or_else(|| AppError::Unauthorized("Not authenticated".into()))?;
    let required_perm = match &update {
        SettingsUpdate::Download(_) => {
            crate::permissions::Permission::Settings(crate::permissions::Settings::EditDownload)
        }
        SettingsUpdate::Scan(_) => {
            crate::permissions::Permission::Settings(crate::permissions::Settings::EditScan)
        }
        SettingsUpdate::Advanced(_) => {
            crate::permissions::Permission::Settings(crate::permissions::Settings::EditAdvanced)
        }
        SettingsUpdate::Tracking(_) => {
            crate::permissions::Permission::Settings(crate::permissions::Settings::EditScan)
        }
        SettingsUpdate::Email(_) => {
            crate::permissions::Permission::Settings(crate::permissions::Settings::EditAdvanced)
        }
        SettingsUpdate::Maintenance(_) => {
            crate::permissions::Permission::Settings(crate::permissions::Settings::EditAdvanced)
        }
        SettingsUpdate::Security(_) => {
            crate::permissions::Permission::Settings(crate::permissions::Settings::EditAdvanced)
        }
        SettingsUpdate::Performance(_) => {
            crate::permissions::Permission::Settings(crate::permissions::Settings::EditAdvanced)
        }
    };
    if !auth
        .backend
        .has_perm(&user, required_perm)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    {
        return Err(AppError::Forbidden("Insufficient permissions".into()));
    }
    if let crate::types::SettingsUpdate::Advanced(ref adv) = update {
        crate::HTTP_LOGGING_ENABLED.store(
            adv.http_request_logging,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    svc.update_settings(update, user.id).await?;
    Ok(Json(json!({})))
}

#[utoipa::path(
    post, path = "/rest/refresh/start",
    responses(
        (status = 202, description = "Library metadata refresh started"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn start_refresh_all_rest(
    _: AuthGuard<crate::permissions::guards::LibraryRefresh>,
    State(svc): State<Arc<dyn SettingsDomain>>,
) -> Result<impl IntoResponse, AppError> {
    svc.start_refresh_all().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({}))))
}

#[utoipa::path(
    get, path = "/rest/refresh/status",
    responses(
        (status = 200, description = "Whether a metadata refresh is currently running"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "library"
)]
pub(super) async fn get_refresh_status(
    _: AuthGuard<crate::permissions::guards::LibraryRefresh>,
    State(svc): State<Arc<dyn SettingsDomain>>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(json!({ "is_refreshing": svc.is_refreshing().await })))
}

#[utoipa::path(
    post, path = "/rest/settings/solver/test",
    responses(
        (status = 200, description = "Solver probe result"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Insufficient permissions"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn test_solver(
    _: AuthGuard<crate::permissions::guards::SettingsEditAdvanced>,
    Json(body): Json<SolverTestBody>,
) -> Result<impl IntoResponse, AppError> {
    let url = body.url.trim().to_string();
    if url.is_empty() {
        return Err(AppError::ValidationError(
            "A solver URL is required.".into(),
        ));
    }

    // Probing through a throwaway client keeps the live one's cached capability
    // tied to the solver that is actually configured, not to whatever an admin
    // typed into the box.
    let probe = kani_core::http::SmartClient::new(Some(url.clone()))
        .map_err(|e| AppError::ValidationError(format!("Invalid solver URL: {e}")))?;
    let capability = probe.solver_capability().await;

    Ok(Json(json!({
        "status": capability.as_str(),
        "insecure_transport": kani_core::http::solver_transport_is_exposed(&url),
    })))
}
