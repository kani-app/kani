use crate::service::AppService;

const RELEASES_URL: &str = "https://api.github.com/repos/kani-app/kani/releases/latest";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct UpdateInfo {
    pub latest: String,
    pub url: String,
}

pub(crate) fn normalise_tag(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.trim().trim_start_matches('v')).ok()
}

pub(crate) fn is_newer(latest: &str, current: &str) -> bool {
    match (normalise_tag(latest), normalise_tag(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

pub(crate) async fn check_for_update(
    client: &kani_core::http::SmartClient,
    current: &str,
) -> Option<UpdateInfo> {
    check_for_update_impl(client, current, RELEASES_URL).await
}

/// Test-only: run the update check against a chosen releases URL.
#[cfg(any(test, feature = "test-util"))]
pub async fn check_for_update_at(
    client: &kani_core::http::SmartClient,
    current: &str,
    releases_url: &str,
) -> Option<UpdateInfo> {
    check_for_update_impl(client, current, releases_url).await
}

async fn check_for_update_impl(
    client: &kani_core::http::SmartClient,
    current: &str,
    releases_url: &str,
) -> Option<UpdateInfo> {
    let response = client
        .get(releases_url)
        .await
        .inspect_err(|e| tracing::debug!("update check request failed: {e}"))
        .ok()?;

    let bytes = response
        .bytes()
        .await
        .inspect_err(|e| tracing::debug!("update check body read failed: {e}"))
        .ok()?;

    let body: serde_json::Value = serde_json::from_slice(&bytes)
        .inspect_err(|e| tracing::debug!("update check response was not JSON: {e}"))
        .ok()?;

    let tag = body.get("tag_name")?.as_str()?;
    if !is_newer(tag, current) {
        return None;
    }

    let url = body
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://github.com/kani-app/kani/releases/latest")
        .to_string();

    Some(UpdateInfo {
        latest: tag.trim_start_matches('v').to_string(),
        url,
    })
}

impl AppService {
    pub(crate) async fn run_update_check(&self) -> crate::error::Result<()> {
        if !self.settings.read().await.update_check_enabled {
            tracing::debug!("update check disabled by setting");
            return Ok(());
        }

        let current = crate::service::diagnostics::current_version();
        if let Some(info) = check_for_update(&self.smart_client, &current).await {
            tracing::info!("Kani {} is available (running {current})", info.latest);
            *self.latest_version.write().await = Some(info);
        } else {
            *self.latest_version.write().await = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn newer_tag_is_detected() {
        assert!(is_newer("0.9.1", "0.9.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("1.2.0", "v1.1.9"));
    }

    #[test]
    fn same_or_older_tag_is_not_an_update() {
        assert!(!is_newer("0.9.0", "0.9.0"));
        assert!(!is_newer("v0.9.0", "0.9.0"));
        assert!(!is_newer("0.8.9", "0.9.0"));
    }

    #[test]
    fn unparseable_tags_never_claim_an_update() {
        assert!(!is_newer("nightly", "0.9.0"));
        assert!(!is_newer("", "0.9.0"));
        assert!(!is_newer("0.9.1", "not-a-version"));
        assert!(!is_newer("v", "0.9.0"));
    }

    #[test]
    fn prerelease_is_older_than_its_release() {
        assert!(!is_newer("1.0.0-rc.1", "1.0.0"));
        assert!(is_newer("1.0.0", "1.0.0-rc.1"));
    }

    #[test]
    fn normalise_tag_strips_v_prefix() {
        assert_eq!(normalise_tag("v1.2.3").unwrap().to_string(), "1.2.3");
        assert_eq!(normalise_tag(" 1.2.3 ").unwrap().to_string(), "1.2.3");
        assert!(normalise_tag("garbage").is_none());
    }
}
