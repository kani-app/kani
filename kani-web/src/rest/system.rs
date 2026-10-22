use super::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/system/info", get(system_info))
        .route("/system/changelog", get(system_changelog))
        .route("/system/first-run-complete", post(complete_first_run))
        .route("/system/update", get(system_update))
}

/// The changelog is compiled in rather than read from disk: it ships with the
/// binary, so there is no path to get wrong and nothing to fail at runtime.
const CHANGELOG_MD: &str = include_str!("../../../CHANGELOG.md");

/// Number of leading sections to surface in the what's-new dialog. The full file
/// grows without bound; the dialog only wants the recent entries.
const CHANGELOG_MAX_SECTIONS: usize = 3;

/// Trims the changelog to its most recent `max_sections` `##` sections (keeping
/// any preamble above the first one).
fn recent_changelog(raw: &str, max_sections: usize) -> String {
    let mut out = String::new();
    let mut sections = 0usize;
    for line in raw.lines() {
        if line.starts_with("## ") {
            sections += 1;
            if sections > max_sections {
                break;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_owned()
}

#[utoipa::path(
    get,
    path = "/rest/system/changelog",
    responses(
        (status = 200, description = "Changelog rendered to sanitised HTML",
         body = inline(serde_json::Value),
         example = json!({ "version": "0.1.0", "html": "<h1>Changelog</h1>" }))
    ),
    tag = "system"
)]
pub(super) async fn system_changelog() -> impl IntoResponse {
    let md = recent_changelog(CHANGELOG_MD, CHANGELOG_MAX_SECTIONS);
    Json(json!({
        "version": crate::KANI_VERSION,
        "html": crate::utils::render_description(&md),
    }))
}

#[cfg(test)]
mod tests {
    use super::{CHANGELOG_MD, recent_changelog};

    #[test]
    fn recent_changelog_keeps_preamble_and_caps_sections() {
        let raw = "# Changelog\n\nintro\n\n## [0.3.0]\na\n\n## [0.2.0]\nb\n\n## [0.1.0]\nc\n";
        let out = recent_changelog(raw, 2);
        assert!(out.contains("intro"), "preamble is kept");
        assert!(out.contains("## [0.3.0]"));
        assert!(out.contains("## [0.2.0]"));
        assert!(!out.contains("## [0.1.0]"), "third section is trimmed");
    }

    #[test]
    fn recent_changelog_handles_fewer_sections_than_the_cap() {
        let raw = "# Changelog\n\n## [0.1.0]\nonly one\n";
        let out = recent_changelog(raw, 3);
        assert!(out.contains("only one"));
    }

    #[test]
    fn bundled_changelog_renders_to_html() {
        let md = recent_changelog(CHANGELOG_MD, 3);
        let html = crate::utils::render_description(&md);
        assert!(html.contains("<h1>") || html.contains("<h2>") || html.contains("<p>"));
    }
}

#[utoipa::path(
    get,
    path = "/rest/system/info",
    responses(
        (status = 200, description = "System information",
         body = inline(serde_json::Value),
         example = json!({
             "version": "0.1.0",
             "first_run": false,
             "oidc_available": false,
             "registration_enabled": true
         }))
    ),
    tag = "system"
)]
pub(super) async fn system_info(State(state): State<AppState>) -> impl IntoResponse {
    let s = state.get_settings().await;
    Json(json!({
        "version": crate::KANI_VERSION,
        "first_run": !s.first_run_complete,
        "oidc_available": std::env::var("KANI_OIDC_ISSUER").is_ok(),
        "registration_enabled": s.registration_enabled,
    }))
}

#[utoipa::path(
    post, path = "/rest/system/first-run-complete",
    responses(
        (status = 204, description = "First-run flag cleared"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Admin permission required"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn complete_first_run(
    _: AuthGuard<crate::permissions::guards::AdminManage>,
    State(state): State<AppState>,
    auth: AuthSession,
) -> Result<impl IntoResponse, AppError> {
    let user_id = auth
        .user
        .ok_or_else(|| AppError::Unauthorized("Not authenticated".into()))?
        .id;
    state.mark_first_run_complete(user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/rest/system/update",
    responses(
        (status = 200, description = "Current version and any available update"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("session" = [])),
    tag = "system"
)]
pub(super) async fn system_update(
    _: AuthGuard<crate::permissions::guards::Authenticated>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let current = kani_app::service::diagnostics::current_version();
    let latest = state.latest_version.read().await.clone();

    Ok(Json(json!({
        "current": current,
        "latest": latest.as_ref().map(|u| u.latest.clone()),
        "url": latest.as_ref().map(|u| u.url.clone()),
        "update_available": latest.is_some(),
    })))
}
