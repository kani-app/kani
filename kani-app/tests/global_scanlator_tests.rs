#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_chapter, insert_manga, insert_source, test_service};

#[tokio::test]
async fn a_global_default_applies_where_no_per_manga_rule_exists() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "s").await;
    let manga = insert_manga(&svc.db, src, "m", "M").await;

    svc.set_global_scanlator_pref("Good", 10, false)
        .await
        .unwrap();
    svc.set_global_scanlator_pref("Bad", 0, true).await.unwrap();

    let eff = svc.effective_scanlator_prefs(manga).await.unwrap();
    assert_eq!(eff.len(), 2);
    assert_eq!(eff[0].scanlator, "Good");
    assert!(eff.iter().all(|p| p.manga_id.is_none()));
}

#[tokio::test]
async fn a_per_manga_rule_overrides_the_global_one() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "s").await;
    let manga = insert_manga(&svc.db, src, "m", "M").await;

    svc.set_global_scanlator_pref("Group", 10, false)
        .await
        .unwrap();
    svc.set_scanlator_pref(manga, "Group", 1, true)
        .await
        .unwrap();

    let eff = svc.effective_scanlator_prefs(manga).await.unwrap();
    assert_eq!(
        eff.len(),
        1,
        "the global must not appear alongside its override"
    );
    assert_eq!(eff[0].manga_id, Some(manga.0));
    assert!(
        eff[0].blocked,
        "the per-manga block wins over a global preference"
    );
}

#[tokio::test]
async fn globals_and_per_manga_rules_coexist_for_different_groups() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "s").await;
    let manga = insert_manga(&svc.db, src, "m", "M").await;

    svc.set_global_scanlator_pref("Global", 5, false)
        .await
        .unwrap();
    svc.set_scanlator_pref(manga, "Local", 9, false)
        .await
        .unwrap();

    let eff = svc.effective_scanlator_prefs(manga).await.unwrap();
    assert_eq!(eff.len(), 2);
    assert_eq!(eff[0].scanlator, "Local", "ordered by priority");
    assert_eq!(eff[1].scanlator, "Global");
}

#[tokio::test]
async fn setting_the_same_global_twice_updates_rather_than_duplicating() {
    let svc = test_service().await;
    svc.set_global_scanlator_pref("Group", 1, false)
        .await
        .unwrap();
    svc.set_global_scanlator_pref("Group", 8, true)
        .await
        .unwrap();

    let globals = svc.get_global_scanlator_prefs().await.unwrap();
    assert_eq!(
        globals.len(),
        1,
        "SQLite treats NULLs as distinct, so a plain ON CONFLICT would have \
         inserted a second row here"
    );
    assert_eq!(globals[0].priority, 8);
    assert!(globals[0].blocked);
}

#[tokio::test]
async fn an_empty_global_name_is_rejected() {
    let svc = test_service().await;
    assert!(
        svc.set_global_scanlator_pref("   ", 1, false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_global_default_reaches_upgrade_detection() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "s").await;
    let manga = insert_manga(&svc.db, src, "m", "M").await;

    let held = insert_chapter(&svc.db, manga, "held", 1.0).await;
    sqlx::query(
        "UPDATE chapters SET download_status = 2, scanlator = 'Low', page_count = 3 WHERE id = ?",
    )
    .bind(held)
    .execute(&svc.db)
    .await
    .unwrap();
    let rival = insert_chapter(&svc.db, manga, "rival", 1.0).await;
    sqlx::query("UPDATE chapters SET scanlator = 'High', page_count = 3 WHERE id = ?")
        .bind(rival)
        .execute(&svc.db)
        .await
        .unwrap();

    svc.set_global_scanlator_pref("Low", 1, false)
        .await
        .unwrap();
    svc.set_global_scanlator_pref("High", 10, false)
        .await
        .unwrap();

    let found = svc.evaluate_upgrades(manga).await.unwrap();
    assert!(
        found
            .iter()
            .any(|c| c.candidate_scanlator.as_deref() == Some("High")),
        "a library-wide preference that upgrade detection ignores is a \
         preference in name only"
    );
}

#[tokio::test]
async fn scanlators_by_usage_ranks_by_how_much_you_hold() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "s").await;
    let manga = insert_manga(&svc.db, src, "m", "M").await;

    for (i, group) in ["A", "A", "A", "B"].iter().enumerate() {
        let c = insert_chapter(&svc.db, manga, &format!("c{i}"), i as f64).await;
        sqlx::query("UPDATE chapters SET scanlator = ? WHERE id = ?")
            .bind(group)
            .bind(c)
            .execute(&svc.db)
            .await
            .unwrap();
    }

    let ranked = svc.scanlators_by_usage().await.unwrap();
    assert_eq!(ranked[0], ("A".to_string(), 3));
    assert_eq!(ranked[1], ("B".to_string(), 1));
}

async fn set_scanlator(
    svc: &kani_app::service::AppService,
    chapter: kani_app::ids::ChapterId,
    name: &str,
) {
    sqlx::query("UPDATE chapters SET scanlator = ? WHERE id = ?")
        .bind(name)
        .bind(chapter.0)
        .execute(&svc.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn preview_count_matches_filter_under_scanlator_priority_dedup() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "s").await;
    let manga = insert_manga(&svc.db, src, "m", "M").await;

    let ch1 = insert_chapter(&svc.db, manga, "c1", 1.0).await;
    let ch2 = insert_chapter(&svc.db, manga, "c2", 1.0).await;
    let ch3 = insert_chapter(&svc.db, manga, "c3", 2.0).await;
    set_scanlator(&svc, ch1, "GroupA").await;
    set_scanlator(&svc, ch2, "GroupB").await;
    set_scanlator(&svc, ch3, "GroupA").await;

    svc.set_scanlator_pref(manga, "GroupA", 10, false)
        .await
        .unwrap();
    svc.set_scanlator_pref(manga, "GroupB", 1, false)
        .await
        .unwrap();

    let (matching, total) = svc.preview_download_rules(manga, vec![]).await.unwrap();
    assert_eq!(total, 3);
    assert_eq!(
        matching, 2,
        "preview must count the deduped set (GroupA wins number 1.0), not all 3"
    );

    let filtered = svc
        .filter_chapters_by_rules(manga, vec![ch1, ch2, ch3])
        .await;
    assert_eq!(
        filtered.len(),
        matching,
        "preview count must equal the real filter's output count"
    );
    assert!(filtered.contains(&ch1) && filtered.contains(&ch3));
    assert!(
        !filtered.contains(&ch2),
        "the lower-priority duplicate must be dropped by both preview and filter"
    );
}

#[tokio::test]
async fn preview_returns_total_when_no_scanlator_prefs() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "s").await;
    let manga = insert_manga(&svc.db, src, "m", "M").await;
    insert_chapter(&svc.db, manga, "c1", 1.0).await;
    insert_chapter(&svc.db, manga, "c2", 2.0).await;

    let (matching, total) = svc.preview_download_rules(manga, vec![]).await.unwrap();
    assert_eq!((matching, total), (2, 2));
}
