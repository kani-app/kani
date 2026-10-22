#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_user, test_service};
use kani_app::permissions::{Opds, Permission};

#[tokio::test]
async fn create_then_authenticate_succeeds() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "alice").await;
    sqlx::query("INSERT OR IGNORE INTO user_roles (user_id, role_slug) VALUES (?, 'user')")
        .bind(user_id)
        .execute(&svc.db)
        .await
        .unwrap();

    let created = svc
        .create_token(
            user_id,
            "my reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();
    assert!(created.raw_token.starts_with("kani_"));
    assert_eq!(created.token.name, "my reader");

    let auth = svc
        .authenticate_api_token(&created.raw_token)
        .await
        .unwrap()
        .expect("token should authenticate");
    assert_eq!(auth.user_id, user_id);
    assert_eq!(
        auth.scopes,
        vec![
            Permission::Opds(Opds::Read),
            Permission::Opds(Opds::Progress)
        ]
    );
}

#[tokio::test]
async fn list_never_exposes_raw_token_or_hash() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "bob").await;

    let created = svc
        .create_token(
            user_id,
            "reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();

    let tokens = svc.list_api_tokens(user_id).await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].name, "reader");
    assert_eq!(tokens[0].id, created.token.id);
}

#[tokio::test]
async fn revoke_then_authenticate_fails() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "carol").await;

    let created = svc
        .create_token(
            user_id,
            "reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();
    svc.revoke_api_token(user_id, &created.token.id)
        .await
        .unwrap();

    let auth = svc
        .authenticate_api_token(&created.raw_token)
        .await
        .unwrap();
    assert!(auth.is_none());

    let tokens = svc.list_api_tokens(user_id).await.unwrap();
    assert!(tokens.is_empty());
}

#[tokio::test]
async fn expired_token_fails() {
    let svc = test_service().await;
    let user_id = insert_user(&svc.db, "dave").await;

    let created = svc
        .create_token(
            user_id,
            "reader",
            Some(30),
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();

    sqlx::query("UPDATE api_tokens SET expires_at = unixepoch() - 10 WHERE id = ?")
        .bind(&created.token.id)
        .execute(&svc.db)
        .await
        .unwrap();

    let auth = svc
        .authenticate_api_token(&created.raw_token)
        .await
        .unwrap();
    assert!(auth.is_none());
}

#[tokio::test]
async fn wrong_user_revoke_is_not_found() {
    let svc = test_service().await;
    let owner = insert_user(&svc.db, "erin").await;
    let other = insert_user(&svc.db, "frank").await;

    let created = svc
        .create_token(
            owner,
            "reader",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();

    let err = svc
        .revoke_api_token(other, &created.token.id)
        .await
        .unwrap_err();
    assert!(matches!(err, kani_app::error::ServiceError::NotFound(_)));

    assert!(
        svc.authenticate_api_token(&created.raw_token)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn garbage_token_returns_ok_none() {
    let svc = test_service().await;
    assert!(
        svc.authenticate_api_token("not-a-kani-token")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        svc.authenticate_api_token("kani_deadbeef")
            .await
            .unwrap()
            .is_none()
    );
}

use kani_app::service::api_tokens::TokenKind;

fn rand_suffix() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
}

async fn grant_role(pool: &sqlx::SqlitePool, user_id: i64, role: &str) {
    sqlx::query("INSERT OR IGNORE INTO user_roles (user_id, role_slug) VALUES (?, ?)")
        .bind(user_id)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
}

async fn revoke_role(pool: &sqlx::SqlitePool, user_id: i64, role: &str) {
    sqlx::query("DELETE FROM user_roles WHERE user_id = ? AND role_slug = ?")
        .bind(user_id)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn opds_tokens_keep_their_fixed_scopes_and_kind() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, &format!("u{}", rand_suffix())).await;
    grant_role(&svc.db, user.0, "user").await;

    let created = svc
        .create_token(
            user,
            "reader app",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();
    let auth = svc
        .authenticate_api_token(&created.raw_token)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(auth.kind, TokenKind::Opds, "default kind stays opds");
    assert!(auth.scopes.iter().any(|p| p.to_string() == "opds:read"));
}

#[tokio::test]
async fn an_api_token_cannot_be_granted_permissions_the_creator_lacks() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, &format!("u{}", rand_suffix())).await;
    grant_role(&svc.db, user.0, "user").await;

    let elevated: kani_app::permissions::Permission = "user:manage".parse().unwrap();
    let held = svc.user_permissions(user).await.unwrap();
    assert!(
        !held.contains(&elevated),
        "precondition: a plain user cannot manage users"
    );

    let err = svc
        .create_token(
            user,
            "over-privileged",
            None,
            TokenKind::Api,
            Some(&[elevated]),
        )
        .await;

    assert!(
        err.is_err(),
        "minting a token more capable than its creator must be refused"
    );
}

#[tokio::test]
async fn losing_a_role_after_minting_strips_the_scope_from_the_token() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, &format!("u{}", rand_suffix())).await;
    grant_role(&svc.db, user.0, "admin").await;

    let scope: kani_app::permissions::Permission = "user:manage".parse().unwrap();
    assert!(svc.user_permissions(user).await.unwrap().contains(&scope));

    let created = svc
        .create_token(user, "bot", None, TokenKind::Api, Some(&[scope]))
        .await
        .unwrap();

    let before = svc
        .authenticate_api_token(&created.raw_token)
        .await
        .unwrap()
        .unwrap();
    assert!(
        before.scopes.contains(&scope),
        "granted while the role held"
    );

    revoke_role(&svc.db, user.0, "admin").await;

    let after = svc
        .authenticate_api_token(&created.raw_token)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !after.scopes.contains(&scope),
        "a token must never outlive the permission it was granted from — \
         creation-time validation alone cannot provide this"
    );
}

#[tokio::test]
async fn token_count_and_lifetime_are_bounded() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, &format!("u{}", rand_suffix())).await;

    let too_long = svc
        .create_token(
            user,
            "forever",
            Some(10_000),
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await;
    assert!(too_long.is_err(), "lifetime cap should reject 10000 days");

    for i in 0..25 {
        svc.create_token(
            user,
            &format!("t{i}"),
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();
    }
    let over = svc
        .create_token(
            user,
            "one too many",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await;
    assert!(over.is_err(), "per-user token cap should be enforced");
}

#[tokio::test]
async fn an_opds_token_stops_working_once_its_owner_loses_the_permission() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, &format!("u{}", rand_suffix())).await;
    grant_role(&svc.db, user.0, "user").await;

    let created = svc
        .create_token(
            user,
            "kindle",
            None,
            kani_app::service::api_tokens::TokenKind::Opds,
            None,
        )
        .await
        .unwrap();
    let before = svc
        .authenticate_api_token(&created.raw_token)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !before.scopes.is_empty(),
        "precondition: the reader token works while the role holds"
    );

    revoke_role(&svc.db, user.0, "user").await;

    let after = svc
        .authenticate_api_token(&created.raw_token)
        .await
        .unwrap()
        .unwrap();
    assert!(
        after.scopes.is_empty(),
        "a reader token must not outlive its owner's access to the library"
    );
}
