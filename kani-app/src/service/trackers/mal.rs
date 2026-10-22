use super::{ExternalTracker, TokenResponse, TrackerMangaResult, TrackerMangaStatus};
use crate::error::{Result, ServiceError};
use kani_shared::types::MangaTrackingStatus;
use serde::Deserialize;

const AUTH_URL: &str = "https://myanimelist.net/v1/oauth2/authorize";
const TOKEN_URL: &str = "https://myanimelist.net/v1/oauth2/token";
const API_URL: &str = "https://api.myanimelist.net/v2";

/// Endpoint set for the tracker. Defaults to MAL's real URLs; a test can point
/// them at a local origin via [`MalTracker::with_test_base`].
struct Endpoints {
    auth: String,
    token: String,
    api: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            auth: AUTH_URL.to_string(),
            token: TOKEN_URL.to_string(),
            api: API_URL.to_string(),
        }
    }
}

pub struct MalTracker {
    client_id: String,
    http: rquest::Client,
    endpoints: Endpoints,
}

impl MalTracker {
    pub fn new(client_id: String) -> Self {
        Self {
            client_id,
            http: super::tracker_http_client(),
            endpoints: Endpoints::default(),
        }
    }

    /// Test-only: point every endpoint at a local origin (`{base}/authorize`,
    /// `{base}/token`, `{base}` for the API) and shorten the client timeout so a
    /// stalled-origin test resolves quickly.
    #[cfg(any(test, feature = "test-util"))]
    pub fn with_test_base(mut self, base: &str) -> Self {
        self.http = rquest::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()
            .unwrap_or_else(|_| rquest::Client::new());
        self.endpoints = Endpoints {
            auth: format!("{base}/authorize"),
            token: format!("{base}/token"),
            api: base.to_string(),
        };
        self
    }

    fn map_status_to_mal(status: MangaTrackingStatus) -> &'static str {
        // MAL represents rereading with its ordinary reading status.
        match status {
            MangaTrackingStatus::Reading => "reading",
            MangaTrackingStatus::OnHold => "on_hold",
            MangaTrackingStatus::Dropped => "dropped",
            MangaTrackingStatus::PlanToRead => "plan_to_read",
            MangaTrackingStatus::Completed => "completed",
            MangaTrackingStatus::Rereading => "reading",
        }
    }

    fn map_status_from_mal(status: &str) -> Option<MangaTrackingStatus> {
        match status {
            "reading" => Some(MangaTrackingStatus::Reading),
            "on_hold" => Some(MangaTrackingStatus::OnHold),
            "dropped" => Some(MangaTrackingStatus::Dropped),
            "plan_to_read" => Some(MangaTrackingStatus::PlanToRead),
            "completed" => Some(MangaTrackingStatus::Completed),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct MalTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Deserialize)]
struct MalSearchResponse {
    data: Vec<MalSearchNode>,
}

#[derive(Deserialize)]
struct MalSearchNode {
    node: MalMangaNode,
}

#[derive(Deserialize)]
struct MalMangaNode {
    id: i64,
    title: String,
    main_picture: Option<MalPicture>,
}

#[derive(Deserialize)]
struct MalPicture {
    large: Option<String>,
    medium: Option<String>,
}

#[derive(Deserialize)]
struct MalListStatus {
    status: Option<String>,
    score: Option<i64>,
    num_chapters_read: Option<i64>,
    is_rereading: Option<bool>,
}

#[derive(Deserialize)]
struct MalMangaDetail {
    my_list_status: Option<MalListStatus>,
}

#[async_trait::async_trait]
impl ExternalTracker for MalTracker {
    fn name(&self) -> &'static str {
        "MyAnimeList"
    }

    fn requires_pkce(&self) -> bool {
        true
    }

    /// `code_challenge` must be base64url(SHA-256(code_verifier)) — computed by the caller.
    fn auth_url(&self, redirect_uri: &str, state: &str, code_challenge: Option<&str>) -> String {
        let challenge = code_challenge.unwrap_or("");
        format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256",
            self.endpoints.auth,
            urlencoding::encode(&self.client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(state),
            urlencoding::encode(challenge),
        )
    }

    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        code_verifier: Option<&str>,
    ) -> Result<TokenResponse> {
        let verifier = code_verifier.unwrap_or("");
        let resp: MalTokenResponse = self
            .http
            .post(&self.endpoints.token)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier),
            ])
            .send()
            .await
            .map_err(|e| ServiceError::Internal(format!("MAL token exchange failed: {e}")))?
            .json()
            .await
            .map_err(|e| ServiceError::Internal(format!("MAL token parse failed: {e}")))?;

        let expires_at = resp
            .expires_in
            .map(|s| time::OffsetDateTime::now_utc() + time::Duration::seconds(s));

        Ok(TokenResponse {
            access_token: resp.access_token,
            refresh_token: resp.refresh_token,
            expires_at,
        })
    }

    async fn refresh_token(&self, refresh_token: &str) -> Result<TokenResponse> {
        let resp = self
            .http
            .post(&self.endpoints.token)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await
            .map_err(|e| ServiceError::Internal(format!("MAL token refresh failed: {e}")))?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ServiceError::TrackerAuthExpired(format!(
                "MyAnimeList refused to refresh the token (HTTP {code}): {body}"
            )));
        }
        let resp: MalTokenResponse = resp
            .json()
            .await
            .map_err(|e| ServiceError::Internal(format!("MAL token refresh parse failed: {e}")))?;

        let expires_at = resp
            .expires_in
            .map(|s| time::OffsetDateTime::now_utc() + time::Duration::seconds(s));

        Ok(TokenResponse {
            access_token: resp.access_token,
            refresh_token: resp.refresh_token,
            expires_at,
        })
    }

    async fn search_manga(
        &self,
        access_token: &str,
        query: &str,
    ) -> Result<Vec<TrackerMangaResult>> {
        let resp = self
            .http
            .get(format!("{}/manga", self.endpoints.api))
            .bearer_auth(access_token)
            .query(&[("q", query), ("limit", "10"), ("fields", "main_picture")])
            .send()
            .await
            .map_err(|e| ServiceError::Internal(format!("MAL search failed: {e}")))?;
        let resp: MalSearchResponse = super::check_tracker_response(resp, "MyAnimeList")?
            .json()
            .await
            .map_err(|e| ServiceError::Internal(format!("MAL search parse failed: {e}")))?;

        Ok(resp
            .data
            .into_iter()
            .map(|n| TrackerMangaResult {
                tracker_manga_id: n.node.id.to_string(),
                title: n.node.title,
                cover_url: n.node.main_picture.and_then(|p| p.large.or(p.medium)),
            })
            .collect())
    }

    async fn update_status(
        &self,
        access_token: &str,
        tracker_manga_id: &str,
        status: MangaTrackingStatus,
        score: Option<f64>,
        chapters_read: i64,
    ) -> Result<()> {
        let mut params = vec![
            (
                "status".to_string(),
                Self::map_status_to_mal(status).to_string(),
            ),
            ("num_chapters_read".to_string(), chapters_read.to_string()),
        ];

        if matches!(status, MangaTrackingStatus::Rereading) {
            params.push(("is_rereading".to_string(), "true".to_string()));
        }

        if let Some(s) = score {
            params.push(("score".to_string(), (s.round() as i64).to_string()));
        }

        let resp = self
            .http
            .patch(format!(
                "{}/manga/{}/my_list_status",
                self.endpoints.api, tracker_manga_id
            ))
            .bearer_auth(access_token)
            .form(&params)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(format!("MAL update failed: {e}")))?;

        let resp = super::check_tracker_response(resp, "MyAnimeList")?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ServiceError::Internal(format!("MAL update failed: {text}")));
        }

        Ok(())
    }

    async fn get_status(
        &self,
        access_token: &str,
        tracker_manga_id: &str,
    ) -> Result<TrackerMangaStatus> {
        let resp = self
            .http
            .get(format!("{}/manga/{}", self.endpoints.api, tracker_manga_id))
            .bearer_auth(access_token)
            .query(&[("fields", "my_list_status")])
            .send()
            .await
            .map_err(|e| ServiceError::Internal(format!("MAL get_status failed: {e}")))?;
        let resp = super::check_tracker_response(resp, "MyAnimeList")?;
        let resp: MalMangaDetail = resp
            .json()
            .await
            .map_err(|e| ServiceError::Internal(format!("MAL get_status parse failed: {e}")))?;

        match resp.my_list_status {
            Some(entry) => {
                let mut status = entry.status.as_deref().and_then(Self::map_status_from_mal);

                // If MAL says "reading" and is_rereading is true, map to Rereading.
                if matches!(status, Some(MangaTrackingStatus::Reading))
                    && entry.is_rereading == Some(true)
                {
                    status = Some(MangaTrackingStatus::Rereading);
                }

                Ok(TrackerMangaStatus {
                    status,
                    score: entry.score.map(|s| s as f64),
                    chapters_read: entry.num_chapters_read.unwrap_or(0),
                })
            }
            None => Ok(TrackerMangaStatus {
                status: None,
                score: None,
                chapters_read: 0,
            }),
        }
    }
}
