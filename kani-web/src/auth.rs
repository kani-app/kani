//! Session authentication backend, credential verification, users, roles, and password hashing.

use secrecy::{ExposeSecret, Secret};
use std::collections::HashSet;
use std::sync::OnceLock;

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_login::{AuthUser, AuthnBackend, AuthzBackend, UserId as AxLoginUserId};
use kani_app::ids::UserId;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;

/// Where the first-run admin password is written.
///
/// This must remain inside the writable data directory: account creation and credential delivery
/// form one setup operation, while container home directories may be read-only.
pub(crate) fn admin_password_path() -> std::path::PathBuf {
    std::env::var("KANI_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| ".".into()))
        .join(".kani_admin_password")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// Authenticated account loaded into an `axum-login` session.
/// Secret authentication material is retained for validation and never serialized.
pub struct User {
    pub id: UserId,
    pub username: String,
    pub email: String,
    pub is_active: bool,
    pub roles: Vec<String>,
    /// RFC3339 UTC. Only populated by `list_users` (the only consumer that shows it);
    /// the session/auth paths leave it `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// Never serialised to the client.
    #[serde(skip)]
    pub password_hash: String,
    #[serde(skip)]
    pub change_id: Vec<u8>,
}

impl AuthUser for User {
    type Id = UserId;

    fn id(&self) -> UserId {
        self.id
    }

    fn session_auth_hash(&self) -> &[u8] {
        &self.change_id
    }
}

#[derive(Clone, Deserialize)]
/// Username/password credentials submitted to the authentication backend.
pub struct Credentials {
    pub username: String,
    pub password: Secret<String>,
}

/// Web session bound to Kani's SQL-backed authentication backend.
pub(crate) type AuthSession = axum_login::AuthSession<AuthBackend>;

#[derive(Clone)]
/// SQL-backed `axum-login` authentication and role-loading backend.
pub struct AuthBackend {
    db: SqlitePool,
}

impl AuthBackend {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Fetch a user row by ID.
    pub(crate) async fn fetch_user_by_id(&self, id: UserId) -> Result<Option<User>, AppError> {
        struct Row {
            id: i64,
            username: String,
            email: String,
            password_hash: String,
            change_id: Vec<u8>,
            is_active: bool,
        }

        let Some(row) = sqlx::query_as!(
            Row,
            r#"SELECT id, username, email, password_hash,
                      change_id as "change_id: Vec<u8>",
                      is_active as "is_active: bool"
               FROM users WHERE id = ?"#,
            id
        )
        .fetch_optional(&self.db)
        .await?
        else {
            return Ok(None);
        };

        let roles = self.fetch_roles(row.id).await?;

        Ok(Some(User {
            id: UserId(row.id),
            username: row.username,
            email: row.email,
            password_hash: row.password_hash,
            change_id: row.change_id,
            is_active: row.is_active,
            created_at: None,
            roles,
        }))
    }

    /// Fetch a user row by username OR email (whichever matches).
    async fn fetch_user_by_identity(&self, identity: &str) -> Result<Option<User>, AppError> {
        struct Row {
            id: i64,
            username: String,
            email: String,
            password_hash: String,
            change_id: Vec<u8>,
            is_active: bool,
        }

        let Some(row) = sqlx::query_as!(
            Row,
            r#"SELECT id, username, email, password_hash,
                      change_id as "change_id: Vec<u8>",
                      is_active as "is_active: bool"
               FROM users
               WHERE username = ? OR email = ?
               LIMIT 1"#,
            identity,
            identity
        )
        .fetch_optional(&self.db)
        .await?
        else {
            return Ok(None);
        };

        let roles = self.fetch_roles(row.id).await?;

        Ok(Some(User {
            id: UserId(row.id),
            username: row.username,
            email: row.email,
            password_hash: row.password_hash,
            change_id: row.change_id,
            is_active: row.is_active,
            created_at: None,
            roles,
        }))
    }

    /// Load the role slugs for `user_id`.
    async fn fetch_roles(&self, user_id: i64) -> Result<Vec<String>, AppError> {
        let roles = sqlx::query_scalar!(
            "SELECT role_slug FROM user_roles WHERE user_id = ? ORDER BY role_slug",
            user_id
        )
        .fetch_all(&self.db)
        .await?;
        Ok(roles)
    }

    pub async fn user_count(&self) -> Result<i64, AppError> {
        Ok(sqlx::query_scalar!("SELECT COUNT(*) FROM users")
            .fetch_one(&self.db)
            .await?)
    }

    /// Creates a new user, hashes their password, and assigns the `user` role.
    pub async fn create_user(
        &self,
        username: &str,
        email: &str,
        password: &str,
    ) -> Result<User, AppError> {
        let password_hash = hash_password(password)?;
        let change_id = fresh_change_id();

        let id = sqlx::query_scalar!(
            "INSERT INTO users (username, email, password_hash, change_id)
             VALUES (?, ?, ?, ?)
             RETURNING id",
            username,
            email,
            password_hash,
            change_id,
        )
        .fetch_one(&self.db)
        .await?;

        sqlx::query!(
            "INSERT OR IGNORE INTO user_roles (user_id, role_slug) VALUES (?, 'user')",
            id
        )
        .execute(&self.db)
        .await?;

        self.fetch_user_by_id(UserId(id))
            .await?
            .ok_or_else(|| AppError::SqlxError(sqlx::Error::RowNotFound))
    }

    /// Changes a user's password and simultaneously rotates `change_id`,
    /// invalidating all existing sessions.
    pub(crate) async fn change_password(
        &self,
        user_id: UserId,
        new_password: &str,
    ) -> Result<(), AppError> {
        let path = admin_password_path();

        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("Failed to delete temporary admin password file: {}", e);
        }

        let hash = hash_password(new_password)?;
        let change_id = fresh_change_id();
        sqlx::query!(
            "UPDATE users SET password_hash = ?, change_id = ? WHERE id = ?",
            hash,
            change_id,
            user_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Rotates `change_id` invalidating all active sessions.
    pub(crate) async fn cycle_change_id(&self, user_id: UserId) -> Result<(), AppError> {
        let change_id = fresh_change_id();
        sqlx::query!(
            "UPDATE users SET change_id = ? WHERE id = ?",
            change_id,
            user_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Activates or deactivates a user account.
    /// Deactivating also rotates `change_id` to terminate live sessions.
    pub(crate) async fn set_active(&self, user_id: UserId, active: bool) -> Result<(), AppError> {
        if !active {
            self.cycle_change_id(user_id).await?;
        }
        sqlx::query!(
            "UPDATE users SET is_active = ? WHERE id = ?",
            active,
            user_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Grants a role to a user.  `granted_by` is the admin's user id.
    pub async fn grant_role(
        &self,
        user_id: UserId,
        role_slug: &str,
        granted_by: Option<UserId>,
    ) -> Result<(), AppError> {
        sqlx::query!(
            "INSERT OR IGNORE INTO user_roles (user_id, role_slug, granted_by)
             VALUES (?, ?, ?)",
            user_id,
            role_slug,
            granted_by
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    pub(crate) async fn count_users_with_role(&self, role_slug: &str) -> Result<i64, AppError> {
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM user_roles WHERE role_slug = ?",
            role_slug
        )
        .fetch_one(&self.db)
        .await?;
        Ok(count)
    }

    /// Revokes a role from a user.
    pub async fn revoke_role(&self, user_id: UserId, role_slug: &str) -> Result<(), AppError> {
        sqlx::query!(
            "DELETE FROM user_roles WHERE user_id = ? AND role_slug = ?",
            user_id,
            role_slug
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Lists all users. Single query via GROUP_CONCAT — avoids N+1.
    pub(crate) async fn list_users(&self) -> Result<Vec<User>, AppError> {
        struct Row {
            id: i64,
            username: String,
            email: String,
            password_hash: String,
            change_id: Vec<u8>,
            is_active: bool,
            created_at: Option<Option<String>>,
            roles_csv: Option<Option<String>>,
        }

        let rows = sqlx::query_as!(
            Row,
            r#"SELECT u.id,
                      u.username,
                      u.email,
                      u.password_hash,
                      u.change_id    as "change_id: Vec<u8>",
                      u.is_active    as "is_active: bool",
                      strftime('%Y-%m-%dT%H:%M:%SZ', u.created_at) as "created_at: Option<String>",
                      GROUP_CONCAT(ur.role_slug) as "roles_csv: Option<String>"
               FROM users u
               LEFT JOIN user_roles ur ON ur.user_id = u.id
               GROUP BY u.id
               ORDER BY u.id"#
        )
        .fetch_all(&self.db)
        .await?;

        let users = rows
            .into_iter()
            .map(|row| {
                let roles = row
                    .roles_csv
                    .flatten()
                    .map(|csv| csv.split(',').map(str::to_string).collect())
                    .unwrap_or_default();
                User {
                    id: UserId(row.id),
                    username: row.username,
                    email: row.email,
                    password_hash: row.password_hash,
                    change_id: row.change_id,
                    is_active: row.is_active,
                    created_at: row.created_at.flatten(),
                    roles,
                }
            })
            .collect();

        Ok(users)
    }

    /// Updates a user's username and/or email.
    pub(crate) async fn update_user(
        &self,
        user_id: UserId,
        username: Option<&str>,
        email: Option<&str>,
    ) -> Result<(), AppError> {
        if let Some(un) = username {
            sqlx::query!("UPDATE users SET username = ? WHERE id = ?", un, user_id)
                .execute(&self.db)
                .await?;
        }
        if let Some(em) = email {
            sqlx::query!("UPDATE users SET email = ? WHERE id = ?", em, user_id)
                .execute(&self.db)
                .await?;
        }
        Ok(())
    }

    /// Deletes a user account and all associated data.
    pub(crate) async fn delete_user(&self, user_id: UserId) -> Result<(), AppError> {
        sqlx::query!("DELETE FROM users WHERE id = ?", user_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    /// Resets a user's password (admin action — no current-password check).
    pub(crate) async fn admin_reset_password(
        &self,
        user_id: UserId,
        new_password: &str,
    ) -> Result<(), AppError> {
        let hash =
            crate::auth::hash_password(new_password).map_err(|e| AppError::Other(e.to_string()))?;
        let change_id = fresh_change_id();
        sqlx::query!(
            "UPDATE users SET password_hash = ?, change_id = ? WHERE id = ?",
            hash,
            change_id,
            user_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Lists all roles with their permissions.
    pub(crate) async fn list_roles(&self) -> Result<Vec<RoleWithPermissions>, AppError> {
        #[derive(sqlx::FromRow)]
        struct RoleRow {
            slug: String,
            parent: Option<String>,
            description: Option<String>,
        }

        let role_rows = sqlx::query_as::<_, RoleRow>(
            "SELECT slug, parent, description FROM roles ORDER BY slug",
        )
        .fetch_all(&self.db)
        .await?;

        let perm_rows = sqlx::query!(
            "SELECT role_slug, permission FROM role_permissions ORDER BY role_slug, permission"
        )
        .fetch_all(&self.db)
        .await?;

        let roles = role_rows
            .into_iter()
            .map(|r| {
                let permissions = perm_rows
                    .iter()
                    .filter(|p| p.role_slug == r.slug)
                    .map(|p| p.permission.clone())
                    .collect();
                RoleWithPermissions {
                    slug: r.slug,
                    parent: r.parent,
                    description: r.description,
                    permissions,
                }
            })
            .collect();

        Ok(roles)
    }

    /// Creates a new role with the given slug, optional parent, description, and permissions.
    pub(crate) async fn create_role(
        &self,
        slug: &str,
        parent: Option<&str>,
        description: Option<&str>,
        permissions: &[String],
    ) -> Result<(), AppError> {
        sqlx::query!(
            "INSERT INTO roles (slug, parent, description) VALUES (?, ?, ?)",
            slug,
            parent,
            description
        )
        .execute(&self.db)
        .await?;
        for perm in permissions {
            sqlx::query!(
                "INSERT OR IGNORE INTO role_permissions (role_slug, permission) VALUES (?, ?)",
                slug,
                perm
            )
            .execute(&self.db)
            .await?;
        }
        Ok(())
    }

    /// Updates a role's description and replaces its permission set.
    /// The `user` and `admin` role slugs cannot be modified.
    pub(crate) async fn update_role(
        &self,
        slug: &str,
        description: Option<&str>,
        permissions: &[String],
    ) -> Result<(), AppError> {
        if let Some(desc) = description {
            sqlx::query!(
                "UPDATE roles SET description = ? WHERE slug = ?",
                desc,
                slug
            )
            .execute(&self.db)
            .await?;
        }
        sqlx::query!("DELETE FROM role_permissions WHERE role_slug = ?", slug)
            .execute(&self.db)
            .await?;
        for perm in permissions {
            sqlx::query!(
                "INSERT OR IGNORE INTO role_permissions (role_slug, permission) VALUES (?, ?)",
                slug,
                perm
            )
            .execute(&self.db)
            .await?;
        }
        Ok(())
    }

    /// Deletes a role. Returns an error for the protected `user` and `admin` roles.
    pub(crate) async fn delete_role(&self, slug: &str) -> Result<(), AppError> {
        if slug == "user" || slug == "admin" {
            return Err(AppError::Forbidden(format!(
                "The '{slug}' role cannot be deleted"
            )));
        }
        sqlx::query!("DELETE FROM roles WHERE slug = ?", slug)
            .execute(&self.db)
            .await?;
        Ok(())
    }
}

/// A role with its direct (non-inherited) permission list.
#[derive(serde::Serialize)]
pub(crate) struct RoleWithPermissions {
    pub slug: String,
    pub parent: Option<String>,
    pub description: Option<String>,
    pub permissions: Vec<String>,
}

/// A precomputed dummy hash used only to consume argon2 work when a user is not found or is
/// inactive. Computed once at startup; the result is never compared to a real password.
static DUMMY_HASH: OnceLock<String> = OnceLock::new();

fn dummy_hash() -> &'static str {
    DUMMY_HASH.get_or_init(|| {
        hash_password("kani-dummy-timing-placeholder").expect("argon2 dummy hash failed at startup")
    })
}

impl AuthnBackend for AuthBackend {
    type User = User;
    type Credentials = Credentials;
    type Error = AppError;

    async fn authenticate(
        &self,
        creds: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        match self.fetch_user_by_identity(&creds.username).await? {
            None => {
                // Consume argon2 work to prevent timing-based user enumeration.
                let _ = verify_password(creds.password.expose_secret().as_str(), dummy_hash());
                Ok(None)
            }
            Some(user) if !user.is_active => {
                // A found-but-inactive account must take the same time as an active one
                // to prevent distinguishing active from inactive accounts.
                let _ = verify_password(creds.password.expose_secret().as_str(), dummy_hash());
                Ok(None)
            }
            Some(user) => {
                if !verify_password(creds.password.expose_secret().as_str(), &user.password_hash)? {
                    return Ok(None);
                }

                let _ = sqlx::query!(
                    "UPDATE users SET last_login = CURRENT_TIMESTAMP WHERE id = ?",
                    user.id
                )
                .execute(&self.db)
                .await;

                Ok(Some(user))
            }
        }
    }

    async fn get_user(
        &self,
        user_id: &AxLoginUserId<Self>,
    ) -> Result<Option<Self::User>, Self::Error> {
        self.fetch_user_by_id(*user_id).await
    }
}

impl User {
    /// Returns `true` if the user holds `role_slug` **or any ancestor role**.
    pub fn has_role(&self, role_slug: &str) -> bool {
        self.roles.iter().any(|r| r == role_slug)
    }

    /// Convenience: returns `true` if the user holds the `admin` role.
    pub fn is_admin(&self) -> bool {
        self.has_role("admin")
    }
}

/// Axum middleware that enforces authentication on all non-public routes.
pub async fn auth_guard(auth: AuthSession, request: Request, next: Next) -> Response {
    let path = request.uri().path();

    if is_public_path(path) {
        return next.run(request).await;
    }

    if request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Bearer "))
    {
        return next.run(request).await;
    }

    match &auth.user {
        None => {
            if path.starts_with("/rest/") || path.starts_with("/api/") {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "Authentication required" })),
                )
                    .into_response();
            }
            Redirect::to("/login").into_response()
        }
        Some(user) if !user.is_active => {
            if path.starts_with("/rest/") || path.starts_with("/api/") {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({ "error": "Account is inactive" })),
                )
                    .into_response();
            }
            Redirect::to("/login").into_response()
        }
        Some(_) => next.run(request).await,
    }
}

/// Returns `true` for paths that are always accessible without a session.
fn is_public_path(path: &str) -> bool {
    path == "/login"
        || path == "/setup"
        || path == "/register"
        || path == "/forgot-password"
        || path == "/reset-password"
        || path == "/verify-email"
        || path.starts_with("/rest/auth/")
        || path.starts_with("/js/")
        || path.starts_with("/css/")
        || path.starts_with("/locales/")
        || path.starts_with("/fonts/")
        || path == "/favicon.ico"
        || path == "/health"
        || path == "/healthz"
        || path == "/ready"
        || path == "/readyz"
        || path == "/metrics"
        || path == "/manifest.webmanifest"
        || path == "/sw.js"
        || path.starts_with("/icons/")
        || path.starts_with("/opds")
        || path == "/rest/system/info"
        || path == "/changelog.md"
        || (cfg!(debug_assertions) && path.starts_with("/api-docs"))
}

/// Hashes a plaintext password using Argon2id.
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

/// Verifies a plaintext password against a stored Argon2 hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, argon2::password_hash::Error> {
    let parsed = PasswordHash::new(hash)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// Generates 16 fresh random bytes for use as `change_id`.
pub(crate) fn fresh_change_id() -> Vec<u8> {
    use argon2::password_hash::rand_core::{OsRng, RngCore};
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes.to_vec()
}

impl AuthzBackend for AuthBackend {
    type Permission = crate::permissions::Permission;

    async fn get_user_permissions(
        &self,
        _user: &Self::User,
    ) -> Result<HashSet<Self::Permission>, Self::Error> {
        Ok(HashSet::new())
    }

    async fn get_group_permissions(
        &self,
        user: &Self::User,
    ) -> Result<HashSet<Self::Permission>, Self::Error> {
        if user.roles.is_empty() {
            return Ok(HashSet::new());
        }

        let roles_json = serde_json::to_string(&user.roles).unwrap_or_default();
        let rows = sqlx::query_scalar!(
            "WITH RECURSIVE role_tree(slug) AS (
                SELECT slug FROM roles WHERE slug IN (SELECT value FROM json_each(?))
                UNION
                SELECT r.parent FROM roles r JOIN role_tree rt ON r.slug = rt.slug
                WHERE r.parent IS NOT NULL
            )
            SELECT DISTINCT rp.permission
            FROM role_permissions rp
            JOIN role_tree rt ON rp.role_slug = rt.slug",
            roles_json
        )
        .fetch_all(&self.db)
        .await?;

        Ok(rows.into_iter().filter_map(|s| s.parse().ok()).collect())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) async fn test_db() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("../migrations").run(&pool).await.unwrap();
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use axum_login::AuthzBackend;

    #[test]
    fn the_admin_password_is_written_to_the_data_dir_not_the_home_dir() {
        unsafe { std::env::set_var("KANI_DATA_DIR", "/tmp/kani-data-dir-test") };
        let path = admin_password_path();
        unsafe { std::env::remove_var("KANI_DATA_DIR") };

        assert_eq!(
            path,
            std::path::Path::new("/tmp/kani-data-dir-test/.kani_admin_password"),
            "the password file must sit in the data dir, which is a writable volume"
        );
    }

    #[test]
    fn hash_password_produces_argon2_string() {
        let hash = hash_password("hunter2").unwrap();
        assert!(hash.starts_with("$argon2id$"));
    }

    #[test]
    fn verify_password_correct_returns_true() {
        let hash = hash_password("correct-horse").unwrap();
        assert!(verify_password("correct-horse", &hash).unwrap());
    }

    #[test]
    fn verify_password_wrong_returns_false() {
        let hash = hash_password("correct-horse").unwrap();
        assert!(!verify_password("battery-staple", &hash).unwrap());
    }

    #[test]
    fn different_salts_per_hash() {
        let h1 = hash_password("same").unwrap();
        let h2 = hash_password("same").unwrap();
        assert_ne!(h1, h2, "Argon2 should use different salts each time");
    }

    #[test]
    fn login_path_is_public() {
        assert!(is_public_path("/login"));
    }

    #[test]
    fn auth_pages_are_public() {
        assert!(is_public_path("/register"));
        assert!(
            is_public_path("/setup"),
            "first-run setup is reached before any account exists, so it cannot require a session"
        );
        assert!(is_public_path("/forgot-password"));
        assert!(is_public_path("/reset-password"));
        assert!(is_public_path("/verify-email"));
    }

    #[test]
    fn rest_auth_is_public() {
        assert!(is_public_path("/rest/auth/login"));
        assert!(is_public_path("/rest/auth/logout"));
    }

    #[test]
    fn static_assets_are_public() {
        assert!(is_public_path("/js/app.js"));
        assert!(is_public_path("/js/vendor/preact.module.js"));
        assert!(is_public_path("/css/main.css"));
        assert!(is_public_path("/locales/en.js"));
        assert!(is_public_path("/fonts/fonts.css"));
        assert!(is_public_path(
            "/fonts/aFTT7PB1QTsUX8KYth-orYadYY35Zlk.woff2"
        ));
    }

    #[test]
    fn favicon_is_public() {
        assert!(is_public_path("/favicon.ico"));
    }

    #[test]
    fn health_is_public() {
        assert!(is_public_path("/health"));
        assert!(is_public_path("/ready"));
    }

    #[test]
    fn metrics_is_public_for_scrapers() {
        assert!(is_public_path("/metrics"));
    }

    #[test]
    fn healthz_aliases_are_public() {
        assert!(is_public_path("/healthz"));
        assert!(is_public_path("/readyz"));
    }

    #[test]
    fn system_info_is_public() {
        assert!(is_public_path("/rest/system/info"));
        assert!(!is_public_path("/rest/system/first-run-complete"));
    }

    #[test]
    fn api_docs_is_public() {
        assert!(is_public_path("/api-docs"));
        assert!(is_public_path("/api-docs/openapi.json"));
    }

    #[test]
    fn rest_sources_is_not_public() {
        assert!(!is_public_path("/rest/sources"));
    }

    #[test]
    fn settings_page_is_not_public() {
        assert!(!is_public_path("/settings"));
    }

    #[tokio::test]
    async fn create_user_inserts_and_assigns_user_role() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let user = backend
            .create_user("alice", "alice@test.com", "pass123")
            .await
            .unwrap();
        assert_eq!(user.username, "alice");
        assert!(user.has_role("user"));
    }

    #[tokio::test]
    async fn create_user_duplicate_username_errors() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        backend
            .create_user("bob", "bob@test.com", "pass1")
            .await
            .unwrap();
        let result = backend.create_user("bob", "bob2@test.com", "pass2").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fetch_user_by_identity_finds_by_username() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        backend
            .create_user("carol", "carol@test.com", "pass")
            .await
            .unwrap();
        let found = backend.fetch_user_by_identity("carol").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().username, "carol");
    }

    #[tokio::test]
    async fn fetch_user_by_identity_returns_none_for_unknown() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let found = backend.fetch_user_by_identity("nobody").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn list_users_returns_all() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        backend.create_user("u1", "u1@t.com", "p").await.unwrap();
        backend.create_user("u2", "u2@t.com", "p").await.unwrap();
        let users = backend.list_users().await.unwrap();
        assert_eq!(users.len(), 2);
    }

    #[tokio::test]
    async fn user_count_is_correct() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        assert_eq!(backend.user_count().await.unwrap(), 0);
        backend.create_user("x", "x@t.com", "p").await.unwrap();
        assert_eq!(backend.user_count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn authenticate_succeeds_with_valid_credentials() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        backend
            .create_user("dave", "dave@test.com", "secret")
            .await
            .unwrap();
        let result = backend
            .authenticate(Credentials {
                username: "dave".into(),
                password: Secret::new("secret".into()),
            })
            .await
            .unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn authenticate_fails_wrong_password() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        backend
            .create_user("eve", "eve@test.com", "right")
            .await
            .unwrap();
        let result = backend
            .authenticate(Credentials {
                username: "eve".into(),
                password: Secret::new("wrong".into()),
            })
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn authenticate_fails_nonexistent_user() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let result = backend
            .authenticate(Credentials {
                username: "ghost".into(),
                password: Secret::new("pass".into()),
            })
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn authenticate_fails_inactive_user() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let user = backend
            .create_user("frank", "frank@test.com", "pass")
            .await
            .unwrap();
        backend.set_active(user.id, false).await.unwrap();
        let result = backend
            .authenticate(Credentials {
                username: "frank".into(),
                password: Secret::new("pass".into()),
            })
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn dummy_hash_is_stable_across_calls() {
        let h1 = dummy_hash();
        let h2 = dummy_hash();
        assert_eq!(
            h1, h2,
            "dummy_hash() must return the same value every call (OnceLock)"
        );
        assert!(
            h1.starts_with("$argon2"),
            "must be a valid argon2 hash string"
        );
    }

    #[tokio::test]
    #[ignore = "timing test — run locally: cargo test -- --ignored"]
    async fn unknown_user_and_known_user_take_comparable_time() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        backend
            .create_user("timing_user", "t@t.com", "correct_password_42")
            .await
            .unwrap();

        let start_known = std::time::Instant::now();
        let _ = backend
            .authenticate(Credentials {
                username: "timing_user".into(),
                password: Secret::new("wrong".into()),
            })
            .await;
        let elapsed_known = start_known.elapsed();

        let start_unknown = std::time::Instant::now();
        let _ = backend
            .authenticate(Credentials {
                username: "nobody_here".into(),
                password: Secret::new("wrong".into()),
            })
            .await;
        let elapsed_unknown = start_unknown.elapsed();

        let ratio =
            elapsed_known.as_millis().max(1) as f64 / elapsed_unknown.as_millis().max(1) as f64;
        assert!(
            ratio < 10.0 && ratio > 0.1,
            "timing ratio {ratio:.1}× is too large — unknown-user path must run dummy argon2 work"
        );
    }

    #[tokio::test]
    async fn grant_role_adds_role() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let user = backend
            .create_user("grace", "grace@test.com", "pass")
            .await
            .unwrap();
        backend.grant_role(user.id, "admin", None).await.unwrap();
        let updated = backend
            .fetch_user_by_identity("grace")
            .await
            .unwrap()
            .unwrap();
        assert!(updated.has_role("admin"));
    }

    #[tokio::test]
    async fn grant_role_is_idempotent() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let user = backend
            .create_user("hank", "hank@test.com", "pass")
            .await
            .unwrap();
        backend.grant_role(user.id, "admin", None).await.unwrap();
        backend.grant_role(user.id, "admin", None).await.unwrap();
        let updated = backend
            .fetch_user_by_identity("hank")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.roles.iter().filter(|r| *r == "admin").count(), 1);
    }

    #[tokio::test]
    async fn revoke_role_removes_role() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let user = backend
            .create_user("ivy", "ivy@test.com", "pass")
            .await
            .unwrap();
        backend.grant_role(user.id, "admin", None).await.unwrap();
        backend.revoke_role(user.id, "admin").await.unwrap();
        let updated = backend
            .fetch_user_by_identity("ivy")
            .await
            .unwrap()
            .unwrap();
        assert!(!updated.has_role("admin"));
    }

    #[tokio::test]
    async fn is_admin_true_for_admin_role() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let user = backend
            .create_user("jake", "jake@test.com", "pass")
            .await
            .unwrap();
        assert!(!user.is_admin());
        backend.grant_role(user.id, "admin", None).await.unwrap();
        let updated = backend
            .fetch_user_by_identity("jake")
            .await
            .unwrap()
            .unwrap();
        assert!(updated.is_admin());
    }

    #[tokio::test]
    async fn user_role_gets_expected_permissions() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let user = backend
            .create_user("kate", "kate@test.com", "pass")
            .await
            .unwrap();
        let perms = backend.get_group_permissions(&user).await.unwrap();
        assert!(perms.contains(&"library:view".parse().unwrap()));
        assert!(perms.contains(&"source:browse".parse().unwrap()));
        assert!(!perms.contains(&"source:install".parse().unwrap()));
        assert!(!perms.contains(&"user:manage".parse().unwrap()));
    }

    #[tokio::test]
    async fn admin_role_inherits_user_permissions() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let user = backend
            .create_user("leo", "leo@test.com", "pass")
            .await
            .unwrap();
        backend.grant_role(user.id, "admin", None).await.unwrap();
        let updated = backend
            .fetch_user_by_identity("leo")
            .await
            .unwrap()
            .unwrap();
        let perms = backend.get_group_permissions(&updated).await.unwrap();
        assert!(perms.contains(&"library:view".parse().unwrap()));
        assert!(perms.contains(&"source:install".parse().unwrap()));
        assert!(perms.contains(&"user:manage".parse().unwrap()));
    }

    #[tokio::test]
    async fn empty_roles_returns_empty_permissions() {
        let db = test_db().await;
        let backend = AuthBackend::new(db);
        let user = backend
            .create_user("mia", "mia@test.com", "pass")
            .await
            .unwrap();
        backend.revoke_role(user.id, "user").await.unwrap();
        let updated = backend
            .fetch_user_by_identity("mia")
            .await
            .unwrap()
            .unwrap();
        let perms = backend.get_group_permissions(&updated).await.unwrap();
        assert!(perms.is_empty());
    }
}
