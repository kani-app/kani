#![allow(clippy::unwrap_used)]

//! Theme ownership prevents users from restyling or deleting another user's work.

mod common;
use common::{insert_user, test_service};

use kani_app::service::ui_ext::UpsertUiThemeBody;
use std::collections::BTreeMap;

fn tokens() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("--color-accent".to_string(), "#b93a24".to_string()),
        ("--radius-md".to_string(), "0.5rem".to_string()),
    ])
}

fn body(name: &str) -> UpsertUiThemeBody {
    UpsertUiThemeBody {
        id: None,
        name: name.to_string(),
        tokens: tokens(),
        custom_css: None,
    }
}

#[tokio::test]
async fn a_theme_round_trips_through_create_and_list() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;

    let created = svc
        .upsert_ui_theme(Some(user), body("Midnight"))
        .await
        .unwrap();
    assert_eq!(created.name, "Midnight");

    let listed = svc.list_ui_themes(user).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].tokens.get("--color-accent").unwrap(), "#b93a24");
}

#[tokio::test]
async fn another_users_themes_are_not_listed() {
    let svc = test_service().await;
    let alice = insert_user(&svc.db, "alice").await;
    let bob = insert_user(&svc.db, "bob").await;

    svc.upsert_ui_theme(Some(alice), body("Alice's"))
        .await
        .unwrap();

    assert!(
        svc.list_ui_themes(bob).await.unwrap().is_empty(),
        "bob must not see alice's private theme"
    );
}

#[tokio::test]
async fn an_instance_theme_is_visible_to_every_user() {
    let svc = test_service().await;
    let alice = insert_user(&svc.db, "alice").await;
    let bob = insert_user(&svc.db, "bob").await;

    svc.upsert_ui_theme(None, body("House Style"))
        .await
        .unwrap();

    for (who, user) in [("alice", alice), ("bob", bob)] {
        let listed = svc.list_ui_themes(user).await.unwrap();
        assert_eq!(listed.len(), 1, "{who} should see the instance theme");
        assert!(listed[0].user_id.is_none());
    }
}

#[tokio::test]
async fn activating_a_theme_clears_the_previous_one() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;

    let a = svc.upsert_ui_theme(Some(user), body("A")).await.unwrap();
    let b = svc.upsert_ui_theme(Some(user), body("B")).await.unwrap();

    svc.activate_ui_theme(user, &a.id).await.unwrap();
    svc.activate_ui_theme(user, &b.id).await.unwrap();

    let active: Vec<String> = svc
        .list_ui_themes(user)
        .await
        .unwrap()
        .into_iter()
        .filter(|t| t.is_active)
        .map(|t| t.id)
        .collect();
    assert_eq!(active, vec![b.id], "exactly one theme may be active");
}

#[tokio::test]
async fn a_user_cannot_activate_another_users_private_theme() {
    let svc = test_service().await;
    let alice = insert_user(&svc.db, "alice").await;
    let bob = insert_user(&svc.db, "bob").await;

    let hers = svc
        .upsert_ui_theme(Some(alice), body("Hers"))
        .await
        .unwrap();

    assert!(
        svc.activate_ui_theme(bob, &hers.id).await.is_err(),
        "bob must not be able to activate a theme he cannot see"
    );
}

#[tokio::test]
async fn a_user_cannot_delete_another_users_theme() {
    let svc = test_service().await;
    let alice = insert_user(&svc.db, "alice").await;
    let bob = insert_user(&svc.db, "bob").await;

    let hers = svc
        .upsert_ui_theme(Some(alice), body("Hers"))
        .await
        .unwrap();

    assert!(svc.delete_ui_theme(Some(bob), &hers.id).await.is_err());
    assert_eq!(
        svc.list_ui_themes(alice).await.unwrap().len(),
        1,
        "and her theme survives the attempt"
    );
}

#[tokio::test]
async fn a_user_cannot_delete_the_instance_theme_as_their_own() {
    let svc = test_service().await;
    let alice = insert_user(&svc.db, "alice").await;
    let shared = svc.upsert_ui_theme(None, body("House")).await.unwrap();

    assert!(
        svc.delete_ui_theme(Some(alice), &shared.id).await.is_err(),
        "deleting an instance theme is not a per-user action"
    );
}

#[tokio::test]
async fn an_unknown_token_name_is_refused() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;

    let mut b = body("Bad");
    b.tokens.insert("--evil".to_string(), "#000".to_string());

    let err = svc.upsert_ui_theme(Some(user), b).await.unwrap_err();
    assert!(
        err.to_string().contains("--evil"),
        "the offending token must be named: {err}"
    );
}

#[tokio::test]
async fn a_token_value_that_escapes_its_declaration_is_refused() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;

    let mut b = body("Bad");
    b.tokens.insert(
        "--color-accent".to_string(),
        "red; } body { background: url(x)".to_string(),
    );

    assert!(svc.upsert_ui_theme(Some(user), b).await.is_err());
}

#[tokio::test]
async fn an_empty_name_is_refused() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;
    assert!(svc.upsert_ui_theme(Some(user), body("   ")).await.is_err());
}

#[tokio::test]
async fn custom_css_is_stored_already_sanitised() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;

    let mut b = body("Styled");
    b.custom_css = Some("@import url(evil.css); .btn { color: red }".to_string());
    svc.upsert_ui_theme(Some(user), b).await.unwrap();

    let stored = svc.list_ui_themes(user).await.unwrap()[0]
        .custom_css
        .clone()
        .unwrap();
    assert!(
        !stored.contains("@import") && !stored.contains("url("),
        "what is persisted must already be safe, got: {stored}"
    );
    assert!(stored.contains("color: red"), "and the safe part survives");
}

#[tokio::test]
async fn oversized_custom_css_is_refused() {
    let svc = test_service().await;
    let user = insert_user(&svc.db, "alice").await;

    let mut b = body("Huge");
    b.custom_css = Some(format!(".a {{ color: red; }}{}", " ".repeat(33 * 1024)));
    assert!(svc.upsert_ui_theme(Some(user), b).await.is_err());
}

#[tokio::test]
async fn an_update_cannot_be_retargeted_at_someone_elses_theme() {
    let svc = test_service().await;
    let alice = insert_user(&svc.db, "alice").await;
    let bob = insert_user(&svc.db, "bob").await;

    let hers = svc
        .upsert_ui_theme(Some(alice), body("Hers"))
        .await
        .unwrap();

    let mut b = body("Hijacked");
    b.id = Some(hers.id.clone());
    assert!(
        svc.upsert_ui_theme(Some(bob), b).await.is_err(),
        "supplying someone else's id must not let bob overwrite it"
    );

    assert_eq!(
        svc.list_ui_themes(alice).await.unwrap()[0].name,
        "Hers",
        "and her theme is unchanged"
    );
}
