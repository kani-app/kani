//! Axum interface layer for Kani's application service.
//!
//! This crate owns HTTP authentication, authorization, middleware, REST and OPDS routing, proxying,
//! embedded frontend delivery, and transport-specific error responses. Domain mutations remain in
//! `kani-app`.

/// Version reported by web diagnostics and API metadata.
pub const KANI_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod app;
pub mod assets;
pub mod auth;
pub mod cache;
pub mod client_ip;
pub mod csrf;
pub mod error;
pub mod etag;
pub mod i18n;
pub mod idempotency;
pub mod install_gating;
pub mod logging;
pub mod metrics;
pub mod middleware;
pub mod models;
pub mod opds;
pub mod openapi;
pub mod permissions;
pub mod proxy;
pub mod rate_limit;
pub mod rate_limit_key;
pub mod repo_keys;
pub mod rest;
pub mod session_touch;
pub mod state;
pub mod types;
pub mod utils;

pub use kani_core::HTTP_LOGGING_ENABLED;

/// Process-wide emergency gate checked before source installation and update operations.
pub static SOURCE_INSTALL_ALLOWED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
