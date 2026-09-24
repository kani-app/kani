use utoipa::{Modify, OpenApi};

/// Compatibility tier of a published operation.
///
/// `Stable` operations are covered by the 1.x compatibility promise: within a major version they
/// may gain optional fields but may not remove or repurpose existing ones, change status codes, or
/// move. `Unstable` operations carry no such promise and may change in any release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stability {
    Stable,
    Unstable,
}

impl Stability {
    /// Value published as `x-stability` in the OpenAPI document.
    pub fn as_str(self) -> &'static str {
        match self {
            Stability::Stable => "stable",
            Stability::Unstable => "unstable",
        }
    }
}

/// Path prefixes excluded from the compatibility promise.
///
/// A prefix matches the path itself and anything below it, so `/rest/jobs` covers
/// `/rest/jobs/{id}`. Anything not listed is stable, which makes the omission of a new
/// administrative or internal route the failure that must be caught in review.
const UNSTABLE_PREFIXES: &[&str] = &[
    "/rest/admin",
    "/rest/boot_id",
    "/rest/features",
    "/rest/image_proxy",
    "/rest/jobs",
    "/rest/refresh",
    "/rest/server",
    "/rest/trash",
    "/rest/ui",
];

fn covers(prefix: &str, path: &str) -> bool {
    match path.strip_prefix(prefix) {
        Some("") => true,
        Some(rest) => rest.starts_with('/'),
        None => false,
    }
}

/// Compatibility tier for a documented path. Unlisted paths are [`Stability::Stable`].
pub(crate) fn stability_for(path: &str) -> Stability {
    if UNSTABLE_PREFIXES.iter().any(|p| covers(p, path)) {
        Stability::Unstable
    } else {
        Stability::Stable
    }
}

/// Stamps every operation with its `x-stability` tier so the published document carries it.
pub(crate) struct StabilityAddon;

impl Modify for StabilityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        for (path, item) in &mut openapi.paths.paths {
            let tier = stability_for(path);
            let operations = [
                &mut item.get,
                &mut item.post,
                &mut item.put,
                &mut item.patch,
                &mut item.delete,
                &mut item.head,
                &mut item.options,
                &mut item.trace,
            ];
            for op in operations.into_iter().flatten() {
                let mut extensions = op.extensions.clone().unwrap_or_default();
                extensions.insert(
                    "x-stability".to_string(),
                    serde_json::Value::String(tier.as_str().to_string()),
                );
                op.extensions = Some(extensions);
            }
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    modifiers(&StabilityAddon),
    info(
        title = "Kani API",
        version = env!("CARGO_PKG_VERSION"),
        description = "REST API for Kani — a self-hosted manga/comics server.",
        license(name = "MIT"),
    ),
    paths(
        // system
        crate::rest::system::system_info,
        crate::rest::system::system_changelog,
        crate::rest::system::complete_first_run,
        crate::rest::system::system_update,
        crate::rest::admin::system_capabilities,
        // auth
        crate::rest::auth::auth_login,
        crate::rest::auth::auth_logout,
        crate::rest::auth::auth_me,
        crate::rest::auth::get_current_user,
        crate::rest::auth::change_password,
        crate::rest::auth::logout_everywhere,
        crate::rest::auth::list_sessions,
        crate::rest::auth::revoke_all_other_sessions,
        crate::rest::auth::revoke_session,
        crate::rest::auth::totp_setup,
        crate::rest::auth::totp_verify,
        crate::rest::auth::get_features,
        crate::rest::auth::get_my_permissions,
        crate::rest::auth::get_registration_enabled,
        crate::rest::auth::auth_register,
        crate::rest::auth::get_password_reset_enabled,
        crate::rest::auth::password_reset_request,
        crate::rest::auth::password_reset_confirm,
        crate::rest::auth::password_strength,
        crate::rest::auth::totp_disable,
        crate::rest::auth::totp_regenerate_backup_codes,
        crate::rest::auth::totp_step_up,
        crate::rest::auth::get_captcha,
        crate::rest::auth::password_reset_validate,
        crate::rest::auth::verify_email,
        crate::rest::auth::resend_verification,
        crate::rest::auth::auth_setup,
        crate::rest::auth::setup_state,
        // settings
        crate::rest::settings::get_settings,
        crate::rest::settings::start_refresh_all_rest,
        crate::rest::settings::get_refresh_status,
        crate::rest::settings::update_settings,
        crate::rest::settings::test_solver,
        // library
        crate::rest::library::get_library_filtered,
        crate::rest::library::get_library,
        crate::rest::library::scan_all_library,
        crate::rest::library::scan_manga_multiple,
        crate::rest::library::get_continue_reading_shelf,
        crate::rest::library::library_backup,
        crate::rest::library::library_backup_preview,
        crate::rest::library::library_restore,
        crate::rest::library::library_tachiyomi_preview,
        crate::rest::library::library_import_tachiyomi,
        crate::rest::library::library_pending_imports,
        crate::rest::library::library_delete_pending_import,
        crate::rest::library::library_resolve_pending_import,
        crate::rest::library::library_orphaned,
        crate::rest::library::library_duplicates,
        crate::rest::library::library_merge_duplicate,
        crate::rest::library::library_duplicates_scan,
        crate::rest::library::library_dismiss_duplicate,
        crate::rest::library::get_recent_updates,
        crate::rest::library::global_search_handler,
        // categories
        crate::rest::categories::list_categories,
        crate::rest::categories::create_category,
        crate::rest::categories::reorder_categories,
        crate::rest::categories::rename_category,
        crate::rest::categories::delete_category_handler,
        crate::rest::categories::get_manga_categories,
        crate::rest::categories::set_manga_categories,
        // manga
        crate::rest::manga::get_manga,
        crate::rest::manga::delete_manga,
        crate::rest::manga::upload_manga_cover_handler,
        crate::rest::manga::clear_manga_cover_handler,
        crate::rest::manga::get_local_manga_details,
        crate::rest::manga::get_local_chapters,
        crate::rest::manga::get_chapter_ids,
        crate::rest::manga::download_all,
        crate::rest::manga::cancel_all_downloads,
        crate::rest::manga::relink_manga,
        crate::rest::manga::refresh_manga,
        crate::rest::manga::scan_manga,
        crate::rest::manga::toggle_auto_download,
        crate::rest::manga::toggle_download_all_preferred,
        crate::rest::manga::update_manga_notes,
        crate::rest::manga::update_local_metadata_handler,
        crate::rest::manga::mark_manga_seen,
        crate::rest::manga::preview_migration,
        crate::rest::manga::migrate_manga_handler,
        crate::rest::manga::match_migration_targets,
        crate::rest::manga::submit_bulk_migration,
        crate::rest::manga::migration_statuses,
        crate::rest::manga::get_download_rules,
        crate::rest::manga::add_download_rule,
        crate::rest::manga::delete_download_rule,
        crate::rest::manga::update_download_rule,
        crate::rest::manga::reorder_download_rules,
        crate::rest::manga::preview_download_rules,
        crate::rest::manga::enrich_metadata_handler,
        crate::rest::manga::toggle_auto_scan_manga,
        crate::rest::manga::apply_chapter_upgrade,
        crate::rest::manga::dismiss_chapter_upgrade,
        crate::rest::manga::untrash_manga_by_token_handler,
        crate::rest::manga::dismiss_suppressed_chapters,
        crate::rest::manga::untrash_manga_handler,
        crate::rest::manga::set_upgrade_auto_replace,
        crate::rest::manga::get_notify_prefs,
        crate::rest::manga::get_all_upgrades,
        crate::rest::manga::purge_trash_all_handler,
        crate::rest::manga::list_trash_handler,
        crate::rest::manga::purge_trash_one_handler,
        // scanlators
        crate::rest::scanlators::get_scanlator_prefs,
        crate::rest::scanlators::set_scanlator_pref,
        crate::rest::scanlators::delete_scanlator_pref,
        crate::rest::scanlators::set_scanlator_mode_handler,
        crate::rest::scanlators::get_chapter_scanlators,
        crate::rest::scanlators::get_chapter_languages,
        crate::rest::scanlators::get_global_prefs,
        crate::rest::scanlators::set_global_pref,
        crate::rest::scanlators::get_known_scanlators,
        // chapters
        crate::rest::chapters::get_chapter_page_manifest,
        crate::rest::chapters::set_chapter_progress_handler,
        crate::rest::chapters::get_bookmarks_handler,
        crate::rest::chapters::toggle_bookmark_handler,
        crate::rest::chapters::get_chapter_note_handler,
        crate::rest::chapters::set_chapter_note_handler,
        crate::rest::chapters::get_manga_chapter_notes_handler,
        crate::rest::chapters::set_chapter_read_status_handler,
        crate::rest::chapters::get_continue_reading_handler,
        crate::rest::chapters::mark_chapters_up_to_handler,
        // downloads
        crate::rest::downloads::get_download_history,
        crate::rest::downloads::start_download,
        crate::rest::downloads::delete_downloaded,
        crate::rest::downloads::cancel_download,
        crate::rest::downloads::cancel_all_global_downloads,
        crate::rest::downloads::retry_download,
        crate::rest::downloads::get_manga_download_status,
        // export
        crate::rest::export::serve_chapter_cbz,
        crate::rest::export::export_epub,
        crate::rest::export::export_kepub,
        crate::rest::export::export_kcc,
        // api tokens
        crate::rest::api_tokens::list_tokens,
        crate::rest::api_tokens::create_token,
        crate::rest::api_tokens::revoke_token,
        // stats
        crate::rest::stats::reading_stats,
        // sources
        crate::rest::sources::list_sources,
        crate::rest::sources::add_source,
        crate::rest::sources::get_sources_health,
        crate::rest::sources::get_active_source_ids,
        crate::rest::sources::list_metadata_providers,
        crate::rest::sources::get_source,
        crate::rest::sources::update_source,
        crate::rest::sources::delete_source,
        crate::rest::sources::get_metadata,
        crate::rest::sources::upload_wasm,
        crate::rest::sources::fetch_wasm,
        crate::rest::sources::install_wasm,
        crate::rest::sources::install_wasm_from_url,
        crate::rest::sources::install_yaml,
        crate::rest::sources::fetch_yaml,
        crate::rest::sources::reload_source_handler,
        crate::rest::sources::get_popular_manga,
        crate::rest::sources::search_manga,
        crate::rest::sources::get_manga_details,
        crate::rest::sources::get_source_manga_url,
        crate::rest::sources::save_to_library,
        crate::rest::sources::get_chapter_list,
        crate::rest::sources::get_chapter_sort_list,
        crate::rest::sources::get_pages,
        crate::rest::sources::check_in_library,
        crate::rest::sources::toggle_source_enabled,
        crate::rest::sources::toggle_source_favourite,
        crate::rest::sources::get_source_filters,
        crate::rest::sources::get_pref_schema,
        crate::rest::sources::get_source_preferences,
        crate::rest::sources::set_source_preference,
        crate::rest::sources::append_pref_list_item,
        crate::rest::sources::remove_pref_list_item,
        crate::rest::sources::toggle_pref_select_item,
        crate::rest::sources::get_all_capabilities,
        crate::rest::sources::get_capabilities,
        crate::rest::sources::install_from_repo_handler,
        crate::rest::sources::list_repos_handler,
        crate::rest::sources::add_repo_handler,
        crate::rest::sources::remove_repo_handler,
        crate::rest::sources::get_repo_handler,
        crate::rest::sources::list_repo_extensions_handler,
        crate::rest::sources::refresh_repo_handler,
        crate::rest::sources::set_browser_enabled,
        crate::rest::sources::set_download_concurrency,
        crate::rest::sources::update_from_repo_handler,
        // trackers
        crate::rest::trackers::get_manga_tracking_handler,
        crate::rest::trackers::set_manga_tracking_handler,
        crate::rest::trackers::list_trackers,
        crate::rest::trackers::get_tracker_auth_url,
        crate::rest::trackers::tracker_oauth_callback,
        crate::rest::trackers::unlink_tracker,
        crate::rest::trackers::search_tracker_manga,
        crate::rest::trackers::get_tracker_config,
        crate::rest::trackers::set_tracker_config,
        crate::rest::trackers::delete_tracker_config,
        crate::rest::trackers::get_tracker_mappings,
        crate::rest::trackers::set_tracker_mapping,
        crate::rest::trackers::delete_tracker_mapping,
        crate::rest::trackers::sync_all_trackers,
        crate::rest::trackers::sync_manga_trackers,
        // webhooks
        crate::rest::webhooks::list_webhooks,
        crate::rest::webhooks::create_webhook,
        crate::rest::webhooks::update_webhook,
        crate::rest::webhooks::delete_webhook,
        crate::rest::webhooks::test_webhook,
        crate::rest::webhooks::list_webhook_deliveries,
        crate::rest::webhooks::get_manga_webhook_notify,
        crate::rest::webhooks::set_manga_webhook_notify,
        // admin
        crate::rest::admin::server_stop,
        crate::rest::admin::server_restart,
        crate::rest::admin::admin_list_users,
        crate::rest::admin::admin_create_user,
        crate::rest::admin::admin_update_user,
        crate::rest::admin::admin_delete_user,
        crate::rest::admin::admin_grant_role,
        crate::rest::admin::admin_revoke_role,
        crate::rest::admin::admin_user_activity,
        crate::rest::admin::admin_list_roles,
        crate::rest::admin::admin_create_role,
        crate::rest::admin::admin_update_role,
        crate::rest::admin::admin_delete_role,
        crate::rest::admin::run_maintenance,
        crate::rest::admin::db_stats,
        crate::rest::admin::db_analyze,
        crate::rest::admin::db_vacuum,
        crate::rest::admin::clear_cache,
        crate::rest::admin::stop_scan,
        crate::rest::admin::admin_send_test_email,
        crate::rest::admin::admin_trigger_password_reset_handler,
        crate::rest::admin::get_credential_encryption_status_handler,
        crate::rest::admin::migrate_credentials_handler,
        crate::rest::admin::admin_logs,
        crate::rest::admin::admin_logs_stream,
        crate::rest::admin::admin_logs_download,
        crate::rest::admin::admin_purge_logs,
        crate::rest::admin::admin_audit_log,
        crate::rest::admin::admin_audit_log_download,
        crate::rest::admin::fs_browse_handler,
        crate::rest::admin::fs_mkdir_handler,
        crate::rest::admin::path_migrate_estimate_handler,
        crate::rest::admin::path_migrate_handler,
        crate::rest::admin::admin_backup_run_now,
        crate::rest::admin::admin_get_backup_schedule,
        crate::rest::admin::admin_put_backup_schedule,
        crate::rest::admin::list_blocked_repos_handler,
        crate::rest::admin::block_repo_handler,
        crate::rest::admin::delete_blocked_repo_handler,
        crate::rest::admin::admin_diagnostics,
        crate::rest::admin::admin_archive_export,
        crate::rest::admin::admin_archive_download,
        crate::rest::admin::admin_delete_orphans,
        crate::rest::admin::admin_library_scrub,
        crate::rest::admin::admin_library_scrub_last,
        crate::rest::admin::proxy_bandwidth_stats,
        crate::rest::admin::trigger_recurring,
        crate::rest::admin::list_source_circuits,
        crate::rest::admin::reset_source_circuit,
        crate::rest::admin::admin_storage_stats,
        crate::rest::admin::admin_storage_stats_history,
        crate::rest::admin::admin_support_bundle,
        // ui_themes
        crate::rest::ui_themes::list_themes,
        crate::rest::ui_themes::upsert_theme,
        crate::rest::ui_themes::deactivate_theme,
        crate::rest::ui_themes::delete_theme,
        crate::rest::ui_themes::activate_theme,
        // collections
        crate::rest::collections::list_collections,
        crate::rest::collections::create_collection,
        crate::rest::collections::delete_collection,
        crate::rest::collections::update_collection,
        crate::rest::collections::get_collection_manga,
        // filters
        crate::rest::filters::get_filter_artists,
        crate::rest::filters::get_filter_authors,
        crate::rest::filters::get_filter_tags,
        // jobs
        crate::rest::jobs::list_jobs,
        crate::rest::jobs::cancel_job,
        crate::rest::jobs::get_job,
        crate::rest::jobs::pause_job,
        crate::rest::jobs::resume_job,
        // volumes
        crate::rest::volumes::assign_chapter_volume,
        crate::rest::volumes::list_volumes,
        crate::rest::volumes::create_volume,
        crate::rest::volumes::delete_volume,
        crate::rest::volumes::update_volume,
        // saved_searches
        crate::rest::saved_searches::list_saved_searches,
        crate::rest::saved_searches::create_saved_search,
        crate::rest::saved_searches::delete_saved_search,
        crate::rest::saved_searches::update_saved_search,
        // sse
        crate::rest::sse::get_boot_id,
        // image + page serving (rest/mod.rs)
        crate::rest::serve_chapter_page,
        crate::rest::image_proxy,
        crate::rest::serve_manga_cover,
    ),
    components(
        schemas(
            crate::rest::api_tokens::TokenResponse,
            crate::rest::api_tokens::CreatedTokenResponse,
            crate::rest::api_tokens::CreateTokenBody,
            crate::models::LoginRequest,
            crate::models::SetChapterProgressRequest,
            crate::models::ChangePasswordRequest,
            crate::models::CreateSource,
            crate::models::UpdateSource,
            crate::models::FetchWasmRequest,
            crate::models::InstallYamlRequest,
            crate::models::FetchYamlRequest,
            crate::models::SearchMangaRequest,
            crate::models::UpdateLocalMetadataRequest,
            crate::models::AddDownloadRuleRequest,
            crate::models::UpdateDownloadRuleRequest,
            crate::models::ReorderDownloadRulesRequest,
            crate::models::PreviewDownloadRulesRequest,
            crate::models::ToggleEnabledRequest,
            crate::models::ToggleFavouritedRequest,
            crate::models::ToggleAutoDownloadRequest,
            crate::models::PreviewMigrationRequest,
            crate::models::MigrateMangaRequest,
            crate::models::MatchMigrationTargetsRequest,
            crate::models::MigrationMatchItem,
            crate::models::BulkMigrationRequest,
            crate::models::BulkMigrationRequestItem,
            crate::models::MigrationStatusRequest,
            crate::models::CreateCategoryRequest,
            crate::models::RenameCategoryRequest,
            crate::models::ReorderCategoriesRequest,
            crate::models::SetMangaCategoriesRequest,
            crate::models::SetScanlatorPrefRequest,
            crate::models::SetScanlatorModeRequest,
            crate::models::SetReadStatusRequest,
            crate::models::SetMangaTrackingRequest,
            crate::models::ToggleBookmarkRequest,
            crate::models::SetChapterNoteRequest,
            crate::models::MarkUpToRequest,
            crate::models::ScanMangaRequest,
            crate::models::SetPreferenceRequest,
            crate::models::ListItemRequest,
            crate::models::ToggleSelectRequest,
            crate::models::SetTrackerConfigRequest,
            crate::models::SetTrackerMappingRequest,
            crate::models::AdminCreateUserRequest,
            crate::models::AdminUpdateUserRequest,
            crate::models::AdminGrantRoleRequest,
            crate::models::AdminCreateRoleRequest,
            crate::models::AdminUpdateRoleRequest,
            crate::models::PasswordResetRequestBody,
            crate::models::PasswordResetConfirmBody,
            crate::models::SendTestEmailBody,
        crate::models::SolverTestBody,
            crate::rest::TotpCodeRequest,
            crate::rest::RegisterRequest,
        )
    ),
    tags(
        (name = "system", description = "Server info, first-run onboarding, and settings"),
        (name = "auth", description = "Authentication, sessions, TOTP, and password management"),
        (name = "library", description = "Library listing, scanning, backup/restore, and categories"),
        (name = "manga", description = "Per-manga operations, tracking, scanlators, and download rules"),
        (name = "chapters", description = "Chapter reading, progress, bookmarks, downloads, and exports"),
        (name = "sources", description = "Extension source management and browsing"),
        (name = "admin", description = "User/role administration, logs, audit, and server management"),
        (name = "api-tokens", description = "Long-lived bearer tokens for integrations"),
        (name = "ui", description = "Client-side themes and presentation preferences"),
    ),
)]
/// Generated OpenAPI document for every registered REST operation and schema.
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_covers_itself_and_its_children() {
        assert!(covers("/rest/admin", "/rest/admin"));
        assert!(covers("/rest/admin", "/rest/admin/users"));
        assert!(covers("/rest/admin", "/rest/admin/users/{id}"));
    }

    #[test]
    fn a_prefix_does_not_cover_a_longer_sibling_segment() {
        assert!(!covers("/rest/ui", "/rest/uikit"));
        assert!(!covers("/rest/jobs", "/rest/jobsearch"));
        assert!(!covers("/rest/admin", "/rest/administration"));
    }

    #[test]
    fn listed_prefixes_are_unstable_and_everything_else_is_stable() {
        assert_eq!(stability_for("/rest/admin/users"), Stability::Unstable);
        assert_eq!(stability_for("/rest/ui/themes"), Stability::Unstable);
        assert_eq!(stability_for("/rest/library"), Stability::Stable);
        assert_eq!(stability_for("/rest/manga/{id}"), Stability::Stable);
    }
}
