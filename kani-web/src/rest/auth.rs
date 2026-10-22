//! Authentication, session, TOTP, registration & password-reset routes.

use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(auth_login))
        .route("/auth/logout", post(auth_logout))
        .route("/auth/me", get(auth_me))
        .route("/auth/current_user", get(get_current_user))
        .route("/auth/change_password", post(change_password))
        .route("/auth/logout_everywhere", post(logout_everywhere))
        .route(
            "/auth/sessions",
            get(list_sessions).delete(revoke_all_other_sessions),
        )
        .route("/auth/sessions/{id}", delete(revoke_session))
        .route("/auth/password-strength", post(password_strength))
        .route("/auth/totp/setup", post(totp_setup))
        .route("/auth/totp/verify", post(totp_verify))
        .route("/auth/totp/disable", post(totp_disable))
        .route(
            "/auth/totp/backup-codes",
            post(totp_regenerate_backup_codes),
        )
        .route("/auth/totp/step-up", post(totp_step_up))
        .route("/features", get(get_features))
        .route("/auth/permissions", get(get_my_permissions))
        .route("/auth/registration-enabled", get(get_registration_enabled))
        .route("/auth/setup-state", get(setup_state))
        .route("/auth/setup", post(auth_setup))
        .route("/auth/captcha", get(get_captcha))
        .route("/auth/register", post(auth_register))
        .route(
            "/auth/password-reset-enabled",
            get(get_password_reset_enabled),
        )
        .route("/auth/password-reset/request", post(password_reset_request))
        .route("/auth/password-reset/confirm", post(password_reset_confirm))
        .route(
            "/auth/password-reset/validate",
            get(password_reset_validate),
        )
        .route("/auth/verify-email", post(verify_email))
        .route("/auth/resend-verification", post(resend_verification))
}

#[utoipa::path(
    post, path = "/rest/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful — sets a session cookie"),
        (status = 401, description = "Invalid credentials"),
        (status = 429, description = "Too many failed attempts"),
    ),
    tag = "auth"
)]
pub(super) async fn auth_login(
    mut auth: AuthSession,
    State(state): State<AppState>,
    client_ip: crate::client_ip::ClientIp,
    Json(form): Json<LoginRequest>,
) -> impl IntoResponse {
    let ip = client_ip.to_key();
    let username = form.username.clone();

    // The pre-flight check must not record an attempt; successful logins do not
    // contribute to the failure window.
    use crate::rate_limit::RateLimitResult;
    let pre_check = state.rate_limiter.check_lockout(&username, &ip).await;
    if let RateLimitResult::LockedOutByIdentity { retry_after_secs }
    | RateLimitResult::LockedOutByIp { retry_after_secs } = pre_check
    {
        let locked_by = match pre_check {
            RateLimitResult::LockedOutByIdentity { .. } => "identity",
            _ => "ip",
        };
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("Retry-After", retry_after_secs.to_string().as_str())],
            Json(json!({
                "error": "too_many_attempts",
                "retry_after_seconds": retry_after_secs,
                "locked_by": locked_by,
            })),
        )
            .into_response();
    }

    let creds = Credentials {
        username: form.username,
        password: form.password.into(),
    };
    match auth.authenticate(creds).await {
        Ok(Some(user)) => {
            // A successful authentication resets the identity's failure window.
            state
                .rate_limiter
                .record_and_check(&username, &ip, true)
                .await;
            match auth.login(&user).await {
                Ok(_) => {
                    if let Err(e) = auth.session.cycle_id().await {
                        tracing::warn!("Failed to cycle session ID after login: {}", e);
                    }
                    tracing::info!(user_id = user.id.0, username = %user.username, "User logged in");
                    state
                        .audit(Some(user.id), "auth.login", Some(&user.username), None)
                        .await;
                    (StatusCode::OK, Json(json!({"ok": true}))).into_response()
                }
                Err(e) => {
                    tracing::error!("Session login error: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Server error"})),
                    )
                        .into_response()
                }
            }
        }
        Ok(None) => {
            // Record the failure before deciding whether this attempt triggered lockout.
            let post_check = state
                .rate_limiter
                .record_and_check(&username, &ip, false)
                .await;
            tracing::warn!(attempted_username = %username, "Failed login attempt");
            state
                .audit(None, "auth.login.failed", Some(&username), None)
                .await;
            if let RateLimitResult::LockedOutByIdentity { retry_after_secs }
            | RateLimitResult::LockedOutByIp { retry_after_secs } = post_check
            {
                let locked_by = match post_check {
                    RateLimitResult::LockedOutByIdentity { .. } => "identity",
                    _ => "ip",
                };
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("Retry-After", retry_after_secs.to_string().as_str())],
                    Json(json!({
                        "error": "too_many_attempts",
                        "retry_after_seconds": retry_after_secs,
                        "locked_by": locked_by,
                    })),
                )
                    .into_response();
            }
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid credentials"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Auth backend error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Server error"})),
            )
                .into_response()
        }
    }
}

#[utoipa::path(
    post, path = "/rest/auth/logout",
    responses(
        (status = 200, description = "Session invalidated"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn auth_logout(
    mut auth: AuthSession,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let (user_id, username) = auth
        .user
        .as_ref()
        .map(|u| (Some(u.id), Some(u.username.clone())))
        .unwrap_or((None, None));

    if let Err(e) = auth.logout().await {
        tracing::error!("Logout error: {}", e);
    }
    state
        .audit(user_id, "auth.logout", username.as_deref(), None)
        .await;
    (StatusCode::OK, Json(json!({"ok": true})))
}

#[utoipa::path(
    get, path = "/rest/auth/me",
    responses(
        (status = 200, description = "Current user ID and username"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn auth_me(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(json!({
        "id": user.id,
        "username": user.username,
    })))
}

#[utoipa::path(
    get, path = "/rest/auth/current_user",
    responses(
        (status = 200, description = "Full current user profile including email and roles"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn get_current_user(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let verified_at =
        sqlx::query_scalar!("SELECT email_verified_at FROM users WHERE id = ?", user.id)
            .fetch_optional(&state.db)
            .await?
            .flatten()
            .map(|t: sqlx::types::time::OffsetDateTime| t.to_string());

    Ok(Json(crate::types::AuthenticatedUser {
        id: user.id.0,
        username: user.username,
        email: user.email,
        roles: user.roles,
        email_verified_at: verified_at,
    }))
}

#[utoipa::path(
    post, path = "/rest/auth/change_password",
    request_body = ChangePasswordRequest,
    responses(
        (status = 200, description = "Password changed successfully"),
        (status = 401, description = "Not authenticated or current password incorrect"),
        (status = 422, description = "New password too short"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn change_password(
    auth: AuthSession,
    State(state): State<AppState>,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    let user = auth
        .user
        .ok_or_else(|| AppError::Unauthorized("Not authenticated".into()))?;
    if body.new_password.len() < 8 {
        return Err(AppError::ValidationError(
            "New password must be at least 8 characters".into(),
        ));
    }
    let backend = AuthBackend::new(state.db.clone());
    let verified = backend
        .authenticate(Credentials {
            username: user.username.clone(),
            password: body.current_password.into(),
        })
        .await?;
    if verified.is_none() {
        return Err(AppError::PasswordError(
            "Current password is incorrect".into(),
        ));
    }
    backend.change_password(user.id, &body.new_password).await?;
    state.notify_password_changed(user.id);
    state
        .audit(
            Some(user.id),
            "auth.change_password",
            Some(&user.username),
            None,
        )
        .await;
    Ok(Json(json!({})))
}

#[utoipa::path(
    post, path = "/rest/auth/logout_everywhere",
    responses(
        (status = 200, description = "All sessions for this user invalidated"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn logout_everywhere(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let backend = AuthBackend::new(state.db.clone());
    backend.cycle_change_id(user.id).await?;
    state
        .audit(
            Some(user.id),
            "auth.logout_everywhere",
            Some(&user.username),
            None,
        )
        .await;
    Ok(Json(json!({})))
}

#[utoipa::path(
    get, path = "/rest/auth/sessions",
    responses(
        (status = 200, description = "Active sessions for the current user"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn list_sessions(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    auth: AuthSession,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let current_id = auth.session.id().map(|id| id.to_string());
    let sessions = state.list_sessions(user.id).await?;
    let response: Vec<_> = sessions
        .into_iter()
        .map(|s| {
            let is_current = current_id.as_deref() == Some(s.id.as_str());
            json!({
                "id": s.id,
                "created_at": s.created_at,
                "last_seen_at": s.last_seen_at,
                "user_agent": s.user_agent,
                "ip_addr": s.ip_addr,
                "is_current": is_current,
            })
        })
        .collect();
    Ok(Json(json!({ "sessions": response })))
}

#[utoipa::path(
    delete, path = "/rest/auth/sessions",
    responses(
        (status = 200, description = "All sessions except the current one revoked"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn revoke_all_other_sessions(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    auth: AuthSession,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let current_id = auth
        .session
        .id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let count = state.revoke_other_sessions(user.id, &current_id).await?;
    state
        .audit(
            Some(user.id),
            "auth.sessions.revoked_all_others",
            None,
            Some(json!({ "count": count })),
        )
        .await;
    Ok(Json(json!({ "revoked": count })))
}

#[utoipa::path(
    delete, path = "/rest/auth/sessions/{id}",
    params(("id" = String, Path, description = "Session ID")),
    responses(
        (status = 204, description = "Session revoked"),
        (status = 401, description = "Not authenticated"),
        (status = 404, description = "Session not found"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn revoke_session(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let revoked = state.revoke_session(&session_id, user.id).await?;
    if revoked {
        state
            .audit(
                Some(user.id),
                "auth.session.revoked",
                Some(&session_id),
                None,
            )
            .await;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("Session not found".into()))
    }
}

#[utoipa::path(
    post, path = "/rest/auth/password-strength",
    request_body = PasswordStrengthRequest,
    responses(
        (status = 200, description = "Password strength score and feedback"),
        (status = 422, description = "Password too short, too weak, same as identity, or pwned"),
    ),
    tag = "auth"
)]
pub(super) async fn password_strength(
    State(state): State<AppState>,
    Json(body): Json<PasswordStrengthRequest>,
) -> impl IntoResponse {
    use kani_app::service::password_policy::{PasswordPolicyError, check_password};
    let identity = body.identity.as_deref().unwrap_or("");
    match check_password(&body.password, identity, &state.smart_client).await {
        Ok(result) => Json(json!({
            "score": result.score,
            "feedback": result.feedback,
            "pwned": result.pwned_count.map(|c| c > 0).unwrap_or(false),
            "pwned_count": result.pwned_count,
        }))
        .into_response(),
        Err(PasswordPolicyError::TooShort) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "password_too_short" })),
        )
            .into_response(),
        Err(PasswordPolicyError::SameAsIdentity) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "password_same_as_identity" })),
        )
            .into_response(),
        Err(PasswordPolicyError::Pwned(count)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "password_pwned", "count": count })),
        )
            .into_response(),
        Err(PasswordPolicyError::TooWeak(score, msg)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "password_too_weak", "score": score, "feedback": [msg] })),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post, path = "/rest/auth/totp/setup",
    responses(
        (status = 200, description = "TOTP secret and QR code for enrollment"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn totp_setup(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let (secret, uri, qr_data_url) = state.begin_totp_setup(user.id, &user.username).await?;
    use secrecy::ExposeSecret;
    Ok(Json(json!({
        "secret": secret.expose_secret(),
        "otpauth_uri": uri,
        "qr_data_url": qr_data_url,
    })))
}

#[utoipa::path(
    post, path = "/rest/auth/totp/verify",
    request_body = TotpCodeRequest,
    responses(
        (status = 200, description = "TOTP enrolled; returns one-time backup codes"),
        (status = 401, description = "Not authenticated"),
        (status = 422, description = "Incorrect TOTP code"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn totp_verify(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
    Json(body): Json<TotpCodeRequest>,
) -> Result<impl IntoResponse, AppError> {
    let backup_codes = state.verify_totp_setup(user.id, &body.code).await?;
    state
        .audit(Some(user.id), "totp.enrolled", Some(&user.username), None)
        .await;
    Ok(Json(json!({ "backup_codes": backup_codes })))
}

#[utoipa::path(
    post, path = "/rest/auth/totp/disable",
    request_body = TotpCodeRequest,
    responses(
        (status = 204, description = "TOTP disabled"),
        (status = 401, description = "Not authenticated"),
        (status = 422, description = "Incorrect TOTP code"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn totp_disable(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
    Json(body): Json<TotpCodeRequest>,
) -> Result<impl IntoResponse, AppError> {
    state.disable_totp(user.id, &body.code).await?;
    state
        .audit(Some(user.id), "totp.disabled", Some(&user.username), None)
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/rest/auth/totp/backup-codes",
    request_body = TotpCodeRequest,
    responses(
        (status = 200, description = "New backup codes generated"),
        (status = 401, description = "Not authenticated"),
        (status = 422, description = "Incorrect TOTP code"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn totp_regenerate_backup_codes(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
    Json(body): Json<TotpCodeRequest>,
) -> Result<impl IntoResponse, AppError> {
    let codes = state.regenerate_backup_codes(user.id, &body.code).await?;
    Ok(Json(json!({ "backup_codes": codes })))
}

#[utoipa::path(
    post, path = "/rest/auth/totp/step-up",
    request_body = TotpCodeRequest,
    responses(
        (status = 204, description = "Step-up TOTP challenge satisfied"),
        (status = 401, description = "Not authenticated"),
        (status = 422, description = "Incorrect TOTP code"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn totp_step_up(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
    auth: AuthSession,
    Json(body): Json<TotpCodeRequest>,
) -> Result<impl IntoResponse, AppError> {
    let verified = state.verify_totp_code(user.id, &body.code).await?
        || state.verify_totp_backup_code(user.id, &body.code).await?;
    if !verified {
        return Err(AppError::ValidationError("Incorrect TOTP code".into()));
    }
    let _ = auth.session.insert("totp_verified", true).await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/features",
    responses(
        (status = 200, description = "Server feature flags: public_instance, totp_enabled"),
    ),
    tag = "system"
)]
pub(super) async fn get_features(
    State(state): State<AppState>,
    auth: AuthSession,
) -> impl IntoResponse {
    let totp_enabled = if let Some(user) = &auth.user {
        state.is_totp_enabled(user.id).await.unwrap_or(false)
    } else {
        false
    };
    Json(json!({
        "public_instance": state.public_instance,
        "totp_enabled": totp_enabled,
    }))
}

#[utoipa::path(
    get, path = "/rest/auth/permissions",
    responses(
        (status = 200, description = "All permissions the current user holds"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn get_my_permissions(auth: AuthSession) -> Result<impl IntoResponse, AppError> {
    let user = auth
        .user
        .ok_or_else(|| AppError::Unauthorized("Not authenticated".into()))?;
    let perms = auth
        .backend
        .get_all_permissions(&user)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(Json(perms))
}

#[utoipa::path(
    get, path = "/rest/auth/registration-enabled",
    responses(
        (status = 200, description = "Whether self-registration is enabled"),
    ),
    tag = "auth"
)]
pub(super) async fn get_registration_enabled(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let enabled = state.get_settings().await.registration_enabled;
    Ok(Json(json!({ "enabled": enabled })))
}

/// Whether this instance still needs its first account, and whether the caller
/// may create it.
///
/// The window is open only while the `users` table is empty; the first
/// successful setup closes it permanently. It is additionally restricted to
/// clients on a loopback or private address, so an instance exposed to the
/// internet before its owner reaches it cannot be claimed by a passer-by. Set
/// `KANI_ALLOW_REMOTE_SETUP=true` when the first account must be created over
/// the internet — for a VPS reached directly rather than over a tunnel.
#[utoipa::path(
    get, path = "/rest/auth/setup-state",
    responses((status = 200, description = "Whether first-run setup is available")),
    tag = "auth"
)]
pub(super) async fn setup_state(
    State(state): State<AppState>,
    PeerAddr(peer): PeerAddr,
) -> Result<impl IntoResponse, AppError> {
    let backend = crate::auth::AuthBackend::new(state.db.clone());
    let needs_setup = backend.user_count().await? == 0;
    Ok(Json(json!({
        "needs_setup": needs_setup,
        "allowed_from_here": setup_allowed_from(peer),
    })))
}

fn setup_allowed_from(peer: Option<std::net::SocketAddr>) -> bool {
    if std::env::var("KANI_ALLOW_REMOTE_SETUP")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
    {
        return true;
    }
    // `is_forbidden_ip` is the SSRF predicate — "not a public address" — which
    // is exactly the set we want to *allow* here.
    peer.map(|addr| kani_core::network::is_forbidden_ip(addr.ip()))
        .unwrap_or(false)
}

/// Creates the instance's first account and makes it the administrator.
#[utoipa::path(
    post, path = "/rest/auth/setup",
    responses(
        (status = 200, description = "First account created"),
        (status = 403, description = "Setup is closed, or not permitted from this address"),
        (status = 422, description = "Invalid username or password"),
    ),
    tag = "auth"
)]
pub(super) async fn auth_setup(
    auth: AuthSession,
    State(state): State<AppState>,
    PeerAddr(peer): PeerAddr,
    Json(body): Json<SetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    if !setup_allowed_from(peer) {
        return Err(AppError::Forbidden(
            "First-run setup must be performed from the local network. Set \
             KANI_ALLOW_REMOTE_SETUP=true to allow it from anywhere."
                .into(),
        ));
    }

    // The window is the empty users table, so this check *is* the lock: the
    // account created below closes it for good.
    if auth.backend.user_count().await? != 0 {
        return Err(AppError::Forbidden(
            "This instance has already been set up.".into(),
        ));
    }

    if body.username.trim().is_empty() || body.password.len() < 8 {
        return Err(AppError::ValidationError(
            "Username required and password must be at least 8 characters.".into(),
        ));
    }

    let user = auth
        .backend
        .create_user(&body.username, &body.email, &body.password)
        .await?;
    auth.backend.grant_role(user.id, "admin", None).await?;
    state
        .audit(Some(user.id), "auth.first_run_setup", None, None)
        .await;
    tracing::info!(username = %user.username, "First account created via first-run setup");

    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(
    get, path = "/rest/auth/captcha",
    responses(
        (status = 200, description = "Math captcha challenge: id + prompt"),
        (status = 404, description = "Registration disabled"),
    ),
    tag = "auth"
)]
pub(super) async fn get_captcha(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    if !state.get_settings().await.registration_enabled {
        return Err(AppError::NotFound("Registration is disabled".into()));
    }
    let a: i64 = rand::random::<u8>() as i64 % 10 + 1;
    let b: i64 = rand::random::<u8>() as i64 % 10 + 1;
    let answer = a + b;
    let id = uuid::Uuid::new_v4().to_string();
    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        + 300;
    sqlx::query!(
        "INSERT INTO captcha_challenges (id, answer, expires_at) VALUES (?, ?, ?)",
        id,
        answer,
        expires_at
    )
    .execute(&state.db)
    .await?;
    Ok(Json(
        json!({ "id": id, "prompt": format!("What is {} + {}?", a, b) }),
    ))
}

#[utoipa::path(
    post, path = "/rest/auth/register",
    request_body = RegisterRequest,
    responses(
        (status = 200, description = "Registered and logged in"),
        (status = 403, description = "Registration disabled"),
        (status = 422, description = "Invalid captcha, short password, or duplicate username"),
    ),
    tag = "auth"
)]
pub(super) async fn auth_register(
    auth: AuthSession,
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<impl IntoResponse, AppError> {
    if !state.get_settings().await.registration_enabled {
        return Err(AppError::NotFound("Registration is disabled".into()));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let row = sqlx::query!(
        "DELETE FROM captcha_challenges WHERE id = ? AND expires_at > ? RETURNING answer",
        body.captcha_id,
        now
    )
    .fetch_optional(&state.db)
    .await?;
    let row = row.ok_or_else(|| AppError::ValidationError("Invalid or expired captcha.".into()))?;
    if row.answer != body.captcha_answer {
        return Err(AppError::ValidationError(
            "Incorrect captcha answer.".into(),
        ));
    }
    if body.username.trim().is_empty() || body.password.len() < 8 {
        return Err(AppError::ValidationError(
            "Username required and password must be at least 8 characters.".into(),
        ));
    }
    let user = auth
        .backend
        .create_user(&body.username, &body.email, &body.password)
        .await?;
    state
        .audit(Some(user.id), "auth.register", Some(&user.username), None)
        .await;
    state.send_welcome_email(user.id);
    if state.get_settings().await.email_verification_required {
        let _ = state.send_verification_email(user.id).await;
    }
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(
    get, path = "/rest/auth/password-reset-enabled",
    responses((status = 200, description = "Whether password reset via email is enabled")),
    tag = "auth"
)]
pub(super) async fn get_password_reset_enabled(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let enabled = state.get_settings().await.password_reset_enabled;
    Ok(Json(json!({ "enabled": enabled })))
}

#[utoipa::path(
    post, path = "/rest/auth/password-reset/request",
    request_body = PasswordResetRequestBody,
    responses(
        (status = 200, description = "Reset email sent if account exists (always 200 to avoid enumeration)"),
    ),
    tag = "auth"
)]
pub(super) async fn password_reset_request(
    State(state): State<AppState>,
    Json(body): Json<PasswordResetRequestBody>,
) -> Result<impl IntoResponse, AppError> {
    state.request_password_reset(&body.email).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(
    post, path = "/rest/auth/password-reset/confirm",
    request_body = PasswordResetConfirmBody,
    responses(
        (status = 200, description = "Password changed via reset token"),
        (status = 422, description = "Invalid/expired token or weak password"),
    ),
    tag = "auth"
)]
pub(super) async fn password_reset_confirm(
    State(state): State<AppState>,
    Json(body): Json<PasswordResetConfirmBody>,
) -> Result<impl IntoResponse, AppError> {
    if body.new_password.len() < 8 {
        return Err(AppError::ValidationError(
            "Password must be at least 8 characters".into(),
        ));
    }
    // Atomically consume the token — validates, checks expiry, and marks used in one operation.
    // Password is only changed if the token was valid; no TOCTOU window.
    let user_id = state.consume_reset_token(&body.token).await?;
    let backend = AuthBackend::new(state.db.clone());
    backend.change_password(user_id, &body.new_password).await?;
    state.notify_password_changed(user_id);
    state
        .audit(Some(user_id), "auth.password_reset", None, None)
        .await;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(
    get, path = "/rest/auth/password-reset/validate",
    params(("token" = String, Query, description = "Password reset token")),
    responses(
        (status = 200, description = "Token valid; returns redacted email hint"),
        (status = 404, description = "Token not found or expired"),
    ),
    tag = "auth"
)]
pub(super) async fn password_reset_validate(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
) -> Result<impl IntoResponse, AppError> {
    let email_hint = state.reset_token_email_hint(&q.token).await?;
    Ok(Json(json!({ "email_hint": email_hint })))
}

#[utoipa::path(
    post, path = "/rest/auth/verify-email",
    request_body(content = inline(serde_json::Value), description = r#"{"token":"..."}"#),
    responses(
        (status = 200, description = "Email verified"),
        (status = 422, description = "Invalid or expired token"),
    ),
    tag = "auth"
)]
pub(super) async fn verify_email(
    State(state): State<AppState>,
    Json(body): Json<TokenQuery>,
) -> Result<impl IntoResponse, AppError> {
    state.verify_email_token(&body.token).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(
    post, path = "/rest/auth/resend-verification",
    responses(
        (status = 200, description = "Verification email resent"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "auth"
)]
pub(super) async fn resend_verification(
    AuthGuard(user, _): AuthGuard<crate::permissions::IsAuthenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    state.resend_verification_email(user.id).await?;
    Ok(Json(json!({ "ok": true })))
}
