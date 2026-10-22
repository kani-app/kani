//! Server-persisted UI themes.
//!
//! Every route needs only an authenticated user — a user manages their own
//! themes. The extra authority, `theme:publish`, is checked *inside* the
//! handlers, because whether a request touches the instance-wide theme depends
//! on the body (`instance_wide`) or on who owns the row being changed, neither
//! of which a type-level guard can see.

use super::*;
use kani_app::service::ui_ext::{UiTheme, UpsertUiThemeBody};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/themes", get(list_themes).post(upsert_theme))
        .route("/ui/themes/deactivate", put(deactivate_theme))
        .route("/ui/themes/{id}", delete(delete_theme))
        .route("/ui/themes/{id}/activate", put(activate_theme))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub(super) struct UpsertUiThemeRequest {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub tokens: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub custom_css: Option<String>,
    /// Publish for everyone. Requires `theme:publish`.
    #[serde(default)]
    pub instance_wide: bool,
}

const THEME_NOT_YOURS: &str = "That theme belongs to someone else";

fn to_json(t: &UiTheme) -> serde_json::Value {
    json!({
        "id": t.id,
        "name": t.name,
        "tokens": t.tokens,
        "custom_css": t.custom_css,
        "is_active": t.is_active,
        "instance_wide": t.user_id.is_none(),
    })
}

/// Fails with 403 unless the caller holds `theme:publish`. Used wherever a
/// request would create, change or remove the instance-wide theme.
async fn require_publish(
    auth: &crate::auth::AuthSession,
    user: &crate::types::User,
) -> Result<(), AppError> {
    use kani_app::permissions::{AuthRequirement, guards::ThemePublish};
    let Some(perm) = ThemePublish::required_permission() else {
        return Ok(());
    };
    if auth.backend.has_perm(user, perm).await.unwrap_or(false) {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "Publishing a theme for all users requires the theme:publish permission".into(),
        ))
    }
}

#[utoipa::path(
    get, path = "/rest/ui/themes",
    responses(
        (status = 200, description = "The user's themes plus every instance-wide theme"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "ui"
)]
pub(super) async fn list_themes(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::Authenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let themes = state.service.list_ui_themes(user.id).await?;
    let active_id = themes.iter().find(|t| t.is_active).map(|t| t.id.clone());
    Ok(Json(json!({
        "themes": themes.iter().map(to_json).collect::<Vec<_>>(),
        "active_id": active_id,
    })))
}

#[utoipa::path(
    post, path = "/rest/ui/themes",
    responses(
        (status = 200, description = "Theme created or updated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "instance_wide requires theme:publish"),
        (status = 422, description = "Unknown token, bad value, or invalid name"),
    ),
    security(("session" = [])),
    tag = "ui"
)]
pub(super) async fn upsert_theme(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::Authenticated>,
    auth: crate::auth::AuthSession,
    State(state): State<AppState>,
    Json(body): Json<UpsertUiThemeRequest>,
) -> Result<impl IntoResponse, AppError> {
    let owner = if let Some(ref id) = body.id {
        match state.service.ui_theme_owner(id).await? {
            None => {
                require_publish(&auth, &user).await?;
                None
            }
            Some(uid) if uid == user.id.0 => Some(user.id),
            Some(_) => return Err(AppError::Forbidden(THEME_NOT_YOURS.into())),
        }
    } else if body.instance_wide {
        require_publish(&auth, &user).await?;
        None
    } else {
        Some(user.id)
    };

    let theme = state
        .service
        .upsert_ui_theme(
            owner,
            UpsertUiThemeBody {
                id: body.id,
                name: body.name,
                tokens: body.tokens,
                custom_css: body.custom_css,
            },
        )
        .await?;
    Ok(Json(to_json(&theme)))
}

#[utoipa::path(
    put, path = "/rest/ui/themes/{id}/activate",
    params(("id" = String, Path, description = "Theme ID")),
    responses(
        (status = 204, description = "Theme activated"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Theme belongs to another user"),
        (status = 404, description = "No such theme"),
    ),
    security(("session" = [])),
    tag = "ui"
)]
pub(super) async fn activate_theme(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::Authenticated>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    state.service.activate_ui_theme(user.id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    put, path = "/rest/ui/themes/deactivate",
    responses(
        (status = 204, description = "No theme active"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "ui"
)]
pub(super) async fn deactivate_theme(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::Authenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    state.service.deactivate_ui_theme(user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete, path = "/rest/ui/themes/{id}",
    params(("id" = String, Path, description = "Theme ID")),
    responses(
        (status = 204, description = "Theme deleted"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Not yours, or instance theme without theme:publish"),
        (status = 404, description = "No such theme"),
    ),
    security(("session" = [])),
    tag = "ui"
)]
pub(super) async fn delete_theme(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::Authenticated>,
    auth: crate::auth::AuthSession,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    // Resolve the owner first: removing the instance-wide theme affects every
    // user, so it needs the publish authority even though the route does not.
    let owner = match state.service.ui_theme_owner(&id).await? {
        None => {
            require_publish(&auth, &user).await?;
            None
        }
        Some(uid) if uid == user.id.0 => Some(user.id),
        Some(_) => return Err(AppError::Forbidden(THEME_NOT_YOURS.into())),
    };
    state.service.delete_ui_theme(owner, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
