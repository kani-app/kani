//! OPDS catalog handlers — mounted at /opds in main.rs.

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::prelude::{BASE64_STANDARD, Engine as _};
use kani_app::ids::{ChapterId, MangaId};
use kani_app::permissions::{Opds, Permission};
use serde::Deserialize;
use std::io::SeekFrom;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::{
    auth::{AuthBackend, AuthSession, Credentials, User},
    error::AppError,
    state::AppState,
};
use axum_login::{AuthnBackend, AuthzBackend};

const ATOM_XML: &str = "application/atom+xml;profile=opds-catalog; charset=utf-8";
const CBZ_MIME: &str = "application/vnd.comicbook+zip";

pub fn routes(state: AppState) -> Router {
    Router::new()
        .route("/", get(opds_root))
        .route("/catalogue", get(opds_catalogue))
        .route("/manga/{id}", get(opds_manga))
        .route("/search", get(opds_search))
        .route("/opensearch", get(opds_opensearch))
        .route("/chapters/{id}", get(opds_chapter))
        .route("/chapters/{id}/page", get(opds_chapter_page))
        .route("/chapters/{id}/file", get(opds_chapter_file))
        .route("/chapters/{id}/progress", post(opds_set_progress))
        .with_state(state)
}

#[derive(Deserialize)]
struct CatalogueQuery {
    #[serde(default = "default_page")]
    page: i32,
    #[serde(default = "default_page_size")]
    page_size: i32,
    q: Option<String>,
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default = "default_page")]
    page: i32,
    q: Option<String>,
}

#[derive(Deserialize)]
struct PageQuery {
    page: usize,
    #[serde(default)]
    width: u32,
    format: Option<String>,
}

#[derive(Deserialize)]
struct ProgressBody {
    page: i64,
}

fn default_page() -> i32 {
    1
}
fn default_page_size() -> i32 {
    20
}

async fn opds_root(
    auth: AuthSession,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    let Some(identity) = opds_authenticate(&auth, &headers, &state).await else {
        return opds_401();
    };
    if !opds_allowed(&identity, Permission::Opds(Opds::Read), &state).await {
        return opds_403();
    }
    let base_url = base_url(&headers);
    let body = state.service.opds_root_feed(&base_url);
    atom_response(body)
}

async fn opds_catalogue(
    auth: AuthSession,
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<CatalogueQuery>,
) -> Response {
    let Some(identity) = opds_authenticate(&auth, &headers, &state).await else {
        return opds_401();
    };
    if !opds_allowed(&identity, Permission::Opds(Opds::Read), &state).await {
        return opds_403();
    }
    let base_url = base_url(&headers);
    match state
        .service
        .opds_catalogue_feed(q.page, q.page_size, q.q, identity.user.id, &base_url)
        .await
    {
        Ok(body) => atom_response(body),
        Err(e) => error_response(e),
    }
}

async fn opds_manga(
    auth: AuthSession,
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(manga_id): Path<MangaId>,
) -> Response {
    let Some(identity) = opds_authenticate(&auth, &headers, &state).await else {
        return opds_401();
    };
    if !opds_allowed(&identity, Permission::Opds(Opds::Read), &state).await {
        return opds_403();
    }
    let base_url = base_url(&headers);
    match state
        .service
        .opds_manga_feed(manga_id, identity.user.id, &base_url)
        .await
    {
        Ok(body) => atom_response(body),
        Err(e) => error_response(e),
    }
}

async fn opds_search(
    auth: AuthSession,
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Response {
    let Some(identity) = opds_authenticate(&auth, &headers, &state).await else {
        return opds_401();
    };
    if !opds_allowed(&identity, Permission::Opds(Opds::Read), &state).await {
        return opds_403();
    }
    let base_url = base_url(&headers);
    let query = q.q.unwrap_or_default();
    match state
        .service
        .opds_search_feed(&query, q.page, identity.user.id, &base_url)
        .await
    {
        Ok(body) => atom_response(body),
        Err(e) => error_response(e),
    }
}

async fn opds_opensearch(
    auth: AuthSession,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    let Some(identity) = opds_authenticate(&auth, &headers, &state).await else {
        return opds_401();
    };
    if !opds_allowed(&identity, Permission::Opds(Opds::Read), &state).await {
        return opds_403();
    }
    let base_url = base_url(&headers);
    let body = state.service.opds_opensearch_description(&base_url);
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "application/opensearchdescription+xml; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

async fn opds_chapter(
    auth: AuthSession,
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(chapter_id): Path<ChapterId>,
) -> Response {
    let Some(identity) = opds_authenticate(&auth, &headers, &state).await else {
        return opds_401();
    };
    if !opds_allowed(&identity, Permission::Opds(Opds::Read), &state).await {
        return opds_403();
    }
    let base_url = base_url(&headers);
    match state
        .service
        .opds_chapter_feed(chapter_id, identity.user.id, &base_url)
        .await
    {
        Ok(body) => atom_response(body),
        Err(e) => error_response(e),
    }
}

async fn opds_chapter_page(
    auth: AuthSession,
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(chapter_id): Path<ChapterId>,
    Query(q): Query<PageQuery>,
) -> Response {
    let Some(identity) = opds_authenticate(&auth, &headers, &state).await else {
        return opds_401();
    };
    if !opds_allowed(&identity, Permission::Opds(Opds::Read), &state).await {
        return opds_403();
    }

    let format = match q.format.as_deref() {
        None => None,
        Some("jpeg") | Some("jpg") => Some(image::ImageFormat::Jpeg),
        Some("webp") => Some(image::ImageFormat::WebP),
        Some(other) => {
            return AppError::ValidationError(format!("unsupported format: {other}"))
                .into_response();
        }
    };

    let fmt_tag = match format {
        None => "orig",
        Some(image::ImageFormat::WebP) => "webp",
        _ => "jpeg",
    };
    let etag = format!("\"{}-{}-{}-{}\"", chapter_id.0, q.page, q.width, fmt_tag);
    if let Some(inm) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && inm == etag
    {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag.as_str())]).into_response();
    }

    match state
        .service
        .opds_chapter_page(chapter_id, q.page, q.width, format)
        .await
    {
        Ok((bytes, content_type)) => {
            let mut hm = HeaderMap::new();
            hm.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            hm.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=86400"),
            );
            if let Ok(v) = HeaderValue::from_str(&etag) {
                hm.insert(header::ETAG, v);
            }
            (StatusCode::OK, hm, bytes).into_response()
        }
        Err(e) => error_response(e),
    }
}

async fn opds_chapter_file(
    auth: AuthSession,
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(chapter_id): Path<ChapterId>,
) -> Response {
    let Some(identity) = opds_authenticate(&auth, &headers, &state).await else {
        return opds_401();
    };
    if !opds_allowed(&identity, Permission::Opds(Opds::Read), &state).await {
        return opds_403();
    }

    let info = match state.service.chapter_cbz_path(chapter_id).await {
        Ok(info) => info,
        Err(e) => return error_response(e),
    };

    let mut file = match tokio::fs::File::open(&info.path).await {
        Ok(f) => f,
        Err(_) => {
            return AppError::NotFound("Chapter file not found".into()).into_response();
        }
    };
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let filename = format!(
        "{}.cbz",
        kani_core::utilities::sanitize_filename(&info.chapter_title)
    );
    let disposition = format!("attachment; filename=\"{filename}\"");

    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    if let Some(range) = range {
        match parse_byte_range(&range, len) {
            Some((start, end)) => {
                let to_read = end - start + 1;
                if file.seek(SeekFrom::Start(start)).await.is_err() {
                    return AppError::InternalServerError("seek failed".into()).into_response();
                }
                let stream = ReaderStream::new(file.take(to_read));
                let mut hm = HeaderMap::new();
                hm.insert(header::CONTENT_TYPE, HeaderValue::from_static(CBZ_MIME));
                hm.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
                if let Ok(v) = HeaderValue::from_str(&format!("bytes {start}-{end}/{len}")) {
                    hm.insert(header::CONTENT_RANGE, v);
                }
                if let Ok(v) = HeaderValue::from_str(&to_read.to_string()) {
                    hm.insert(header::CONTENT_LENGTH, v);
                }
                if let Ok(v) = HeaderValue::from_str(&disposition) {
                    hm.insert(header::CONTENT_DISPOSITION, v);
                }
                (StatusCode::PARTIAL_CONTENT, hm, Body::from_stream(stream)).into_response()
            }
            None => {
                let mut hm = HeaderMap::new();
                if let Ok(v) = HeaderValue::from_str(&format!("bytes */{len}")) {
                    hm.insert(header::CONTENT_RANGE, v);
                }
                (StatusCode::RANGE_NOT_SATISFIABLE, hm).into_response()
            }
        }
    } else {
        let stream = ReaderStream::new(file);
        let mut hm = HeaderMap::new();
        hm.insert(header::CONTENT_TYPE, HeaderValue::from_static(CBZ_MIME));
        hm.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        if let Ok(v) = HeaderValue::from_str(&len.to_string()) {
            hm.insert(header::CONTENT_LENGTH, v);
        }
        if let Ok(v) = HeaderValue::from_str(&disposition) {
            hm.insert(header::CONTENT_DISPOSITION, v);
        }
        (StatusCode::OK, hm, Body::from_stream(stream)).into_response()
    }
}

async fn opds_set_progress(
    auth: AuthSession,
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(chapter_id): Path<ChapterId>,
    Json(body): Json<ProgressBody>,
) -> Response {
    let Some(identity) = opds_authenticate(&auth, &headers, &state).await else {
        return opds_401();
    };
    if !opds_allowed(&identity, Permission::Opds(Opds::Progress), &state).await {
        return opds_403();
    }
    // The reader reports a PSE page number; progress is stored as a 0-based index.
    let page_index = match state
        .service
        .opds_page_to_index(body.page.max(0) as usize)
        .await
    {
        Ok(i) => i as i64,
        Err(e) => return error_response(e),
    };
    match state
        .service
        .set_chapter_progress(identity.user.id, chapter_id, page_index)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

struct OpdsIdentity {
    user: User,
    scopes: OpdsScopes,
}

enum OpdsScopes {
    /// Full session/password identity — permissions resolved via RBAC roles.
    Full,
    /// API-token identity — carries exactly the token's granted scopes.
    Token(Vec<Permission>),
}

/// Resolves an OPDS request to an identity: session cookie, `Bearer` token, or
/// `Basic` (password, or an API token pasted as the password). Returns `None`
/// (caller emits 401) when no credential succeeds.
async fn opds_authenticate(
    auth: &AuthSession,
    headers: &HeaderMap,
    state: &AppState,
) -> Option<OpdsIdentity> {
    if let Some(user) = &auth.user
        && user.is_active
    {
        return Some(OpdsIdentity {
            user: user.clone(),
            scopes: OpdsScopes::Full,
        });
    }

    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;

    if let Some(token) = value.strip_prefix("Bearer ") {
        let token_auth = state.service.authenticate_api_token(token).await.ok()??;
        let backend = AuthBackend::new(state.service.db.clone());
        let user = backend.fetch_user_by_id(token_auth.user_id).await.ok()??;
        if !user.is_active {
            return None;
        }
        return Some(OpdsIdentity {
            user,
            scopes: OpdsScopes::Token(token_auth.scopes),
        });
    }

    let encoded = value.strip_prefix("Basic ")?;
    let decoded = BASE64_STANDARD.decode(encoded).ok()?;
    let decoded_str = std::str::from_utf8(&decoded).ok()?;
    let (username, password) = decoded_str.split_once(':')?;

    if password.starts_with("kani_") {
        let token_auth = state
            .service
            .authenticate_api_token(password)
            .await
            .ok()??;
        let backend = AuthBackend::new(state.service.db.clone());
        let user = backend.fetch_user_by_id(token_auth.user_id).await.ok()??;
        if !user.is_active || user.username != username {
            return None;
        }
        return Some(OpdsIdentity {
            user,
            scopes: OpdsScopes::Token(token_auth.scopes),
        });
    }

    let backend = AuthBackend::new(state.service.db.clone());
    let creds = Credentials {
        username: username.to_owned(),
        password: secrecy::Secret::new(password.to_owned()),
    };
    let user = backend.authenticate(creds).await.ok()??;
    if !user.is_active {
        return None;
    }
    Some(OpdsIdentity {
        user,
        scopes: OpdsScopes::Full,
    })
}

/// Returns whether the identity may exercise `perm`. Token identities carry an
/// explicit scope list; full identities are checked against their RBAC roles.
async fn opds_allowed(identity: &OpdsIdentity, perm: Permission, state: &AppState) -> bool {
    match &identity.scopes {
        OpdsScopes::Token(scopes) => scopes.contains(&perm),
        OpdsScopes::Full => {
            let backend = AuthBackend::new(state.service.db.clone());
            backend
                .has_perm(&identity.user, perm)
                .await
                .unwrap_or(false)
        }
    }
}

/// Parses a single-range HTTP `Range: bytes=start-end` header against `len`.
/// Returns the inclusive, clamped `(start, end)` byte offsets, or `None` when
/// the header is malformed, multi-range, or entirely beyond `len`.
fn parse_byte_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start_s, end_s) = spec.split_once('-')?;
    let start_s = start_s.trim();
    let end_s = end_s.trim();

    if len == 0 {
        return None;
    }

    let (start, end) = if start_s.is_empty() {
        // Suffix range: last N bytes.
        let n: u64 = end_s.parse().ok()?;
        if n == 0 {
            return None;
        }
        let n = n.min(len);
        (len - n, len - 1)
    } else {
        let start: u64 = start_s.parse().ok()?;
        let end = if end_s.is_empty() {
            len - 1
        } else {
            end_s.parse::<u64>().ok()?.min(len - 1)
        };
        (start, end)
    };

    if start > end || start >= len {
        return None;
    }
    Some((start, end))
}

fn atom_response(body: String) -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, ATOM_XML)], body).into_response()
}

fn opds_401() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"Kani\"")],
        "",
    )
        .into_response()
}

fn opds_403() -> Response {
    (StatusCode::FORBIDDEN, "").into_response()
}

fn error_response(e: kani_app::error::ServiceError) -> Response {
    let app_err: AppError = e.into();
    app_err.into_response()
}

/// Best-effort base URL derived from request headers.
fn base_url(headers: &HeaderMap) -> String {
    // X-Forwarded-Proto / X-Forwarded-Host set by reverse proxies
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:8242");
    format!("{proto}://{host}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::parse_byte_range;

    #[test]
    fn parses_closed_range() {
        assert_eq!(parse_byte_range("bytes=0-99", 1000), Some((0, 99)));
    }

    #[test]
    fn clamps_end_to_length() {
        assert_eq!(parse_byte_range("bytes=0-99", 50), Some((0, 49)));
    }

    #[test]
    fn open_ended_range_reads_to_end() {
        assert_eq!(parse_byte_range("bytes=500-", 1000), Some((500, 999)));
    }

    #[test]
    fn suffix_range_returns_last_n() {
        assert_eq!(parse_byte_range("bytes=-100", 1000), Some((900, 999)));
    }

    #[test]
    fn suffix_larger_than_len_is_whole_file() {
        assert_eq!(parse_byte_range("bytes=-5000", 1000), Some((0, 999)));
    }

    #[test]
    fn rejects_multi_range() {
        assert_eq!(parse_byte_range("bytes=0-10,20-30", 1000), None);
    }

    #[test]
    fn rejects_start_beyond_len() {
        assert_eq!(parse_byte_range("bytes=2000-3000", 1000), None);
    }

    #[test]
    fn rejects_missing_prefix() {
        assert_eq!(parse_byte_range("0-99", 1000), None);
    }

    #[test]
    fn rejects_zero_length_resource() {
        assert_eq!(parse_byte_range("bytes=0-0", 0), None);
    }
}
