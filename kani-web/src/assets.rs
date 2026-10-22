//! Frontend assets, served from the binary or from disk.
//!
//! Release binaries embed the frontend so the archive remains self-contained.
//!
//! Three sources, most explicit first:
//!
//! 1. `KANI_STATIC_DIR` — always wins, so a deployment can override the built-in
//!    frontend without rebuilding.
//! 2. The embedded copy, on release builds.
//! 3. `./static` on disk, on debug builds, where `build.rs` does not run esbuild
//!    at all and the page loads raw modules through an import map. Reading from
//!    disk is what makes the edit-reload loop work.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

/// Assets staged by `build.rs` into `OUT_DIR/assets`.
///
/// Staged rather than read from `../static` because those files are generated:
/// cargo does not tie a recompile of this crate to files a build script writes
/// outside `OUT_DIR`, so embedding the source tree risks a binary whose frontend
/// is one build behind — a failure that is invisible, because everything works
/// and the UI is merely stale.
#[cfg(not(debug_assertions))]
#[derive(rust_embed::Embed)]
#[folder = "$OUT_DIR/assets/"]
struct Embedded;

/// Where the frontend comes from for the life of the process.
#[derive(Clone, Debug)]
pub enum Assets {
    /// An operator-supplied directory, or the working tree on a debug build.
    Disk(PathBuf),
    /// Compiled into the binary.
    Embedded,
}

/// One asset, plus what is needed to serve it conditionally.
pub struct Asset {
    pub bytes: Cow<'static, [u8]>,
    pub content_type: Option<String>,
    /// Strong validator. For embedded files this is the content hash rust-embed
    /// already computed; for disk reads it is derived from the bytes, so the two
    /// sources behave identically to a client.
    pub etag: String,
}

impl Assets {
    /// Resolves the source once, at startup, and says so in the log — a 404 for
    /// every page is otherwise very hard to attribute.
    pub fn resolve() -> Self {
        if let Ok(dir) = std::env::var("KANI_STATIC_DIR")
            && !dir.trim().is_empty()
        {
            tracing::info!("Serving static files from {dir} (KANI_STATIC_DIR)");
            return Self::Disk(PathBuf::from(dir));
        }
        #[cfg(not(debug_assertions))]
        {
            tracing::info!("Serving the frontend embedded in this binary");
            Self::Embedded
        }
        #[cfg(debug_assertions)]
        {
            tracing::info!("Serving static files from ./static (debug build)");
            Self::Disk(PathBuf::from("static"))
        }
    }

    /// Fetches an asset by its path relative to the static root.
    ///
    /// Returns `None` for anything that escapes the root, preventing traversal requests.
    pub fn get(&self, relative: &str) -> Option<Asset> {
        let clean = normalise(relative)?;
        match self {
            Self::Disk(root) => {
                let path = root.join(&clean);
                let bytes = std::fs::read(&path).ok()?;
                let etag = weak_hash(&bytes);
                Some(Asset {
                    content_type: mime_for(&clean),
                    bytes: Cow::Owned(bytes),
                    etag,
                })
            }
            #[cfg(not(debug_assertions))]
            Self::Embedded => {
                let file = Embedded::get(&clean)?;
                let etag = hex16(&file.metadata.sha256_hash()[..8]);
                Some(Asset {
                    content_type: file.metadata.mimetype().to_owned().into(),
                    bytes: file.data,
                    etag,
                })
            }
            #[cfg(debug_assertions)]
            Self::Embedded => None,
        }
    }
}

/// Rejects absolute paths and any traversal, and normalises separators so the
/// embedded lookup (which is always `/`-separated) matches on Windows too.
fn normalise(relative: &str) -> Option<String> {
    let trimmed = relative.trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => out.push(part.to_str()?.to_owned()),
            // `.` is harmless but pointless; everything else is an escape
            // attempt or a platform oddity we do not want to resolve.
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(out.join("/"))
}

fn mime_for(path: &str) -> Option<String> {
    Some(
        mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string(),
    )
}

fn weak_hash(bytes: &[u8]) -> String {
    // Not cryptographic — an ETag only has to change when the bytes do.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Only the embedded path needs this; a debug build has no embedded files.
#[cfg(not(debug_assertions))]
fn hex16(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Serves an asset, honouring `If-None-Match`.
///
/// Caching headers are not set here: `cache_control_middleware` already keys on
/// the `/js/` and `/css/` prefixes, and duplicating that would let the two
/// drift.
pub(crate) fn respond(asset: Option<Asset>, request_headers: &HeaderMap) -> Response {
    let Some(asset) = asset else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let quoted = format!("\"{}\"", asset.etag);
    if let Some(inm) = request_headers.get(header::IF_NONE_MATCH)
        && inm
            .to_str()
            .is_ok_and(|v| v.split(',').any(|c| c.trim() == quoted))
    {
        return StatusCode::NOT_MODIFIED.into_response();
    }

    let mut response = Response::builder().status(StatusCode::OK);
    if let Some(ct) = asset.content_type.as_deref()
        && let Ok(value) = HeaderValue::from_str(ct)
    {
        response = response.header(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = HeaderValue::from_str(&quoted) {
        response = response.header(header::ETAG, value);
    }
    response
        .body(Body::from(asset.bytes.into_owned()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Axum handler for a prefixed asset tree, e.g. `/js/{*path}` -> `js/<path>`.
pub async fn serve_prefixed(
    prefix: &'static str,
    assets: Assets,
    path: String,
    headers: HeaderMap,
) -> Response {
    respond(assets.get(&format!("{prefix}/{path}")), &headers)
}

/// Axum handler for a single named file.
pub async fn serve_named(name: &'static str, assets: Assets, headers: HeaderMap) -> Response {
    respond(assets.get(name), &headers)
}

/// State wrapper so handlers can be plain functions.
#[derive(Clone)]
pub struct AssetState(pub Assets);

pub async fn changelog(
    State(AssetState(assets)): State<AssetState>,
    headers: HeaderMap,
) -> Response {
    serve_named("changelog.md", assets, headers).await
}

#[cfg(test)]
mod tests {
    use super::{normalise, weak_hash};

    #[test]
    fn traversal_is_refused() {
        assert_eq!(normalise("../../etc/passwd"), None);
        assert_eq!(normalise("js/../../../etc/passwd"), None);
        assert_eq!(normalise("/etc/passwd"), Some("etc/passwd".to_owned()));
        assert_eq!(normalise(".."), None);
    }

    #[test]
    fn ordinary_paths_survive() {
        assert_eq!(
            normalise("js/dist/app.js"),
            Some("js/dist/app.js".to_owned())
        );
        assert_eq!(normalise("/css/main.css"), Some("css/main.css".to_owned()));
        assert_eq!(normalise("./sw.js"), Some("sw.js".to_owned()));
    }

    #[test]
    fn empty_is_not_a_path() {
        assert_eq!(normalise(""), None);
        assert_eq!(normalise("/"), None);
    }

    #[test]
    fn the_etag_tracks_the_bytes() {
        assert_eq!(weak_hash(b"alpha"), weak_hash(b"alpha"));
        assert_ne!(weak_hash(b"alpha"), weak_hash(b"beta"));
        assert_ne!(weak_hash(b"alpha"), weak_hash(b"alphb"));
    }
}
