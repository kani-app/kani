//! Concurrent, resumable chapter download orchestration and CBZ assembly.

use crate::error::{self, Result};
use crate::http::{SmartClient, SmartResponse};
use crate::utilities::{assert_within_root, sanitize_filename};
use async_zip::tokio::write::ZipFileWriter;
use async_zip::{Compression, ZipEntryBuilder};
use futures::stream::{self, StreamExt};
use kani_shared::DownloadProgressEvent;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;
use tokio_util::compat::{FuturesAsyncWriteCompatExt, TokioAsyncWriteCompatExt};
use tokio_util::sync::CancellationToken;

/// Error type returned by [`DownloaderManager::download_chapter_direct`].
#[derive(Debug)]
pub enum DownloadError {
    PageFetch(String),
    /// A page failed with a specific HTTP status. Kept structured so retry
    /// policy is a numeric decision rather than a substring search over
    /// formatted error text.
    PageHttp {
        status: u16,
        retry_after_secs: Option<u64>,
        message: String,
    },
    Io(std::io::Error),
    Extension {
        kind: kani_shared::extension::ExtensionErrorKind,
        message: String,
        /// Source-provided `Retry-After` used by the host retry policy.
        retry_after_secs: Option<u32>,
    },
    Cancelled,
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PageFetch(msg) => write!(f, "page fetch failed: {msg}"),
            Self::PageHttp {
                status, message, ..
            } => write!(f, "page fetch failed with HTTP {status}: {message}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Extension { message, .. } => write!(f, "extension error: {message}"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl DownloadError {
    fn from_page_list_error(e: crate::error::Error) -> Self {
        match e {
            crate::error::Error::Extension(ext) => Self::Extension {
                kind: ext.kind,
                message: ext.message,
                retry_after_secs: ext.retry_after_secs,
            },
            other => Self::Extension {
                kind: kani_shared::extension::ExtensionErrorKind::Unknown,
                message: other.to_string(),
                retry_after_secs: None,
            },
        }
    }
}

/// Successful outcome of [`DownloaderManager::download_chapter_direct`].
pub struct DownloadOutcome {
    pub successful_pages: usize,
}

const PROGRESS_CHANNEL_CAPACITY: usize = 512;

const TERMINAL_STATE_TTL_SECS: u64 = 30;

#[async_trait::async_trait]
/// Supplies a chapter's remote page list and base URL to the downloader.
/// Implementations may cross the extension boundary and must preserve source page order.
pub trait PageListFetcher: Send + Sync {
    async fn fetch_page_list(
        &self,
        manga_id: &str,
        chapter_id: &str,
    ) -> Result<(crate::wasm::kani::extension::types::Chapter, String)>;
}

/// Complete input required to download and assemble one chapter.
pub struct DownloadTask {
    pub chapter_id: i64,
    pub manga_id: i64,
    pub manga_title: String,
    pub source_manager: Arc<dyn PageListFetcher>,
    pub source_manga_id: String,
    pub source_chapter_id: String,
    pub name: String,
    pub library_path: PathBuf,
    pub save_path: PathBuf,
    pub comic_info: Option<crate::comic_info::ComicInfo>,
    /// The source's own client when it differs from the manager's, e.g. one honouring a
    /// local-network grant. `None` uses the manager's client.
    pub http_client: Option<SmartClient>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
/// Snapshot broadcast while a chapter download is active or briefly retained after termination.
pub struct ActiveDownloadState {
    pub chapter_id: i64,
    pub chapter_name: String,
    pub manga_id: i64,
    pub manga_title: String,
    pub total_pages: usize,
    pub completed_pages: usize,
    pub status: ActiveDownloadStatus,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
/// Observable lifecycle state of an active or recently terminated download.
pub enum ActiveDownloadStatus {
    InProgress,
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Clone)]
/// Coordinates chapter downloads and broadcasts progress snapshots.
/// Terminal entries remain observable for a short TTL before removal.
pub struct DownloaderManager {
    progress_tx: broadcast::Sender<DownloadProgressEvent>,
    active: Arc<tokio::sync::RwLock<HashMap<i64, ActiveDownloadState>>>,
    smart_client: SmartClient,
    concurrent_pages: usize,
    max_attempts: i64,
    initial_retry_delay_ms: i64,
}

impl DownloaderManager {
    pub fn subscribe(&self) -> broadcast::Receiver<DownloadProgressEvent> {
        self.progress_tx.subscribe()
    }

    /// Announce a finished chapter. The caller owns the timing: a subscriber
    /// re-reads the chapter as soon as this lands, so it must not be sent until
    /// the row records the download.
    pub fn emit_chapter_completed(
        &self,
        chapter_id: i64,
        chapter_name: String,
        manga_id: i64,
        manga_title: String,
        successful_pages: usize,
    ) {
        let _ = self
            .progress_tx
            .send(DownloadProgressEvent::ChapterCompleted {
                chapter_id,
                chapter_name,
                manga_id,
                manga_title,
                successful_pages,
            });
    }
}

pub(crate) const DEFAULT_CONCURRENT_PAGES: usize = 4;
pub(crate) const DEFAULT_MAX_ATTEMPTS: i64 = 3;
/// Default delay before the first page retry, in milliseconds.
pub(crate) const DEFAULT_INITIAL_RETRY_DELAY_MS: i64 = 1_000;

#[derive(Debug, Clone)]
/// Page-level concurrency and retry policy for a [`DownloaderManager`].
pub struct DownloaderConfig {
    /// Maximum page requests in flight within one chapter download.
    pub concurrent_pages: usize,
    /// Maximum attempts per page, including the initial request.
    pub max_attempts: i64,
    /// Delay before the first retry, in milliseconds; later retries back off exponentially.
    pub initial_retry_delay_ms: i64,
}

impl Default for DownloaderConfig {
    fn default() -> Self {
        Self {
            concurrent_pages: DEFAULT_CONCURRENT_PAGES,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            initial_retry_delay_ms: DEFAULT_INITIAL_RETRY_DELAY_MS,
        }
    }
}

impl DownloaderManager {
    pub async fn new(smart_client: SmartClient, config: DownloaderConfig) -> Result<Self> {
        let (progress_tx, _) = broadcast::channel(PROGRESS_CHANNEL_CAPACITY);
        let active = Arc::new(tokio::sync::RwLock::new(HashMap::new()));

        Ok(Self {
            progress_tx,
            active,
            smart_client,
            concurrent_pages: config.concurrent_pages,
            max_attempts: config.max_attempts,
            initial_retry_delay_ms: config.initial_retry_delay_ms,
        })
    }

    pub async fn snapshot(&self) -> Vec<ActiveDownloadState> {
        self.active.read().await.values().cloned().collect()
    }

    /// Send an event on the progress broadcast channel.
    fn send_event(tx: &broadcast::Sender<DownloadProgressEvent>, event: DownloadProgressEvent) {
        let _ = tx.send(event);
    }

    async fn create_cbz(
        cbz_path: &std::path::Path,
        mut staged_pages: Vec<(PathBuf, String)>,
        comic_info: Option<crate::comic_info::ComicInfo>,
    ) -> Result<()> {
        // Zip entry order and spread detection must use the same display order.
        staged_pages.sort_unstable_by(|a, b| a.1.cmp(&b.1));

        let comic_info = if let Some(mut info) = comic_info {
            let paths: Vec<PathBuf> = staged_pages.iter().map(|(p, _)| p.clone()).collect();
            let spread_flags =
                tokio::task::spawn_blocking(move || crate::cbz::detect_spread_pages(&paths))
                    .await
                    .ok()
                    .unwrap_or_default();
            info.pages = Some(crate::comic_info::ComicPages::from_flags(
                staged_pages.len(),
                &spread_flags,
            ));
            Some(info)
        } else {
            None
        };

        let cbz_file = tokio::fs::File::create(cbz_path).await?;
        let mut zip_writer = ZipFileWriter::new(cbz_file.compat_write());

        if let Some(info) = comic_info {
            let xml_content = crate::comic_info::build_xml(&info)?;
            let comic_info_builder =
                ZipEntryBuilder::new("ComicInfo.xml".into(), Compression::Stored);
            zip_writer
                .write_entry_whole(comic_info_builder, xml_content.as_bytes())
                .await
                .map_err(|e| crate::error::Error::Other(format!("Zip error: {}", e)))?;
        }

        for (tmp_path, filename) in staged_pages {
            let mut tmp_file = tokio::fs::File::open(&tmp_path).await?;
            let entry_builder = ZipEntryBuilder::new(filename.into(), Compression::Stored);
            let mut entry_writer = zip_writer
                .write_entry_stream(entry_builder)
                .await
                .map_err(|e| crate::error::Error::Other(format!("Zip error: {}", e)))?;

            tokio::io::copy(&mut tmp_file, &mut (&mut entry_writer).compat_write()).await?;

            entry_writer
                .close()
                .await
                .map_err(|e| crate::error::Error::Other(format!("Zip error: {}", e)))?;
        }

        let mut file = zip_writer
            .close()
            .await
            .map_err(|e| crate::error::Error::Other(format!("Zip error: {}", e)))?
            .into_inner();
        file.flush().await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    /// `Retry-After` as whole seconds. Only the delta-seconds form is read; the
    /// HTTP-date form is rare in practice and guessing at clock skew is worse than
    /// falling back to our own backoff.
    fn retry_after_secs(headers: &rquest::header::HeaderMap) -> Option<u64> {
        headers
            .get(rquest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
    }

    /// Statuses no amount of retrying will change.
    fn is_permanent_http(e: &error::Error) -> bool {
        matches!(
            e,
            error::Error::HttpStatus { status, .. }
                if matches!(status, 400 | 401 | 403 | 404 | 405 | 410 | 451)
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn download_page_with_retry(
        client: &SmartClient,
        url: &str,
        page_index: i32,
        staging_dir: &std::path::Path,
        max_attempts: i64,
        initial_retry_delay_ms: i64,
        base_url: &str,
        transform: Option<&str>,
    ) -> Result<(PathBuf, String)> {
        let mut attempts = 0i64;
        let mut delay = Duration::from_millis(initial_retry_delay_ms.try_into()?);
        const MAX_DELAY: Duration = Duration::from_secs(30);

        loop {
            match Self::download_page(client, url, page_index, staging_dir, base_url, transform)
                .await
            {
                Ok(data) => return Ok(data),
                Err(e) => {
                    // Permanent HTTP failures bypass retries so one bad page cannot amplify
                    // the per-attempt backoff across the entire chapter.
                    if Self::is_permanent_http(&e) {
                        tracing::warn!("Page {page_index} failed permanently: {e}");
                        return Err(e);
                    }
                    attempts += 1;
                    if attempts >= max_attempts {
                        tracing::error!(
                            "Failed to download page {} after {} attempts: {}",
                            page_index,
                            attempts,
                            e
                        );
                        return Err(e);
                    }

                    tracing::warn!(
                        "Retry {}/{} for page {} after error: {}",
                        attempts,
                        max_attempts,
                        page_index,
                        e
                    );

                    delay = (delay * 2).min(MAX_DELAY);
                    let jitter_frac: f64 = rand::rng().random_range(-0.25..=0.25);
                    let jittered = Duration::from_secs_f64(
                        (delay.as_secs_f64() * (1.0 + jitter_frac)).max(0.0),
                    );
                    tokio::time::sleep(jittered).await;
                }
            }
        }
    }

    /// Test hook for `download_page_with_retry`.
    #[cfg(any(test, feature = "test-util"))]
    #[allow(clippy::too_many_arguments)]
    pub async fn download_page_with_retry_for_test(
        client: &SmartClient,
        url: &str,
        page_index: i32,
        staging_dir: &std::path::Path,
        max_attempts: i64,
        initial_retry_delay_ms: i64,
        base_url: &str,
        transform: Option<&str>,
    ) -> Result<(PathBuf, String)> {
        Self::download_page_with_retry(
            client,
            url,
            page_index,
            staging_dir,
            max_attempts,
            initial_retry_delay_ms,
            base_url,
            transform,
        )
        .await
    }

    /// Test hook for `download_page`, which is otherwise reachable only through
    /// a whole chapter download.
    #[cfg(any(test, feature = "test-util"))]
    pub async fn download_page_for_test(
        client: &SmartClient,
        url: &str,
        page: i32,
        staging_dir: &std::path::Path,
        referer: &str,
        transform: Option<&str>,
    ) -> Result<(PathBuf, String)> {
        Self::download_page(client, url, page, staging_dir, referer, transform).await
    }

    async fn download_page(
        client: &SmartClient,
        url: &str,
        page: i32,
        staging_dir: &std::path::Path,
        referer: &str,
        transform: Option<&str>,
    ) -> Result<(PathBuf, String)> {
        let request = client
            .inner()
            .get(url)
            .header(rquest::header::REFERER, referer)
            .build()
            .map_err(|e| crate::error::Error::Other(e.to_string()))?;

        let mut resp = client.send_request(request).await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(error::Error::HttpStatus {
                status: status.as_u16(),
                retry_after_secs: Self::retry_after_secs(resp.headers()),
                context: format!("downloading page {page}"),
            });
        }

        let resolved = transform.and_then(|hint| {
            crate::transform::registry().resolve(
                hint,
                crate::transform::TransformKind::Image,
                resp.headers(),
            )
        });

        let announced = resp
            .headers()
            .get(rquest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        // The first chunk is read before the name is chosen, so a body whose
        // Content-Type and URL both say nothing can still be identified from
        // its magic bytes.
        let first_chunk = resp.chunk().await?;
        let (extension, filename) = if let Some(r) = &resolved {
            let ext = r.output().file_extension;
            (ext, format!("{:04}.{}", page, ext))
        } else {
            let ext = Self::get_image_extension_sniffed(
                &resp,
                url,
                first_chunk.as_deref().unwrap_or(&[]),
            );
            (ext, format!("{:04}.{}", page, ext))
        };
        let tmp_file_path = staging_dir.join(format!("{:04}.{}.tmp", page, extension));
        let part_file_path = staging_dir.join(format!("{:04}.{}.part", page, extension));

        if let Some(r) = &resolved {
            let mut raw: Vec<u8> = Vec::new();
            if let Some(ref c) = first_chunk {
                raw.extend_from_slice(c);
            }
            while let Some(chunk) = resp.chunk().await? {
                raw.extend_from_slice(&chunk);
            }
            if raw.is_empty() {
                return Err(error::Error::Other(format!(
                    "Server returned empty body for page {page}"
                )));
            }
            Self::check_complete(announced, raw.len() as u64, page)?;
            let descrambled = r.apply(&raw)?;
            let mut file = tokio::fs::File::create(&part_file_path).await?;
            file.write_all(&descrambled).await?;
            file.flush().await?;
        } else {
            let mut file = tokio::fs::File::create(&part_file_path).await?;
            let mut bytes_written: u64 = 0;
            if let Some(ref c) = first_chunk {
                bytes_written += c.len() as u64;
                file.write_all(c).await?;
            }
            while let Some(chunk) = resp.chunk().await? {
                bytes_written += chunk.len() as u64;
                file.write_all(&chunk).await?;
            }
            file.flush().await?;
            if bytes_written == 0 {
                return Err(error::Error::Other(format!(
                    "Server returned empty body for page {page}"
                )));
            }
            Self::check_complete(announced, bytes_written, page)?;
        }

        tokio::fs::rename(&part_file_path, &tmp_file_path).await?;

        Ok((tmp_file_path, filename))
    }

    /// Rejects a body that stopped short of the length the server announced.
    ///
    /// A silently truncated image still decodes to *something* often enough
    /// that nothing downstream notices, so the mismatch has to be caught here
    /// where the promise and the delivery are both in hand.
    fn check_complete(announced: Option<u64>, received: u64, page: i32) -> Result<()> {
        match announced {
            Some(expected) if received < expected => Err(error::Error::Other(format!(
                "Truncated body for page {page}: got {received} of {expected} bytes"
            ))),
            _ => Ok(()),
        }
    }

    /// Extension for a page, from the Content-Type, then the URL, then the
    /// first bytes.
    ///
    /// The byte-sniff matters: a source serving
    /// `Content-Type: application/octet-stream` from an extensionless URL used
    /// to fall through to `jpg`, so a PNG was stored inside the CBZ as
    /// `0001.jpg`. Readers mostly cope, but the manifest records a name that
    /// contradicts the bytes.
    fn get_image_extension_sniffed(resp: &SmartResponse, url: &str, prefix: &[u8]) -> &'static str {
        let by_header_or_url = Self::get_image_extension(resp, url);
        // Only second-guess the fallback, never a definite answer.
        if by_header_or_url != "jpg" || Self::looks_like_jpeg(prefix) {
            return by_header_or_url;
        }
        Self::sniff_extension(prefix).unwrap_or(by_header_or_url)
    }

    fn looks_like_jpeg(b: &[u8]) -> bool {
        b.starts_with(&[0xFF, 0xD8])
    }

    fn sniff_extension(b: &[u8]) -> Option<&'static str> {
        if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            return Some("png");
        }
        if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
            return Some("gif");
        }
        if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
            return Some("webp");
        }
        if b.len() >= 12 && &b[4..8] == b"ftyp" && (&b[8..12] == b"avif" || &b[8..12] == b"avis") {
            return Some("avif");
        }
        None
    }

    fn get_image_extension(resp: &SmartResponse, url: &str) -> &'static str {
        if let Some(ct) = resp
            .headers()
            .get("content-type")
            .and_then(|c| c.to_str().ok())
        {
            let base = ct.split(';').next().unwrap_or(ct).trim();
            match base {
                "image/jpeg" | "image/jpg" => return "jpg",
                "image/png" => return "png",
                "image/webp" => return "webp",
                "image/gif" => return "gif",
                "image/avif" => return "avif",
                _ => {}
            }
        }

        let clean_url = url.split('?').next().unwrap_or(url);

        if let Some(ext) = clean_url.rsplit('.').next() {
            match ext.to_lowercase().as_str() {
                "jpg" | "jpeg" => return "jpg",
                "png" => return "png",
                "webp" => return "webp",
                "gif" => return "gif",
                "avif" => return "avif",
                _ => {}
            }
        }

        "jpg"
    }

    async fn update_active(
        active: &Arc<tokio::sync::RwLock<HashMap<i64, ActiveDownloadState>>>,
        chapter_id: i64,
        f: impl FnOnce(&mut ActiveDownloadState),
    ) {
        let mut map = active.write().await;
        if let Some(s) = map.get_mut(&chapter_id) {
            f(s);
        }
    }

    pub async fn download_chapter_direct(
        &self,
        task: DownloadTask,
        cancel: CancellationToken,
        job_id: Option<String>,
        on_page: impl Fn(u64, u64) + Send + Sync + 'static,
    ) -> std::result::Result<DownloadOutcome, DownloadError> {
        let concurrent_pages = self.concurrent_pages;
        let max_attempts = self.max_attempts;
        let initial_retry_delay_ms = self.initial_retry_delay_ms;
        let DownloadTask {
            chapter_id,
            manga_id,
            manga_title,
            source_manager,
            source_manga_id,
            source_chapter_id,
            name,
            library_path,
            save_path,
            comic_info,
            http_client,
        } = task;

        let safe_name = sanitize_filename(&name);

        tokio::fs::create_dir_all(&save_path)
            .await
            .map_err(DownloadError::Io)?;

        let cbz_path =
            assert_within_root(&library_path, &save_path.join(format!("{}.cbz", safe_name)))
                .map_err(|e| DownloadError::PageFetch(e.to_string()))?;

        let staging_dir = assert_within_root(
            &library_path,
            &save_path.join(format!(".tmp_staging_{}", chapter_id)),
        )
        .map_err(|e| DownloadError::PageFetch(e.to_string()))?;

        tokio::fs::create_dir_all(&staging_dir)
            .await
            .map_err(DownloadError::Io)?;

        let fetch_fut = source_manager.fetch_page_list(&source_manga_id, &source_chapter_id);

        let chapter_data = tokio::select! {
            res = fetch_fut => match res {
                Ok(data) => data,
                Err(e) => {
                    Self::send_event(
                        &self.progress_tx,
                        DownloadProgressEvent::ChapterFailed {
                            chapter_id,
                            chapter_name: name.clone(),
                            error: format!("Failed to fetch pages: {e}"),
                        },
                    );
                    return Err(DownloadError::from_page_list_error(e));
                }
            },
            _ = cancel.cancelled() => {
                Self::send_event(
                    &self.progress_tx,
                    DownloadProgressEvent::ChapterCancelled {
                        chapter_id,
                        chapter_name: name.clone(),
                    },
                );
                return Err(DownloadError::Cancelled);
            }
        };

        let pages = chapter_data.0.pages;
        let base_url = chapter_data.1;

        if pages.is_empty() {
            Self::send_event(
                &self.progress_tx,
                DownloadProgressEvent::ChapterFailed {
                    chapter_id,
                    chapter_name: name.clone(),
                    error: "Source returned no pages for this chapter".to_string(),
                },
            );
            return Err(DownloadError::PageFetch(
                "source returned no pages for this chapter".to_string(),
            ));
        }

        let total_pages = pages.len() as u64;

        let already_staged = Self::collect_staged_indices(&staging_dir).await;
        let pre_completed = already_staged.len() as u64;

        {
            let mut map = self.active.write().await;
            map.insert(
                chapter_id,
                ActiveDownloadState {
                    chapter_id,
                    chapter_name: name.clone(),
                    manga_id,
                    manga_title: manga_title.clone(),
                    total_pages: pages.len(),
                    completed_pages: pre_completed as usize,
                    status: ActiveDownloadStatus::InProgress,
                },
            );
        }

        Self::send_event(
            &self.progress_tx,
            DownloadProgressEvent::ChapterStarted {
                chapter_id,
                chapter_name: name.clone(),
                manga_id,
                manga_title: manga_title.clone(),
                total_pages: pages.len(),
                job_id: job_id.clone(),
            },
        );

        let completed = Arc::new(AtomicU64::new(pre_completed));
        let on_page = Arc::new(on_page);
        let active_ref = self.active.clone();
        let tx = self.progress_tx.clone();
        let client = http_client.unwrap_or_else(|| self.smart_client.clone());

        let mut stream = stream::iter(pages)
            .map(|page| {
                let client = client.clone();
                let base_url = base_url.clone();
                let chapter_name = name.clone();
                let staging_dir = staging_dir.clone();
                let active_ref2 = active_ref.clone();
                let tx2 = tx.clone();
                let completed2 = completed.clone();
                let on_page2 = on_page.clone();
                let already_staged2 = already_staged.clone();

                async move {
                    if let Some(ext) = already_staged2.get(&page.index) {
                        let tmp_path = staging_dir.join(format!("{:04}.{}.tmp", page.index, ext));
                        let filename = format!("{:04}.{}", page.index, ext);
                        let done = completed2.fetch_add(1, Ordering::Relaxed) + 1;
                        on_page2(done, total_pages);
                        return Ok((tmp_path, filename));
                    }

                    let result = Self::download_page_with_retry(
                        &client,
                        &page.url,
                        page.index,
                        &staging_dir,
                        max_attempts,
                        initial_retry_delay_ms,
                        &base_url,
                        page.transform.as_deref(),
                    )
                    .await;

                    if result.is_ok() {
                        Self::update_active(&active_ref2, chapter_id, |s| {
                            s.completed_pages += 1;
                        })
                        .await;
                        Self::send_event(
                            &tx2,
                            DownloadProgressEvent::PageCompleted {
                                chapter_id,
                                chapter_name: chapter_name.clone(),
                                page_index: page.index,
                            },
                        );
                        let done = completed2.fetch_add(1, Ordering::Relaxed) + 1;
                        on_page2(done, total_pages);
                    }

                    result.map_err(|e| match e {
                        error::Error::HttpStatus {
                            status,
                            retry_after_secs,
                            ref context,
                        } => DownloadError::PageHttp {
                            status,
                            retry_after_secs,
                            message: format!("HTTP {status}: {context}"),
                        },
                        other => DownloadError::PageFetch(other.to_string()),
                    })
                }
            })
            .buffer_unordered(concurrent_pages);

        let mut successful: Vec<(PathBuf, String)> = Vec::new();
        let mut page_error: Option<DownloadError> = None;
        let mut is_cancelled = false;

        while let Some(result) = tokio::select! {
            res = stream.next() => res,
            _ = cancel.cancelled() => { is_cancelled = true; None }
        } {
            match result {
                Ok(data) => successful.push(data),
                Err(e) => {
                    page_error = Some(e);
                    break;
                }
            }
        }
        drop(stream);

        if is_cancelled {
            Self::update_active(&active_ref, chapter_id, |s| {
                s.status = ActiveDownloadStatus::Cancelled;
            })
            .await;
            Self::send_event(
                &tx,
                DownloadProgressEvent::ChapterCancelled {
                    chapter_id,
                    chapter_name: name.clone(),
                },
            );
            Self::schedule_active_cleanup(active_ref.clone(), chapter_id);
            return Err(DownloadError::Cancelled);
        }

        if let Some(err) = page_error {
            let err_text = err.to_string();
            Self::update_active(&active_ref, chapter_id, |s| {
                s.status = ActiveDownloadStatus::Failed(err_text.clone());
            })
            .await;
            Self::send_event(
                &tx,
                DownloadProgressEvent::ChapterFailed {
                    chapter_id,
                    chapter_name: name.clone(),
                    error: format!("Page download failed: {err_text}"),
                },
            );
            Self::schedule_active_cleanup(active_ref.clone(), chapter_id);
            return Err(err);
        }

        let successful_len = successful.len();
        match Self::create_cbz(&cbz_path, successful, comic_info).await {
            Ok(()) => {
                Self::update_active(&active_ref, chapter_id, |s| {
                    s.status = ActiveDownloadStatus::Completed;
                    s.completed_pages = successful_len;
                })
                .await;
                Self::schedule_active_cleanup(active_ref.clone(), chapter_id);
                let mut delay = Duration::from_millis(200);
                let mut retries = 0u32;
                loop {
                    match tokio::fs::remove_dir_all(&staging_dir).await {
                        Ok(_) => break,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                        Err(e) => {
                            retries += 1;
                            if retries >= 5 {
                                tracing::warn!(
                                    "Failed to clean up staging dir after {} attempts: {}",
                                    retries,
                                    e
                                );
                                break;
                            }
                            tokio::time::sleep(delay).await;
                            delay *= 2;
                        }
                    }
                }
                Ok(DownloadOutcome {
                    successful_pages: successful_len,
                })
            }
            Err(e) => {
                let err_str = e.to_string();
                Self::update_active(&active_ref, chapter_id, |s| {
                    s.status = ActiveDownloadStatus::Failed(err_str.clone());
                })
                .await;
                Self::send_event(
                    &tx,
                    DownloadProgressEvent::ChapterFailed {
                        chapter_id,
                        chapter_name: name.clone(),
                        error: format!("Failed to create CBZ: {e}"),
                    },
                );
                Self::schedule_active_cleanup(active_ref.clone(), chapter_id);
                Err(DownloadError::Io(std::io::Error::other(err_str)))
            }
        }
    }

    async fn collect_staged_indices(staging_dir: &std::path::Path) -> HashMap<i32, String> {
        let mut map = HashMap::new();
        let Ok(mut dir) = tokio::fs::read_dir(staging_dir).await else {
            return map;
        };
        while let Ok(Some(entry)) = dir.next_entry().await {
            let name = entry.file_name();
            let s = name.to_string_lossy();
            // Leftovers from a transfer that died mid-body. They are never
            // resumable — the next attempt rewrites them from scratch — so
            // sweep them rather than letting them accumulate.
            if s.ends_with(".part") {
                let _ = tokio::fs::remove_file(entry.path()).await;
                continue;
            }
            let Some(rest) = s.strip_suffix(".tmp") else {
                continue;
            };
            let Some((idx_str, ext)) = rest.split_once('.') else {
                continue;
            };
            let Ok(n) = idx_str.parse::<i32>() else {
                continue;
            };
            if !entry.metadata().await.map(|m| m.len() > 0).unwrap_or(false) {
                continue;
            }
            map.insert(n, ext.to_string());
        }
        map
    }

    fn schedule_active_cleanup(
        active: Arc<tokio::sync::RwLock<HashMap<i64, ActiveDownloadState>>>,
        chapter_id: i64,
    ) {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(TERMINAL_STATE_TTL_SECS)).await;
            active.write().await.remove(&chapter_id);
        });
    }
}

pub fn ext_retry_params(kind: kani_shared::extension::ExtensionErrorKind) -> Option<(u32, u64)> {
    use kani_shared::extension::ExtensionErrorKind;
    match kind {
        ExtensionErrorKind::NotFound
        | ExtensionErrorKind::Auth
        | ExtensionErrorKind::InvalidInput
        | ExtensionErrorKind::Parse
        | ExtensionErrorKind::ContentUnavailable => None,
        ExtensionErrorKind::Network | ExtensionErrorKind::Timeout => Some((3, 2_000)),
        ExtensionErrorKind::RateLimited => Some((1, 60_000)),
        ExtensionErrorKind::Internal | ExtensionErrorKind::Unknown => Some((2, 4_000)),
        ExtensionErrorKind::Updating => Some((3, 2_000)),
    }
}

#[cfg(any(test, feature = "test-util"))]
pub struct MockPageListFetcher {
    pub page_count: usize,
    pub server_port: u16,
    pub error_msg: Option<String>,
    pub delay_ms: u64,
}

#[cfg(any(test, feature = "test-util"))]
impl MockPageListFetcher {
    pub fn succeeding(page_count: usize, server_port: u16) -> Arc<Self> {
        Arc::new(Self {
            page_count,
            server_port,
            error_msg: None,
            delay_ms: 0,
        })
    }

    pub fn failing(msg: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            page_count: 0,
            server_port: 0,
            error_msg: Some(msg.into()),
            delay_ms: 0,
        })
    }

    pub fn slow(delay_ms: u64, page_count: usize, server_port: u16) -> Arc<Self> {
        Arc::new(Self {
            page_count,
            server_port,
            error_msg: None,
            delay_ms,
        })
    }
}

#[cfg(any(test, feature = "test-util"))]
#[async_trait::async_trait]
impl PageListFetcher for MockPageListFetcher {
    async fn fetch_page_list(
        &self,
        _manga_id: &str,
        _chapter_id: &str,
    ) -> Result<(crate::wasm::kani::extension::types::Chapter, String)> {
        if self.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        }
        if let Some(ref msg) = self.error_msg {
            return Err(crate::error::Error::Other(msg.clone()));
        }
        use crate::wasm::kani::extension::types::{Chapter, Page};
        let pages = (0..self.page_count)
            .map(|i| Page {
                index: i as i32,
                url: format!("http://127.0.0.1:{}/{}.jpg", self.server_port, i),
                transform: None,
            })
            .collect();
        Ok((
            Chapter { pages },
            format!("http://127.0.0.1:{}", self.server_port),
        ))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    async fn an_empty_page_list_fails_rather_than_sealing_a_zero_page_cbz() {
        let tmp = tempfile::tempdir().unwrap();
        let library_path = tmp.path().to_path_buf();
        let save_path = library_path.join("manga");

        let mgr = DownloaderManager::new(
            SmartClient::new(None)
                .unwrap()
                .with_allow_loopback_egress(true),
            DownloaderConfig::default(),
        )
        .await
        .unwrap();

        let task = DownloadTask {
            chapter_id: 1,
            manga_id: 1,
            manga_title: "M".to_string(),
            source_manager: MockPageListFetcher::succeeding(0, 0),
            source_manga_id: "m".to_string(),
            source_chapter_id: "c".to_string(),
            name: "Chapter 1".to_string(),
            library_path,
            save_path: save_path.clone(),
            comic_info: None,
            http_client: None,
        };

        let result = mgr
            .download_chapter_direct(task, CancellationToken::new(), None, |_, _| {})
            .await;

        match result {
            Err(DownloadError::PageFetch(_)) => {}
            Err(other) => panic!("expected PageFetch error, got {other:?}"),
            Ok(_) => panic!("an empty page list must fail, not seal a CBZ"),
        }
        assert!(
            !save_path.join("Chapter 1.cbz").exists(),
            "a zero-page CBZ must not be written"
        );
    }

    #[test]
    fn downloader_config_default_values() {
        let c = DownloaderConfig::default();
        assert_eq!(c.concurrent_pages, 4);
        assert_eq!(c.max_attempts, 3);
        assert_eq!(c.initial_retry_delay_ms, 1_000);
    }

    #[test]
    fn from_page_list_error_preserves_extension_kind() {
        use kani_shared::extension::{ExtensionError, ExtensionErrorKind};
        let e = crate::error::Error::Extension(ExtensionError {
            kind: ExtensionErrorKind::Updating,
            message: "source updating".to_string(),
            source_url: None,
            retry_after_secs: None,
        });
        match DownloadError::from_page_list_error(e) {
            DownloadError::Extension { kind, message, .. } => {
                assert_eq!(kind, ExtensionErrorKind::Updating);
                assert_eq!(message, "source updating");
            }
            other => panic!("expected Extension, got {other:?}"),
        }
    }

    #[test]
    fn from_page_list_error_defaults_unknown_for_non_extension() {
        let e = crate::error::Error::Other("weird".to_string());
        match DownloadError::from_page_list_error(e) {
            DownloadError::Extension { kind, .. } => {
                assert_eq!(kind, kani_shared::extension::ExtensionErrorKind::Unknown);
            }
            other => panic!("expected Extension, got {other:?}"),
        }
    }

    async fn make_manager() -> DownloaderManager {
        let client = crate::http::SmartClient::new(None).expect("SmartClient");
        DownloaderManager::new(
            client,
            DownloaderConfig {
                concurrent_pages: 4,
                max_attempts: 3,
                initial_retry_delay_ms: 100,
            },
        )
        .await
        .expect("DownloaderManager")
    }

    #[tokio::test]
    async fn snapshot_initially_empty() {
        let mgr = make_manager().await;
        assert!(mgr.snapshot().await.is_empty());
    }

    #[tokio::test]
    async fn subscribe_returns_receiver() {
        let mgr = make_manager().await;
        let mut rx = mgr.subscribe();
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn multiple_subscribers_receive_same_broadcast() {
        let mgr = make_manager().await;
        let mut rx1 = mgr.subscribe();
        let mut rx2 = mgr.subscribe();

        let _ = mgr.progress_tx.send(DownloadProgressEvent::ChapterFailed {
            chapter_id: 1,
            chapter_name: "ch".into(),
            error: "test".into(),
        });

        assert!(rx1.recv().await.is_ok());
        assert!(rx2.recv().await.is_ok());
    }

    fn buffered_resp_with_ct(content_type: &'static str) -> SmartResponse {
        let mut headers = rquest::header::HeaderMap::new();
        headers.insert(
            rquest::header::CONTENT_TYPE,
            rquest::header::HeaderValue::from_static(content_type),
        );
        SmartResponse::Buffered {
            status: rquest::StatusCode::OK,
            url: "https://example.com/img".parse::<rquest::Uri>().unwrap(),
            headers,
            body: bytes::Bytes::new(),
        }
    }

    fn buffered_resp_no_ct() -> SmartResponse {
        SmartResponse::Buffered {
            status: rquest::StatusCode::OK,
            url: "https://example.com/img".parse::<rquest::Uri>().unwrap(),
            headers: rquest::header::HeaderMap::new(),
            body: bytes::Bytes::new(),
        }
    }

    #[test]
    fn image_extension_prefers_content_type_then_url_then_jpg() {
        let by_content_type = [
            ("image/jpeg", "jpg"),
            ("image/png", "png"),
            ("image/webp", "webp"),
            ("image/gif", "gif"),
        ];
        for (content_type, expected) in by_content_type {
            let resp = buffered_resp_with_ct(content_type);
            assert_eq!(
                DownloaderManager::get_image_extension(&resp, "x"),
                expected,
                "{content_type} should map to .{expected}"
            );
        }

        let by_url = [
            ("https://cdn.example.com/page.png?v=1", "png"),
            ("https://cdn.example.com/page", "jpg"),
        ];
        for (url, expected) in by_url {
            let resp = buffered_resp_no_ct();
            assert_eq!(
                DownloaderManager::get_image_extension(&resp, url),
                expected,
                "without a content type, {url} should map to .{expected}"
            );
        }
    }

    #[tokio::test]
    async fn create_cbz_produces_valid_zip_with_pages() {
        let dir = tempfile::TempDir::new().unwrap();
        let page1 = dir.path().join("p1.tmp");
        let page2 = dir.path().join("p2.tmp");
        tokio::fs::write(&page1, b"fake-img-1").await.unwrap();
        tokio::fs::write(&page2, b"fake-img-2").await.unwrap();

        let cbz_path = dir.path().join("out.cbz");
        let staged = vec![
            (page1, "0001.jpg".to_string()),
            (page2, "0002.png".to_string()),
        ];
        DownloaderManager::create_cbz(&cbz_path, staged, None)
            .await
            .unwrap();

        let pages = crate::cbz::list_cbz_pages(&cbz_path).unwrap();
        assert_eq!(pages, vec!["0001.jpg", "0002.png"]);
    }

    #[tokio::test]
    async fn create_cbz_includes_comic_info_xml() {
        let dir = tempfile::TempDir::new().unwrap();
        let page = dir.path().join("p.tmp");
        tokio::fs::write(&page, b"data").await.unwrap();

        let cbz_path = dir.path().join("with_info.cbz");
        let info = crate::comic_info::ComicInfo {
            xmlns_xsi: "http://www.w3.org/2001/XMLSchema-instance",
            series: "Test Series".to_string(),
            title: None,
            number: 1.0,
            volume: None,
            summary: None,
            language_iso: None,
            writer: None,
            penciller: None,
            genre: None,
            web: None,
            pages: None,
        };
        DownloaderManager::create_cbz(&cbz_path, vec![(page, "0001.jpg".to_string())], Some(info))
            .await
            .unwrap();

        let file = std::fs::File::open(&cbz_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let entry_names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(
            entry_names.iter().any(|n| n == "ComicInfo.xml"),
            "ComicInfo.xml not found: {entry_names:?}"
        );
    }

    fn full_info() -> crate::comic_info::ComicInfo {
        crate::comic_info::ComicInfo {
            xmlns_xsi: "http://www.w3.org/2001/XMLSchema-instance",
            series: "Berserk".to_string(),
            title: Some("The Black Swordsman".to_string()),
            number: 12.5,
            volume: Some(3),
            summary: Some("A summary.".to_string()),
            language_iso: Some("en".to_string()),
            writer: Some("Kentaro Miura".to_string()),
            penciller: Some("Kentaro Miura".to_string()),
            genre: Some("Dark Fantasy".to_string()),
            web: Some("https://example.com/berserk".to_string()),
            pages: None,
        }
    }

    /// Build a CBZ through the real writer and return its ComicInfo.xml.
    async fn cbz_comic_info(
        dir: &std::path::Path,
        info: crate::comic_info::ComicInfo,
        page_count: usize,
    ) -> String {
        let mut staged = Vec::new();
        for i in 0..page_count {
            let p = dir.join(format!("p{i}.tmp"));
            tokio::fs::write(&p, b"data").await.unwrap();
            staged.push((p, format!("{:04}.jpg", i + 1)));
        }
        let cbz_path = dir.join("out.cbz");
        DownloaderManager::create_cbz(&cbz_path, staged, Some(info))
            .await
            .unwrap();

        let file = std::fs::File::open(&cbz_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("ComicInfo.xml").unwrap(), &mut xml)
            .unwrap();
        xml
    }

    /// Read `<Tag>` text out of the document, parsing rather than substring
    /// matching — a malformed document fails here, which is the point.
    fn xml_field(xml: &str, tag: &str) -> Option<String> {
        let mut reader = quick_xml::Reader::from_str(xml);
        let mut buf = Vec::new();
        let mut capture = false;
        let mut out = String::new();
        let mut seen = false;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Eof) => break,
                Err(e) => panic!("ComicInfo.xml is not well-formed, no reader could use it: {e}"),
                Ok(quick_xml::events::Event::Start(e)) => {
                    if e.name().as_ref() == tag.as_bytes() {
                        capture = true;
                        seen = true;
                    }
                }
                Ok(quick_xml::events::Event::End(e)) => {
                    if e.name().as_ref() == tag.as_bytes() {
                        capture = false;
                    }
                }
                Ok(quick_xml::events::Event::Text(t)) if capture => {
                    out.push_str(&t.xml_content().unwrap_or_default());
                }
                Ok(quick_xml::events::Event::GeneralRef(r)) if capture => {
                    if let Ok(Some(c)) = r.resolve_char_ref() {
                        out.push(c);
                    } else {
                        out.push_str(match String::from_utf8_lossy(r.as_ref()).as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "quot" => "\"",
                            "apos" => "'",
                            other => panic!("unresolvable entity &{other}; in ComicInfo.xml"),
                        });
                    }
                }
                _ => {}
            }
            buf.clear();
        }
        seen.then_some(out)
    }

    #[tokio::test]
    async fn a_written_cbz_contains_parseable_comicinfo_that_round_trips() {
        let dir = tempfile::TempDir::new().unwrap();
        let xml = cbz_comic_info(dir.path(), full_info(), 2).await;

        assert_eq!(xml_field(&xml, "Series").as_deref(), Some("Berserk"));
        assert_eq!(
            xml_field(&xml, "Title").as_deref(),
            Some("The Black Swordsman")
        );
        assert_eq!(xml_field(&xml, "Number").as_deref(), Some("12.5"));
        assert_eq!(xml_field(&xml, "Volume").as_deref(), Some("3"));
        assert_eq!(xml_field(&xml, "Writer").as_deref(), Some("Kentaro Miura"));
        assert_eq!(xml_field(&xml, "LanguageISO").as_deref(), Some("en"));
        assert_eq!(
            xml_field(&xml, "Web").as_deref(),
            Some("https://example.com/berserk")
        );
    }

    #[tokio::test]
    async fn xml_hostile_metadata_still_parses() {
        const NASTY: &str = r#"Tom & Jerry <vol "1"> 'x' ]]> --"#;
        let dir = tempfile::TempDir::new().unwrap();
        let mut info = full_info();
        info.series = NASTY.to_string();
        info.summary = Some(NASTY.to_string());

        let xml = cbz_comic_info(dir.path(), info, 1).await;
        assert_eq!(
            xml_field(&xml, "Series").as_deref(),
            Some(NASTY),
            "the series title must round-trip through escaping intact"
        );
        assert_eq!(xml_field(&xml, "Summary").as_deref(), Some(NASTY));
    }

    #[tokio::test]
    async fn comicinfo_element_names_match_the_schema() {
        let dir = tempfile::TempDir::new().unwrap();
        let xml = cbz_comic_info(dir.path(), full_info(), 1).await;
        for tag in [
            "Series",
            "Title",
            "Number",
            "Volume",
            "Summary",
            "LanguageISO",
            "Writer",
            "Penciller",
            "Genre",
            "Web",
        ] {
            assert!(
                xml_field(&xml, tag).is_some(),
                "consumers match on <{tag}>; it is missing from the sidecar"
            );
        }
    }

    #[tokio::test]
    async fn absent_optional_metadata_is_omitted_not_emitted_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let info = crate::comic_info::ComicInfo {
            title: None,
            volume: None,
            summary: None,
            writer: None,
            penciller: None,
            genre: None,
            web: None,
            language_iso: None,
            ..full_info()
        };
        let xml = cbz_comic_info(dir.path(), info, 1).await;

        for tag in ["Title", "Volume", "Summary", "Writer", "Penciller", "Web"] {
            assert!(
                xml_field(&xml, tag).is_none(),
                "<{tag}> was absent and must be omitted entirely, not emitted empty"
            );
        }
        assert_eq!(xml_field(&xml, "Series").as_deref(), Some("Berserk"));
    }

    #[tokio::test]
    async fn page_metadata_matches_the_images_actually_in_the_archive() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut staged = Vec::new();
        for i in 0..4 {
            let p = dir.path().join(format!("p{i}.tmp"));
            tokio::fs::write(&p, b"data").await.unwrap();
            staged.push((p, format!("{:04}.jpg", i + 1)));
        }
        let cbz_path = dir.path().join("pages.cbz");
        DownloaderManager::create_cbz(&cbz_path, staged, Some(full_info()))
            .await
            .unwrap();

        let file = std::fs::File::open(&cbz_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let images = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .filter(|n| n != "ComicInfo.xml")
            .count();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("ComicInfo.xml").unwrap(), &mut xml)
            .unwrap();

        let page_elements = xml.matches("<Page ").count();
        assert_eq!(
            page_elements, images,
            "the Pages block describes {page_elements} pages but the archive holds {images}"
        );
    }

    #[tokio::test]
    async fn create_cbz_without_comic_info_has_no_xml() {
        let dir = tempfile::TempDir::new().unwrap();
        let page = dir.path().join("p.tmp");
        tokio::fs::write(&page, b"data").await.unwrap();

        let cbz_path = dir.path().join("no_info.cbz");
        DownloaderManager::create_cbz(&cbz_path, vec![(page, "0001.jpg".to_string())], None)
            .await
            .unwrap();

        let file = std::fs::File::open(&cbz_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let entry_names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(
            !entry_names.iter().any(|n| n == "ComicInfo.xml"),
            "unexpected ComicInfo.xml"
        );
    }

    #[test]
    fn not_found_no_retry() {
        use kani_shared::extension::ExtensionErrorKind;
        assert!(ext_retry_params(ExtensionErrorKind::NotFound).is_none());
        assert!(ext_retry_params(ExtensionErrorKind::Auth).is_none());
        assert!(ext_retry_params(ExtensionErrorKind::InvalidInput).is_none());
        assert!(ext_retry_params(ExtensionErrorKind::Parse).is_none());
        assert!(ext_retry_params(ExtensionErrorKind::ContentUnavailable).is_none());
    }

    #[test]
    fn network_retries_three_times() {
        use kani_shared::extension::ExtensionErrorKind;
        let (max, delay) = ext_retry_params(ExtensionErrorKind::Network).unwrap();
        assert_eq!(max, 3);
        assert!(delay >= 1_000, "delay should be at least 1s, got {delay}ms");
        let (max_t, _) = ext_retry_params(ExtensionErrorKind::Timeout).unwrap();
        assert_eq!(max_t, 3);
    }

    #[test]
    fn rate_limited_escalates_not_short_retry() {
        use kani_shared::extension::ExtensionErrorKind;
        let (max, delay) = ext_retry_params(ExtensionErrorKind::RateLimited).unwrap();
        assert_eq!(max, 1, "RateLimited should only allow 1 attempt (escalate)");
        assert!(
            delay >= 30_000,
            "RateLimited delay should be ≥30s, got {delay}ms"
        );
    }
}
