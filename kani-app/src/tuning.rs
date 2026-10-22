//! Named tuneable constants for the service layer. Change here to affect all usages.
//!
//! Downloader defaults live in `kani_core::downloader` (the crate that owns
//! `DownloaderConfig`); kani-core cannot depend on this crate.

/// Capacity of the SSE refresh-event broadcast channel in production. Sized so a
/// briefly-stalled client can fall behind a burst of refresh events without the
/// sender lagging; slow receivers are dropped past this bound rather than blocking.
pub(crate) const SSE_BROADCAST_CAPACITY: usize = 256;

/// Delay between cover-retry sweeps for manga whose cover download previously failed.
pub(crate) const COVER_RETRY_INTERVAL_SECS: u64 = 30;

/// Interval between background refreshes of cached source credentials.
pub(crate) const CREDENTIAL_REFRESH_INTERVAL_SECS: u64 = 20 * 60;

/// Period of the wasmtime epoch ticker; bounds how long a runaway extension can
/// run before an epoch-deadline interrupt fires.
pub(crate) const WASM_EPOCH_TICK_MS: u64 = 10;

/// Maximum simultaneous background jobs across all types.
pub(crate) const DEFAULT_MAX_CONCURRENT_JOBS: usize = 10;

/// Maximum simultaneous chapter downloads from a single source.
#[cfg(any(test, feature = "test-util"))]
pub const DEFAULT_PER_SOURCE_DOWNLOAD_CONCURRENCY: usize = 1;

/// Maximum simultaneous source scans (library-wide refresh).
#[cfg(any(test, feature = "test-util"))]
pub const DEFAULT_SCAN_CONCURRENCY: usize = 2;

/// How many completed/failed/cancelled jobs to retain before pruning.
pub(crate) const DEFAULT_JOB_MAX_HISTORY: usize = 1000;

/// Seconds to wait for running jobs to drain on graceful shutdown.
pub(crate) const DEFAULT_JOB_SHUTDOWN_TIMEOUT_SECS: u64 = 30;

/// Upper bound on the `width` an OPDS-PSE page request may transcode to. Requests
/// above this are clamped, bounding decode/resize cost from untrusted query params.
pub(crate) const OPDS_MAX_TRANSCODE_WIDTH: u32 = 2400;

/// Number of chapter page-index lists (CBZ central-directory scans) held in the
/// request cache. PSE clients prefetch aggressively; caching avoids re-scanning.
pub(crate) const OPDS_PAGE_INDEX_CACHE_ENTRIES: u64 = 512;
