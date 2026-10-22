#![allow(clippy::unwrap_used)]

mod common;
use common::test_service;
use kani_app::ids::UserId;
use kani_shared::types::{AdvancedSettings, DownloadSettings, ScanSettings, SettingsUpdate};

#[tokio::test]
async fn get_settings_reflects_initial_values() {
    let svc = test_service().await;
    let s = svc.get_settings().await;
    assert_eq!(s.concurrent_page_downloads, 4);
    assert_eq!(s.max_retries, 3);
    assert!(!s.auto_scan);
    assert!(s.registration_enabled);
}

#[tokio::test]
async fn update_download_settings_round_trips() {
    let svc = test_service().await;

    svc.update_settings(
        SettingsUpdate::Download(DownloadSettings {
            concurrent_page_downloads: 8,
            max_retries: 5,
            initial_retry_delay_ms: 200,
            auto_download_category_ids: vec![],
            scan_concurrency: 4,
            per_source_download_concurrency: 2,
        }),
        UserId(1),
    )
    .await
    .unwrap();

    let s = svc.get_settings().await;
    assert_eq!(s.concurrent_page_downloads, 8);
    assert_eq!(s.max_retries, 5);
    assert_eq!(s.initial_retry_delay_ms, 200);
}

fn advanced_settings() -> AdvancedSettings {
    AdvancedSettings {
        flaresolverr_url: String::new(),
        library_path: "/data/library".into(),
        wasm_storage_path: "/data/wasm".into(),
        max_wasm_instances: 4,
        http_request_logging: false,
        v8_debug_logging: false,
        registration_enabled: true,
        cover_max_dimension: Some(512),
        v8_max_memory_mb: 512,
        v8_idle_timeout_s: 300,
        update_check_enabled: true,
        opds_page_index_zero_based: false,
        global_search_timeout_secs: 6,
    }
}

#[tokio::test]
async fn update_advanced_settings_round_trips_v8_caps() {
    let svc = test_service().await;

    svc.update_settings(
        SettingsUpdate::Advanced(AdvancedSettings {
            v8_max_memory_mb: 1024,
            v8_idle_timeout_s: 120,
            ..advanced_settings()
        }),
        UserId(1),
    )
    .await
    .unwrap();

    let s = svc.get_settings().await;
    assert_eq!(s.v8_max_memory_mb, 1024);
    assert_eq!(s.v8_idle_timeout_s, 120);
}

#[tokio::test]
async fn update_advanced_settings_rejects_an_out_of_range_v8_idle_timeout() {
    let svc = test_service().await;

    let result = svc
        .update_settings(
            SettingsUpdate::Advanced(AdvancedSettings {
                v8_idle_timeout_s: 5,
                ..advanced_settings()
            }),
            UserId(1),
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn global_search_timeout_defaults_and_round_trips() {
    let svc = test_service().await;

    assert_eq!(svc.get_settings().await.global_search_timeout_secs, 6);

    svc.update_settings(
        SettingsUpdate::Advanced(AdvancedSettings {
            global_search_timeout_secs: 9,
            ..advanced_settings()
        }),
        UserId(1),
    )
    .await
    .unwrap();

    assert_eq!(svc.get_settings().await.global_search_timeout_secs, 9);
}

#[tokio::test]
async fn global_search_timeout_rejects_values_outside_its_bound() {
    let svc = test_service().await;

    let result = svc
        .update_settings(
            SettingsUpdate::Advanced(AdvancedSettings {
                global_search_timeout_secs: 61,
                ..advanced_settings()
            }),
            UserId(1),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn update_scan_settings_round_trips() {
    let svc = test_service().await;

    assert_eq!(svc.get_settings().await.scan_barren_page_tolerance, 3);

    svc.update_settings(
        SettingsUpdate::Scan(ScanSettings {
            auto_scan: true,
            scan_interval_minutes: 30,
            scan_exclude_completed: true,
            upgrade_detection_enabled: true,
            upgrade_min_res_gain: 1.2,
            upgrade_confirm_fetches: 3,
            upgrade_axis_resolution: "both".into(),
            upgrade_axis_colour: "both".into(),
            upgrade_axis_encoder: "both".into(),
            upgrade_axis_bitrate: "gain".into(),
            upgrade_show_downgrades: false,
            upgrade_auto_replace_reasons: "preferred_scanlator,resolution,colour".into(),
            scan_barren_page_tolerance: 4,
        }),
        UserId(1),
    )
    .await
    .unwrap();

    let s = svc.get_settings().await;
    assert!(s.auto_scan);
    assert_eq!(s.scan_interval_minutes, 30);
    assert!(s.scan_exclude_completed);
    assert_eq!(s.scan_barren_page_tolerance, 4);
}

#[tokio::test]
async fn update_scan_settings_rejects_short_interval() {
    let svc = test_service().await;
    let result = svc
        .update_settings(
            SettingsUpdate::Scan(ScanSettings {
                auto_scan: false,
                scan_interval_minutes: 4,
                scan_exclude_completed: false,
                upgrade_detection_enabled: true,
                upgrade_min_res_gain: 1.2,
                upgrade_confirm_fetches: 3,
                upgrade_axis_resolution: "both".into(),
                upgrade_axis_colour: "both".into(),
                upgrade_axis_encoder: "both".into(),
                upgrade_axis_bitrate: "gain".into(),
                upgrade_show_downgrades: false,
                upgrade_auto_replace_reasons: "preferred_scanlator,resolution,colour".into(),
                scan_barren_page_tolerance: 3,
            }),
            UserId(1),
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn scan_barren_page_tolerance_rejects_values_outside_its_bound() {
    let svc = test_service().await;

    let result = svc
        .update_settings(
            SettingsUpdate::Scan(ScanSettings {
                auto_scan: false,
                scan_interval_minutes: 5,
                scan_exclude_completed: false,
                upgrade_detection_enabled: true,
                upgrade_min_res_gain: 1.2,
                upgrade_confirm_fetches: 3,
                upgrade_axis_resolution: "both".into(),
                upgrade_axis_colour: "both".into(),
                upgrade_axis_encoder: "both".into(),
                upgrade_axis_bitrate: "gain".into(),
                upgrade_show_downgrades: false,
                upgrade_auto_replace_reasons: "preferred_scanlator,resolution,colour".into(),
                scan_barren_page_tolerance: 21,
            }),
            UserId(1),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn update_download_settings_rejects_invalid_page_concurrency() {
    let svc = test_service().await;
    let result = svc
        .update_settings(
            SettingsUpdate::Download(DownloadSettings {
                concurrent_page_downloads: 0,
                max_retries: 3,
                initial_retry_delay_ms: 100,
                auto_download_category_ids: vec![],
                scan_concurrency: 4,
                per_source_download_concurrency: 2,
            }),
            UserId(1),
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn update_settings_does_not_affect_unrelated_fields() {
    let svc = test_service().await;

    svc.update_settings(
        SettingsUpdate::Scan(ScanSettings {
            auto_scan: true,
            scan_interval_minutes: 30,
            scan_exclude_completed: false,
            upgrade_detection_enabled: true,
            upgrade_min_res_gain: 1.2,
            upgrade_confirm_fetches: 3,
            upgrade_axis_resolution: "both".into(),
            upgrade_axis_colour: "both".into(),
            upgrade_axis_encoder: "both".into(),
            upgrade_axis_bitrate: "gain".into(),
            upgrade_show_downgrades: false,
            upgrade_auto_replace_reasons: "preferred_scanlator,resolution,colour".into(),
            scan_barren_page_tolerance: 3,
        }),
        UserId(1),
    )
    .await
    .unwrap();

    svc.update_settings(
        SettingsUpdate::Download(DownloadSettings {
            concurrent_page_downloads: 8,
            max_retries: 3,
            initial_retry_delay_ms: 100,
            auto_download_category_ids: vec![],
            scan_concurrency: 4,
            per_source_download_concurrency: 2,
        }),
        UserId(1),
    )
    .await
    .unwrap();

    let s = svc.get_settings().await;
    assert!(s.auto_scan, "scan setting should be unchanged");
    assert_eq!(
        s.scan_interval_minutes, 30,
        "scan interval should be unchanged"
    );
    assert_eq!(
        s.concurrent_page_downloads, 8,
        "download setting should be updated"
    );
}

#[tokio::test]
async fn update_check_enabled_round_trips_and_defaults_on() {
    let svc = test_service().await;

    assert!(
        svc.get_settings().await.update_check_enabled,
        "update checking should be on by default"
    );

    let mut advanced = advanced_settings();
    advanced.update_check_enabled = false;
    svc.update_settings(SettingsUpdate::Advanced(advanced), UserId(1))
        .await
        .unwrap();

    assert!(
        !svc.get_settings().await.update_check_enabled,
        "the toggle must persist through update_settings"
    );
}

#[tokio::test]
async fn a_registered_degradation_reaches_the_diagnostics_payload() {
    let svc = common::test_service().await;
    assert!(
        svc.get_diagnostics().await.unwrap().degradations.is_empty(),
        "a healthy service reports nothing"
    );

    svc.degradations.register(
        kani_app::service::degradations::ids::WASM_MODULE_CACHE,
        kani_app::service::degradations::Severity::Warn,
        "WASM module cache",
        "not writable",
        "make it writable",
    );

    let payload = svc.get_diagnostics().await.unwrap();
    assert_eq!(payload.degradations.len(), 1);
    let d = &payload.degradations[0];
    assert_eq!(d.title, "WASM module cache");
    assert_eq!(d.detail, "not writable");
    assert!(!d.remedy.is_empty(), "every degradation carries a remedy");
}

#[tokio::test]
async fn diagnostics_lists_errors_before_warnings() {
    let svc = common::test_service().await;
    use kani_app::service::degradations::{Severity, ids};

    svc.degradations.register(
        ids::WASM_MODULE_CACHE,
        Severity::Warn,
        "Cache",
        "slow",
        "fix",
    );
    svc.degradations.register(
        ids::ENCRYPTED_SETTINGS,
        Severity::Error,
        "Encrypted settings",
        "cannot decrypt",
        "restore secret.key",
    );

    let payload = svc.get_diagnostics().await.unwrap();
    assert_eq!(payload.degradations[0].severity, Severity::Error);
    assert_eq!(payload.degradations[1].severity, Severity::Warn);
}

#[tokio::test]
async fn a_recovered_subsystem_disappears_from_diagnostics() {
    let svc = common::test_service().await;
    use kani_app::service::degradations::{Severity, ids};

    svc.degradations
        .register(ids::LIBRARY_PATH, Severity::Warn, "Library", "gone", "fix");
    assert_eq!(svc.get_diagnostics().await.unwrap().degradations.len(), 1);

    svc.degradations.clear(ids::LIBRARY_PATH);
    assert!(svc.get_diagnostics().await.unwrap().degradations.is_empty());
}

#[tokio::test]
async fn a_solver_without_the_egress_guard_is_reported_until_fixed() {
    use kani_app::service::degradations::ids::SOLVER_EGRESS_GUARD;
    use kani_shared_test::origin::{Response, TestOrigin};

    let solver = TestOrigin::start().await;
    let svc = common::test_service().await;
    let reported = |svc: &kani_app::service::AppService| {
        svc.degradations
            .list()
            .iter()
            .any(|d| d.id == SOLVER_EGRESS_GUARD)
    };
    svc.smart_client
        .update_solver_url(Some(solver.url("/v1")))
        .await;

    solver.set(
        "/",
        Response::json(r#"{"capabilities":["kani.capture/1","kani.capture/2"]}"#),
    );
    svc.refresh_solver_egress_degradation().await;
    assert!(reported(&svc), "a stock-capability solver must be reported");

    solver.set(
        "/",
        Response::json(r#"{"capabilities":["kani.capture/2","kani.egress-guard/1"]}"#),
    );
    svc.refresh_solver_egress_degradation().await;
    assert!(!reported(&svc), "a guarded solver clears the warning");

    solver.set("/", Response::json(r#"{"capabilities":[]}"#));
    svc.refresh_solver_egress_degradation().await;
    svc.smart_client.update_solver_url(None).await;
    svc.refresh_solver_egress_degradation().await;
    assert!(!reported(&svc), "no solver means nothing to warn about");
}
