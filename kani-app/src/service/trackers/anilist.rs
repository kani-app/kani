use super::{ExternalTracker, TokenResponse, TrackerMangaResult, TrackerMangaStatus};
use crate::error::{Result, ServiceError};
use kani_shared::types::MangaTrackingStatus;
use serde::Deserialize;

const AUTH_URL: &str = "https://anilist.co/api/v2/oauth/authorize";
const TOKEN_URL: &str = "https://anilist.co/api/v2/oauth/token";
const GRAPHQL_URL: &str = "https://graphql.anilist.co";

/// Endpoint set for the tracker. Defaults to AniList's real URLs; a test can
/// point them at a local origin via [`AnilistTracker::with_test_base`].
struct Endpoints {
    auth: String,
    token: String,
    graphql: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            auth: AUTH_URL.to_string(),
            token: TOKEN_URL.to_string(),
            graphql: GRAPHQL_URL.to_string(),
        }
    }
}

pub(crate) struct AnilistTracker {
    client_id: String,
    client_secret: String,
    http: rquest::Client,
    endpoints: Endpoints,
}

impl AnilistTracker {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret,
            http: super::tracker_http_client(),
            endpoints: Endpoints::default(),
        }
    }

    /// Test-only: point every endpoint at a local origin (`{base}/authorize`,
    /// `{base}/token`, `{base}/graphql`) and shorten the client timeout so a
    /// stalled-origin test resolves quickly.
    #[cfg(any(test, feature = "test-util"))]
    #[allow(
        dead_code,
        reason = "the seam exists for tracker tests plan 19 has not written yet"
    )]
    pub fn with_test_base(mut self, base: &str) -> Self {
        self.http = rquest::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()
            .unwrap_or_else(|_| rquest::Client::new());
        self.endpoints = Endpoints {
            auth: format!("{base}/authorize"),
            token: format!("{base}/token"),
            graphql: format!("{base}/graphql"),
        };
        self
    }

    fn map_status_to_anilist(status: MangaTrackingStatus) -> &'static str {
        match status {
            MangaTrackingStatus::Reading => "CURRENT",
            MangaTrackingStatus::OnHold => "PAUSED",
            MangaTrackingStatus::Dropped => "DROPPED",
            MangaTrackingStatus::PlanToRead => "PLANNING",
            MangaTrackingStatus::Completed => "COMPLETED",
            MangaTrackingStatus::Rereading => "REPEATING",
        }
    }

    fn map_status_from_anilist(status: &str) -> Option<MangaTrackingStatus> {
        match status {
            "CURRENT" => Some(MangaTrackingStatus::Reading),
            "PAUSED" => Some(MangaTrackingStatus::OnHold),
            "DROPPED" => Some(MangaTrackingStatus::Dropped),
            "PLANNING" => Some(MangaTrackingStatus::PlanToRead),
            "COMPLETED" => Some(MangaTrackingStatus::Completed),
            "REPEATING" => Some(MangaTrackingStatus::Rereading),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct AnilistTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Deserialize)]
struct GraphqlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphqlError>>,
}

#[derive(Deserialize)]
struct GraphqlError {
    message: String,
}

#[derive(Deserialize)]
struct SearchData {
    #[serde(rename = "Page")]
    page: SearchPage,
}

#[derive(Deserialize)]
struct SearchPage {
    media: Vec<SearchMedia>,
}

#[derive(Deserialize)]
struct SearchMedia {
    id: i64,
    title: MediaTitle,
    #[serde(rename = "coverImage")]
    cover_image: Option<CoverImage>,
}

#[derive(Deserialize)]
struct MediaTitle {
    #[serde(rename = "userPreferred")]
    user_preferred: Option<String>,
    romaji: Option<String>,
}

#[derive(Deserialize)]
struct CoverImage {
    large: Option<String>,
}

#[derive(Deserialize)]
struct MediaListData {
    #[serde(rename = "MediaList")]
    media_list: Option<MediaListEntry>,
}

#[derive(Deserialize)]
struct MediaListEntry {
    status: Option<String>,
    score: Option<f64>,
    progress: Option<i64>,
}

#[async_trait::async_trait]
impl ExternalTracker for AnilistTracker {
    fn name(&self) -> &'static str {
        "AniList"
    }

    fn auth_url(&self, redirect_uri: &str, state: &str, _code_challenge: Option<&str>) -> String {
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&state={}",
            self.endpoints.auth,
            urlencoding::encode(&self.client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(state),
        )
    }

    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        _code_verifier: Option<&str>,
    ) -> Result<TokenResponse> {
        let resp: AnilistTokenResponse = self
            .http
            .post(&self.endpoints.token)
            .json(&serde_json::json!({
                "grant_type": "authorization_code",
                "client_id": self.client_id,
                "client_secret": self.client_secret,
                "redirect_uri": redirect_uri,
                "code": code,
            }))
            .send()
            .await
            .map_err(|e| ServiceError::Internal(format!("AniList token exchange failed: {e}")))?
            .json()
            .await
            .map_err(|e| ServiceError::Internal(format!("AniList token parse failed: {e}")))?;

        let expires_at = resp
            .expires_in
            .map(|s| time::OffsetDateTime::now_utc() + time::Duration::seconds(s));

        Ok(TokenResponse {
            access_token: resp.access_token,
            refresh_token: resp.refresh_token,
            expires_at,
        })
    }

    async fn refresh_token(&self, _refresh_token: &str) -> Result<TokenResponse> {
        // AniList tokens are long-lived (1 year) and don't support refresh.
        // If the token is truly expired, the user must re-authenticate.
        Err(ServiceError::Internal(
            "AniList does not support token refresh — please re-link your account".into(),
        ))
    }

    async fn search_manga(
        &self,
        access_token: &str,
        query: &str,
    ) -> Result<Vec<TrackerMangaResult>> {
        let graphql = r#"
            query ($search: String) {
                Page(perPage: 10) {
                    media(search: $search, type: MANGA) {
                        id
                        title { userPreferred romaji }
                        coverImage { large }
                    }
                }
            }
        "#;

        let resp = self
            .http
            .post(&self.endpoints.graphql)
            .bearer_auth(access_token)
            .json(&serde_json::json!({
                "query": graphql,
                "variables": { "search": query }
            }))
            .send()
            .await
            .map_err(|e| ServiceError::Internal(format!("AniList search failed: {e}")))?;
        let resp: GraphqlResponse<SearchData> = super::check_tracker_response(resp, "AniList")?
            .json()
            .await
            .map_err(|e| ServiceError::Internal(format!("AniList search parse failed: {e}")))?;

        if let Some(errors) = resp.errors
            && let Some(first) = errors.first()
        {
            return Err(ServiceError::Internal(format!(
                "AniList API error: {}",
                first.message
            )));
        }

        let results = resp
            .data
            .map(|d| {
                d.page
                    .media
                    .into_iter()
                    .map(|m| TrackerMangaResult {
                        tracker_manga_id: m.id.to_string(),
                        title: m
                            .title
                            .user_preferred
                            .or(m.title.romaji)
                            .unwrap_or_default(),
                        cover_url: m.cover_image.and_then(|c| c.large),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(results)
    }

    async fn update_status(
        &self,
        access_token: &str,
        tracker_manga_id: &str,
        status: MangaTrackingStatus,
        score: Option<f64>,
        chapters_read: i64,
    ) -> Result<()> {
        let media_id: i64 = tracker_manga_id
            .parse()
            .map_err(|_| ServiceError::Internal("Invalid AniList media ID".into()))?;

        let graphql = r#"
            mutation ($mediaId: Int, $status: MediaListStatus, $score: Float, $progress: Int) {
                SaveMediaListEntry(mediaId: $mediaId, status: $status, score: $score, progress: $progress) {
                    id
                }
            }
        "#;

        let mut variables = serde_json::json!({
            "mediaId": media_id,
            "status": Self::map_status_to_anilist(status),
            "progress": chapters_read,
        });
        if let Some(s) = score {
            variables["score"] = serde_json::json!(s);
        }

        let resp = self
            .http
            .post(&self.endpoints.graphql)
            .bearer_auth(access_token)
            .json(&serde_json::json!({
                "query": graphql,
                "variables": variables,
            }))
            .send()
            .await
            .map_err(|e| ServiceError::Internal(format!("AniList update failed: {e}")))?;
        let resp = super::check_tracker_response(resp, "AniList")?;
        let resp: GraphqlResponse<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| ServiceError::Internal(format!("AniList update parse failed: {e}")))?;

        if let Some(errors) = resp.errors
            && let Some(first) = errors.first()
        {
            return Err(ServiceError::Internal(format!(
                "AniList API error: {}",
                first.message
            )));
        }

        Ok(())
    }

    async fn get_status(
        &self,
        access_token: &str,
        tracker_manga_id: &str,
    ) -> Result<TrackerMangaStatus> {
        let media_id: i64 = tracker_manga_id
            .parse()
            .map_err(|_| ServiceError::Internal("Invalid AniList media ID".into()))?;

        let graphql = r#"
            query ($mediaId: Int) {
                MediaList(mediaId: $mediaId, type: MANGA) {
                    status
                    score(format: POINT_10_DECIMAL)
                    progress
                }
            }
        "#;

        let resp = self
            .http
            .post(&self.endpoints.graphql)
            .bearer_auth(access_token)
            .json(&serde_json::json!({
                "query": graphql,
                "variables": { "mediaId": media_id }
            }))
            .send()
            .await
            .map_err(|e| ServiceError::Internal(format!("AniList get_status failed: {e}")))?;
        let resp = super::check_tracker_response(resp, "AniList")?;
        let resp: GraphqlResponse<MediaListData> = resp
            .json()
            .await
            .map_err(|e| ServiceError::Internal(format!("AniList get_status parse failed: {e}")))?;

        match resp.data.and_then(|d| d.media_list) {
            Some(entry) => Ok(TrackerMangaStatus {
                status: entry
                    .status
                    .as_deref()
                    .and_then(Self::map_status_from_anilist),
                score: entry.score,
                chapters_read: entry.progress.unwrap_or(0),
            }),
            None => Ok(TrackerMangaStatus {
                status: None,
                score: None,
                chapters_read: 0,
            }),
        }
    }
}
