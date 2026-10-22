//! Self-service API token management (`/rest/me/api-tokens`).

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/api-tokens", get(list_tokens).post(create_token))
        .route("/me/api-tokens/{id}", axum::routing::delete(revoke_token))
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct TokenResponse {
    id: String,
    name: String,
    kind: String,
    scopes: String,
    /// Scopes the token carries that its owner no longer holds. These are
    /// silently dropped at authentication time, so surfacing them is the only
    /// way a user can tell why their integration started returning 403.
    stale_scopes: Vec<String>,
    created_at: String,
    last_used_at: Option<String>,
    expires_at: Option<String>,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct CreatedTokenResponse {
    id: String,
    name: String,
    kind: String,
    scopes: String,
    created_at: String,
    last_used_at: Option<String>,
    expires_at: Option<String>,
    /// The raw token. Shown exactly once, at creation.
    raw_token: String,
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct CreateTokenBody {
    name: String,
    expires_in_days: Option<u32>,
    /// "opds" (default) or "api".
    #[serde(default)]
    kind: Option<String>,
    /// Required for an API token; ignored for an OPDS token, whose scopes are
    /// fixed.
    #[serde(default)]
    scopes: Vec<String>,
}

fn to_response(
    t: kani_app::service::api_tokens::ApiToken,
    held: &[kani_app::permissions::Permission],
) -> TokenResponse {
    let stale_scopes = t
        .scopes
        .split_whitespace()
        .filter(|raw| {
            raw.parse::<kani_app::permissions::Permission>()
                .map(|p| !held.contains(&p))
                .unwrap_or(true)
        })
        .map(str::to_string)
        .collect();

    TokenResponse {
        id: t.id,
        name: t.name,
        kind: t.kind,
        scopes: t.scopes,
        stale_scopes,
        created_at: rfc3339(t.created_at),
        last_used_at: t.last_used_at.map(rfc3339),
        expires_at: t.expires_at.map(rfc3339),
    }
}

/// Renders a stored Unix timestamp as RFC 3339, matching every other date the REST API emits.
/// An out-of-range value degrades to the Unix epoch rather than dropping the field.
fn rfc3339(secs: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(secs)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[utoipa::path(
    get, path = "/rest/me/api-tokens",
    responses(
        (status = 200, description = "The caller's active API tokens"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "api-tokens"
)]
pub(super) async fn list_tokens(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::Authenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let tokens = state.list_api_tokens(user.id).await?;
    let held = state.service.user_permissions(user.id).await?;
    let resp: Vec<TokenResponse> = tokens.into_iter().map(|t| to_response(t, &held)).collect();
    Ok(Json(resp))
}

#[utoipa::path(
    post, path = "/rest/me/api-tokens",
    request_body = CreateTokenBody,
    responses(
        (status = 201, description = "Token created; raw_token returned once"),
        (status = 400, description = "Invalid token name"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "api-tokens"
)]
pub(super) async fn create_token(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::Authenticated>,
    State(state): State<AppState>,
    Json(body): Json<CreateTokenBody>,
) -> Result<impl IntoResponse, AppError> {
    use kani_app::permissions::Permission;
    use kani_app::service::api_tokens::TokenKind;

    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::ValidationError(
            "Token name must be 1–100 characters".into(),
        ));
    }

    let kind = match body.kind.as_deref().unwrap_or("opds") {
        "opds" => TokenKind::Opds,
        "api" => TokenKind::Api,
        other => {
            return Err(AppError::ValidationError(format!(
                "Unknown token kind: {other}"
            )));
        }
    };

    // Each kind has its own creation permission: pairing a reader app is a much
    // lighter act than minting something that can drive the REST API.
    let required = match kind {
        TokenKind::Opds => "token:create_opds",
        TokenKind::Api => "token:create_api",
    };
    let required: Permission = required
        .parse()
        .map_err(|_| AppError::InternalServerError("bad permission literal".into()))?;
    let held = state.service.user_permissions(user.id).await?;
    if !held.contains(&required) {
        return Err(AppError::Forbidden(format!(
            "User lacks permission: {required}"
        )));
    }

    let scopes: Vec<Permission> = body.scopes.iter().filter_map(|s| s.parse().ok()).collect();
    if kind == TokenKind::Api && scopes.len() != body.scopes.len() {
        return Err(AppError::ValidationError(
            "One or more requested scopes are not valid permissions".into(),
        ));
    }

    let created = state
        .service
        .create_token(
            user.id,
            name,
            body.expires_in_days,
            kind,
            Some(scopes.as_slice()),
        )
        .await?;
    state
        .audit(
            Some(user.id),
            "api_token.created",
            Some(name),
            Some(json!({ "token_id": created.token.id })),
        )
        .await;
    let resp = CreatedTokenResponse {
        id: created.token.id,
        name: created.token.name,
        kind: created.token.kind,
        scopes: created.token.scopes,
        created_at: rfc3339(created.token.created_at),
        last_used_at: created.token.last_used_at.map(rfc3339),
        expires_at: created.token.expires_at.map(rfc3339),
        raw_token: created.raw_token,
    };
    Ok((StatusCode::CREATED, Json(resp)))
}

#[utoipa::path(
    delete, path = "/rest/me/api-tokens/{id}",
    params(("id" = String, Path, description = "API token ID")),
    responses(
        (status = 204, description = "Token revoked"),
        (status = 401, description = "Not authenticated"),
        (status = 404, description = "Token not found"),
    ),
    security(("session" = [])),
    tag = "api-tokens"
)]
pub(super) async fn revoke_token(
    AuthGuard(user, _): AuthGuard<crate::permissions::guards::Authenticated>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    state.revoke_api_token(user.id, &id).await?;
    state
        .audit(Some(user.id), "api_token.revoked", Some(&id), None)
        .await;
    Ok(StatusCode::NO_CONTENT)
}
