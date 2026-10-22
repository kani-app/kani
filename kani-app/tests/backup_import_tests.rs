#![allow(clippy::unwrap_used)]

mod common;
use common::{insert_manga, insert_source, insert_user, test_service};
use kani_app::RestoreOptions;
use kani_app::ids::{MangaId, UserId};

#[tokio::test]
async fn preview_backup_shows_correct_manga_count() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Dragon Ball").await;
    insert_manga(&svc.db, src, "m2", "Naruto").await;

    let zip = svc.export_backup(UserId(1), false, None).await.unwrap();
    let preview = svc.preview_backup(&zip, None).await.unwrap();

    assert_eq!(preview.manga_count, 2);
    assert_eq!(preview.version, 1);
    assert_eq!(preview.category_count, 0);
    assert!(!preview.has_tracking);
    assert!(!preview.has_chapter_progress);
}

#[tokio::test]
async fn restore_backup_reimports_manga_after_wipe() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Dragon Ball").await;

    let zip = svc.export_backup(UserId(1), false, None).await.unwrap();

    let result = svc
        .restore_backup(UserId(1), &zip, RestoreOptions::default(), None)
        .await
        .unwrap();

    assert_eq!(result.imported_manga, 1);
    assert_eq!(result.skipped_manga, 0);
    assert_eq!(result.pending_imports_added, 0);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn restore_backup_unknown_source_adds_pending_import() {
    let svc1 = test_service().await;
    let src = insert_source(&svc1.db, "src").await;
    insert_manga(&svc1.db, src, "m1", "Dragon Ball").await;
    let zip = svc1.export_backup(UserId(1), false, None).await.unwrap();

    let svc2 = test_service().await;
    let user2 = insert_user(&svc2.db, "user").await;
    let result = svc2
        .restore_backup(user2, &zip, RestoreOptions::default(), None)
        .await
        .unwrap();

    assert_eq!(
        result.imported_manga, 0,
        "source unknown → should not import"
    );
    assert_eq!(result.pending_imports_added, 1);
    assert_eq!(result.skipped_manga, 1);
    assert!(!result.warnings.is_empty());

    let manga_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga")
        .fetch_one(&svc2.db)
        .await
        .unwrap();
    assert_eq!(manga_count, 0);

    let pending_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pending_imports")
        .fetch_one(&svc2.db)
        .await
        .unwrap();
    assert_eq!(pending_count, 1);
}

#[tokio::test]
async fn restore_backup_preserves_category_assignment() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    let manga_id = insert_manga(&svc.db, src, "m1", "Dragon Ball").await;

    let cat_id = svc.create_category("Action", 0).await.unwrap();
    svc.set_manga_categories(manga_id, vec![cat_id])
        .await
        .unwrap();

    let zip = svc.export_backup(UserId(1), false, None).await.unwrap();

    let result = svc
        .restore_backup(UserId(1), &zip, RestoreOptions::default(), None)
        .await
        .unwrap();
    assert_eq!(result.imported_manga, 1);
    assert_eq!(result.imported_categories, 1);

    let restored_manga_id: i64 =
        sqlx::query_scalar("SELECT id FROM manga WHERE source_manga_id = 'm1'")
            .fetch_one(&svc.db)
            .await
            .unwrap();

    let cats = svc
        .get_manga_categories(MangaId(restored_manga_id))
        .await
        .unwrap();
    assert_eq!(
        cats.len(),
        1,
        "category assignment should survive export+restore"
    );
}

#[tokio::test]
async fn restore_backup_merge_mode_keeps_existing_manga() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Dragon Ball").await;

    let zip = svc.export_backup(UserId(1), false, None).await.unwrap();

    insert_manga(&svc.db, src, "m2", "Naruto").await;

    let opts = RestoreOptions {
        merge: true,
        import_manga: true,
        import_categories: true,
        import_download_rules: true,
        import_tracking: true,
        import_chapter_progress: false,
        import_settings: false,
        import_repos: true,
    };
    svc.restore_backup(UserId(1), &zip, opts, None)
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM manga")
        .fetch_one(&svc.db)
        .await
        .unwrap();
    assert_eq!(
        count, 2,
        "both original and post-export manga should remain"
    );
}

#[tokio::test]
async fn encrypted_backup_roundtrip() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Bleach").await;

    let passphrase = Some("correct-horse-battery".to_string());
    let encrypted = svc
        .export_backup(UserId(1), false, passphrase.clone())
        .await
        .unwrap();

    assert!(
        encrypted.starts_with(b"KANI-BACKUP-V2\n"),
        "encrypted backup should start with magic"
    );

    let result = svc
        .restore_backup(UserId(1), &encrypted, RestoreOptions::default(), passphrase)
        .await
        .unwrap();
    assert_eq!(result.imported_manga, 1);
}

#[tokio::test]
async fn encrypted_backup_wrong_passphrase_is_rejected() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Bleach").await;

    let encrypted = svc
        .export_backup(UserId(1), false, Some("correct".to_string()))
        .await
        .unwrap();

    let result = svc
        .restore_backup(
            UserId(1),
            &encrypted,
            RestoreOptions::default(),
            Some("wrong".to_string()),
        )
        .await;
    assert!(result.is_err(), "wrong passphrase should fail");
}

#[tokio::test]
async fn encrypted_backup_without_passphrase_is_rejected() {
    let svc = test_service().await;
    let src = insert_source(&svc.db, "src").await;
    insert_manga(&svc.db, src, "m1", "Bleach").await;

    let encrypted = svc
        .export_backup(UserId(1), false, Some("secret".to_string()))
        .await
        .unwrap();

    let result = svc
        .restore_backup(UserId(1), &encrypted, RestoreOptions::default(), None)
        .await;
    assert!(
        result.is_err(),
        "encrypted backup without passphrase should fail"
    );
}

use kani_shared::types::{MaintenanceSettings, SecuritySettings, SettingsUpdate};

#[tokio::test]
async fn backup_settings_round_trip_covers_every_group() {
    let svc = test_service().await;

    let before = svc.get_settings().await;
    svc.update_settings(
        SettingsUpdate::Security(SecuritySettings {
            max_login_attempts: 17,
            max_ip_attempts: 41,
            login_lockout_seconds: 123,
            session_timeout_secs: 4567,
        }),
        UserId(1),
    )
    .await
    .unwrap();
    svc.update_settings(
        SettingsUpdate::Maintenance(MaintenanceSettings {
            trash_retention_days: 19,
            audit_retention_days: 23,
            audit_security_retention_days: 29,
            disk_warn_threshold: 0.77,
            thumbnail_formats: "avif".into(),
            integrity_quick_scrub_interval_hours: 31,
            integrity_deep_scrub_interval_hours: 37,
            scrub_on_startup: !before.scrub_on_startup,
            integrity_revalidate_after_days: 41,
        }),
        UserId(1),
    )
    .await
    .unwrap();

    let zip = svc.export_backup(UserId(1), false, None).await.unwrap();

    svc.update_settings(
        SettingsUpdate::Security(SecuritySettings {
            max_login_attempts: before.max_login_attempts,
            max_ip_attempts: before.max_ip_attempts,
            login_lockout_seconds: before.login_lockout_seconds,
            session_timeout_secs: before.session_timeout_secs,
        }),
        UserId(1),
    )
    .await
    .unwrap();

    let opts = RestoreOptions {
        import_settings: true,
        ..RestoreOptions::default()
    };
    svc.restore_backup(UserId(1), &zip, opts, None)
        .await
        .unwrap();

    let after = svc.get_settings().await;
    assert_eq!(
        after.max_login_attempts, 17,
        "security group did not round-trip"
    );
    assert_eq!(after.max_ip_attempts, 41);
    assert_eq!(after.login_lockout_seconds, 123);
    assert_eq!(after.session_timeout_secs, 4567);
    assert_eq!(
        after.trash_retention_days, 19,
        "maintenance group did not round-trip"
    );
    assert_eq!(after.thumbnail_formats, "avif");
    assert!((after.disk_warn_threshold - 0.77).abs() < f64::EPSILON);
}

#[tokio::test]
async fn restoring_settings_refreshes_the_live_cache_not_just_the_row() {
    let svc = test_service().await;
    svc.update_settings(
        SettingsUpdate::Security(SecuritySettings {
            max_login_attempts: 13,
            max_ip_attempts: 14,
            login_lockout_seconds: 900,
            session_timeout_secs: 3600,
        }),
        UserId(1),
    )
    .await
    .unwrap();
    let zip = svc.export_backup(UserId(1), false, None).await.unwrap();

    svc.update_settings(
        SettingsUpdate::Security(SecuritySettings {
            max_login_attempts: 99,
            max_ip_attempts: 98,
            login_lockout_seconds: 1800,
            session_timeout_secs: 7200,
        }),
        UserId(1),
    )
    .await
    .unwrap();

    let opts = RestoreOptions {
        import_settings: true,
        ..RestoreOptions::default()
    };
    svc.restore_backup(UserId(1), &zip, opts, None)
        .await
        .unwrap();

    assert_eq!(svc.get_settings().await.max_login_attempts, 13);
    assert_eq!(svc.get_settings().await.session_timeout_secs, 3600);
}

#[tokio::test]
async fn the_smtp_password_never_reaches_the_backup_file() {
    let svc = test_service().await;
    let before = svc.get_settings().await;
    svc.update_settings(
        SettingsUpdate::Email(kani_shared::types::EmailSettings {
            email_enabled: true,
            email_provider: "smtp".into(),
            email_provider_config:
                r#"{"host":"smtp.example.com","username":"postmaster","password":"hunter2-secret"}"#
                    .into(),
            email_from_address: "kani@example.com".into(),
            app_url: before.app_url.clone(),
            password_reset_enabled: before.password_reset_enabled,
            email_verification_required: before.email_verification_required,
        }),
        UserId(1),
    )
    .await
    .unwrap();

    let zip = svc.export_backup(UserId(1), false, None).await.unwrap();
    let haystack = String::from_utf8_lossy(&zip).to_string();
    assert!(
        !haystack.contains("hunter2-secret"),
        "the backup archive contains the SMTP password"
    );
}

/// Rewrites `backup.json` inside a backup zip, leaving every other entry intact.
fn rewrite_settings_json(zip_bytes: &[u8], edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut reader = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..reader.len() {
        let mut f = reader.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut buf).unwrap();
        entries.push((name, buf));
    }

    for entry in &mut entries {
        if entry.0 == "backup.json" {
            let mut v: serde_json::Value = serde_json::from_slice(&entry.1).unwrap();
            edit(&mut v["settings"]);
            entry.1 = serde_json::to_vec(&v).unwrap();
            break;
        }
    }

    let out = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(out);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        writer.start_file(&name, opts).unwrap();
        std::io::Write::write_all(&mut writer, &bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

async fn advanced_of(svc: &kani_app::AppService) -> kani_shared::types::AdvancedSettings {
    let s = svc.get_settings().await;
    kani_shared::types::AdvancedSettings {
        flaresolverr_url: s.flaresolverr_url.clone(),
        library_path: s.library_path.clone(),
        wasm_storage_path: s.wasm_storage_path.clone(),
        max_wasm_instances: s.max_wasm_instances,
        http_request_logging: s.http_request_logging,
        v8_debug_logging: s.v8_debug_logging,
        registration_enabled: s.registration_enabled,
        cover_max_dimension: s.cover_max_dimension,
        v8_max_memory_mb: s.v8_max_memory_mb,
        v8_idle_timeout_s: s.v8_idle_timeout_s,
        update_check_enabled: s.update_check_enabled,
        opds_page_index_zero_based: s.opds_page_index_zero_based,
        global_search_timeout_secs: s.global_search_timeout_secs,
    }
}

/// A backup written before the `browser_*` settings were renamed must still
/// restore. The renamed fields are required rather than optional, so without the
/// serde aliases every pre-rename backup fails to deserialise on a missing field,
/// and `browser_max_instances` has no field left to land in at all.
#[tokio::test]
async fn a_backup_written_before_the_v8_rename_still_restores() {
    let svc = test_service().await;

    let zip = svc.export_backup(UserId(1), false, None).await.unwrap();
    let aged = rewrite_settings_json(&zip, |settings| {
        let advanced = settings["advanced"].as_object_mut().unwrap();
        let memory = advanced.remove("v8_max_memory_mb").unwrap();
        let idle = advanced.remove("v8_idle_timeout_s").unwrap();
        let logging = advanced.remove("v8_debug_logging").unwrap();
        advanced.insert("browser_max_memory_mb".into(), memory);
        advanced.insert("browser_idle_timeout_s".into(), idle);
        advanced.insert("browser_debug_logging".into(), logging);
        advanced.insert("browser_max_instances".into(), serde_json::json!(4));
    });

    svc.update_settings(
        SettingsUpdate::Advanced(kani_shared::types::AdvancedSettings {
            v8_max_memory_mb: 999,
            ..advanced_of(&svc).await
        }),
        UserId(1),
    )
    .await
    .unwrap();

    let opts = RestoreOptions {
        import_settings: true,
        ..RestoreOptions::default()
    };
    svc.restore_backup(UserId(1), &aged, opts, None)
        .await
        .expect("a pre-rename backup must restore, not fail on a missing field");

    let after = svc.get_settings().await;
    assert_eq!(
        after.v8_max_memory_mb, 512,
        "the old browser_max_memory_mb value must land in the renamed column"
    );
}

#[tokio::test]
async fn restoring_progress_only_reaches_manga_already_in_the_library() {
    let svc = test_service().await;
    let uid = insert_user(&svc.db, "reader").await;
    let src = insert_source(&svc.db, "src").await;
    let manga = insert_manga(&svc.db, src, "m1", "Dragon Ball").await;
    let ch = common::insert_chapter(&svc.db, manga, "c1", 1.0).await;

    sqlx::query(
        "INSERT INTO user_chapter_tracking (user_id, chapter_id, is_read, last_page_read) \
         VALUES (?, ?, 1, 5)",
    )
    .bind(uid)
    .bind(ch.0)
    .execute(&svc.db)
    .await
    .unwrap();

    let zip = svc.export_backup(uid, true, None).await.unwrap();

    sqlx::query("DELETE FROM user_chapter_tracking")
        .execute(&svc.db)
        .await
        .unwrap();

    let opts = RestoreOptions {
        merge: true,
        import_manga: false,
        import_categories: false,
        import_download_rules: false,
        import_tracking: false,
        import_chapter_progress: true,
        import_settings: false,
        import_repos: false,
    };
    svc.restore_backup(uid, &zip, opts, None).await.unwrap();

    let restored: Option<(bool, i64)> = sqlx::query_as(
        "SELECT is_read, last_page_read FROM user_chapter_tracking \
         WHERE user_id = ? AND chapter_id = ?",
    )
    .bind(uid)
    .bind(ch.0)
    .fetch_optional(&svc.db)
    .await
    .unwrap();
    assert_eq!(
        restored,
        Some((true, 5)),
        "a progress-only restore must reach the library that is already there"
    );
}
