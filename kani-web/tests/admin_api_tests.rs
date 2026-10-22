#![allow(clippy::unwrap_used)]

mod common;
use axum::http::StatusCode;
use common::{
    authed_delete, authed_get, authed_post, body_array, body_json, build_test_app, create_admin,
    create_regular_user, login, test_state,
};
use tower::ServiceExt;

#[tokio::test]
async fn admin_list_users_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/admin/users", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let users = body_array(res).await;
    assert!(
        !users.is_empty(),
        "at least the admin user should be listed"
    );
}

#[tokio::test]
async fn admin_list_users_returns_403_for_regular_user() {
    let state = test_state().await;
    let (username, password) = create_regular_user(&state, "alice").await;
    let app = build_test_app(state).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_get("/rest/admin/users", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let body = body_json(res).await;
    assert_eq!(body["code"], serde_json::json!("forbidden"));
}

#[tokio::test]
async fn admin_create_user_returns_201_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .clone()
        .oneshot(authed_post(
            "/rest/admin/users",
            &cookie,
            serde_json::json!({
                "username": "newuser",
                "email": "newuser@test.local",
                "password": "Password1234!",
                "roles": []
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CREATED);
    let body = body_json(res).await;
    assert_eq!(body["username"], serde_json::json!("newuser"));

    let list_res = app
        .oneshot(authed_get("/rest/admin/users", &cookie))
        .await
        .unwrap();
    let users = body_array(list_res).await;
    assert_eq!(users.len(), 2, "admin + newuser");
}

#[tokio::test]
async fn admin_create_user_returns_400_for_short_password() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/admin/users",
            &cookie,
            serde_json::json!({
                "username": "short",
                "email": "short@test.local",
                "password": "abc",
                "roles": []
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_audit_log_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/admin/audit-log", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    // Creating the admin and logging in are themselves auditable, so the log
    // must already hold entries; an empty array would mean nothing is recorded.
    let body = common::body_json(res).await;
    let entries = body
        .get("entries")
        .or_else(|| body.get("logs"))
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array())
        .expect("the audit log must come back as an array");
    assert!(
        !entries.is_empty(),
        "logging in must leave an audit entry, got {body}"
    );
}

#[tokio::test]
async fn admin_revoke_last_admin_role_returns_400() {
    let (app, cookie, _state) = common::admin_app_with_state().await;

    let users_res = app
        .clone()
        .oneshot(authed_get("/rest/admin/users", &cookie))
        .await
        .unwrap();
    let users = body_array(users_res).await;
    let admin_id = users
        .iter()
        .find(|u| u["username"] == common::ADMIN_USERNAME)
        .expect("admin user in list")["id"]
        .as_i64()
        .unwrap();

    let res = app
        .oneshot(authed_delete(
            &format!("/rest/admin/users/{admin_id}/roles/admin"),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = body_json(res).await;
    assert_eq!(body["code"], serde_json::json!("validation_error"));
}

#[tokio::test]
async fn admin_revoke_admin_role_succeeds_with_multiple_admins() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;

    let backend = kani_web::auth::AuthBackend::new(state.db.clone());
    let second = backend
        .create_user("admin2", "admin2@test.local", "Password1234!")
        .await
        .unwrap();
    backend.grant_role(second.id, "admin", None).await.unwrap();

    let app = build_test_app(state.clone()).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_delete(
            &format!("/rest/admin/users/{}/roles/admin", second.id),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let still_admin: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_roles WHERE user_id = ? AND role_slug = 'admin'",
    )
    .bind(second.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(still_admin, 0, "the admin role must be revoked");
}

#[tokio::test]
async fn admin_revoke_own_admin_role_blocked_after_second_admin_demoted() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;

    let backend = kani_web::auth::AuthBackend::new(state.db.clone());
    let second = backend
        .create_user("admin2", "admin2@test.local", "Password1234!")
        .await
        .unwrap();
    backend.grant_role(second.id, "admin", None).await.unwrap();
    sqlx::query!(
        "DELETE FROM user_roles WHERE user_id = ? AND role_slug = 'admin'",
        second.id
    )
    .execute(&state.db)
    .await
    .unwrap();

    let app = build_test_app(state.clone()).await;
    let cookie = login(&app, username, password).await;

    let users_res = app
        .clone()
        .oneshot(authed_get("/rest/admin/users", &cookie))
        .await
        .unwrap();
    let users = body_array(users_res).await;
    let admin_id = users
        .iter()
        .find(|u| u["username"] == common::ADMIN_USERNAME)
        .unwrap()["id"]
        .as_i64()
        .unwrap();

    let res = app
        .oneshot(authed_delete(
            &format!("/rest/admin/users/{admin_id}/roles/admin"),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = body_json(res).await;
    assert_eq!(body["code"], serde_json::json!("validation_error"));
}

#[tokio::test]
async fn admin_delete_second_admin_succeeds() {
    let state = test_state().await;
    let (username, password) = create_admin(&state).await;

    let backend = kani_web::auth::AuthBackend::new(state.db.clone());
    let second = backend
        .create_user("admin2", "admin2@test.local", "Password1234!")
        .await
        .unwrap();
    backend.grant_role(second.id, "admin", None).await.unwrap();

    let app = build_test_app(state.clone()).await;
    let cookie = login(&app, username, password).await;

    let res = app
        .oneshot(authed_delete(
            &format!("/rest/admin/users/{}", second.id),
            &cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE id = ?")
        .bind(second.id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(remaining, 0, "the deleted admin must be gone from users");
}

#[tokio::test]
async fn list_source_circuits_returns_200_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_get("/rest/admin/sources/circuits", &cookie))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_array(res).await;
    assert!(body.is_empty(), "fresh instance has no circuit state");
}

#[tokio::test]
async fn reset_source_circuit_returns_204_for_admin() {
    let (app, cookie) = common::admin_app().await;

    let res = app
        .oneshot(authed_post(
            "/rest/admin/sources/circuits/example.com/reset",
            &cookie,
            serde_json::json!(null),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn admin_delete_last_admin_returns_400_for_manager_user() {
    let state = test_state().await;
    let (admin_username, _admin_password) = create_admin(&state).await;

    sqlx::query!("INSERT INTO roles (slug, parent) VALUES ('manager', null)")
        .execute(&state.db)
        .await
        .unwrap();
    sqlx::query!(
        "INSERT INTO role_permissions (role_slug, permission) VALUES ('manager', 'user:manage')"
    )
    .execute(&state.db)
    .await
    .unwrap();

    let backend = kani_web::auth::AuthBackend::new(state.db.clone());
    let manager = backend
        .create_user("manager_user", "manager@test.local", "Password1234!")
        .await
        .unwrap();
    backend
        .grant_role(manager.id, "manager", None)
        .await
        .unwrap();

    let app = build_test_app(state.clone()).await;
    let manager_cookie = login(&app, "manager_user", "Password1234!").await;

    let users_res = app
        .clone()
        .oneshot(authed_get("/rest/admin/users", &manager_cookie))
        .await
        .unwrap();
    let users = body_array(users_res).await;
    let admin_id = users
        .iter()
        .find(|u| u["username"] == admin_username)
        .unwrap()["id"]
        .as_i64()
        .unwrap();

    let res = app
        .oneshot(authed_delete(
            &format!("/rest/admin/users/{admin_id}"),
            &manager_cookie,
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = body_json(res).await;
    assert_eq!(body["code"], serde_json::json!("validation_error"));
}
