//! Host-runtime and content-processing primitives for Kani.
//!
//! This crate owns the WASM component host, constrained upstream networking, declarative
//! extraction, chapter downloading, archive and image processing, and script sandboxes. It does
//! not own persistence or HTTP routing; those boundaries live in `kani-app` and `kani-web`.

pub mod archive;
pub mod cache;
pub mod cbz;
pub mod comic_info;
pub mod downloader;
pub mod error;
pub mod evaluator;
pub mod file_storage;
pub mod http;
pub mod install_gating;
pub mod manifest;
pub mod network;
pub mod option_set_fetcher;
pub mod probe;
pub mod quality;
pub mod scripting;
pub mod signing;
pub mod sources;
pub mod transform;
pub mod utilities;
pub mod v8_process;
pub mod wasm;

pub use error::Error;

/// Runtime toggle: when `false`, HTTP request log lines are suppressed.
/// Initialised from settings at startup and updated on settings change.
pub static HTTP_LOGGING_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

pub use wasmtime;

/// Host-side WIT-generated `PreferenceSpec` (with serde derives).
pub use wasm::kani::extension::types::PreferenceSpec;

/// Host-side WIT-generated `FilterList` (with serde derives).
pub use wasm::kani::extension::types::FilterList as WitFilterList;
