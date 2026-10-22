//! Core error types for kani-core.

use thiserror::Error;

/// Core error type for kani operations.
#[derive(Error, Debug)]
pub enum Error {
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Path traversal blocked: {0}")]
    PathTraversal(String),

    /// A URL whose scheme is not http or https; never retried.
    #[error("{0}")]
    UnsupportedScheme(String),

    /// A page whose declared transform cannot be applied; never retried or saved.
    #[error("{0}")]
    Transform(String),

    /// One operation used up its request, response-byte or time budget.
    #[error("{0}")]
    BudgetExceeded(String),

    #[error("Not found: {0}")]
    NotFound(String),

    /// Browser capture could not run, with the solver state preserved.
    ///
    /// The code is carried rather than formatted into a string so the web layer
    /// can turn each state into its own status and hint.
    #[error("Browser capture unavailable: {message} [{code}]")]
    BrowserCaptureUnavailable { code: String, message: String },

    /// A non-success HTTP status, with the code preserved.
    ///
    /// Carrying the status rather than formatting it into a string is what lets
    /// a caller tell "try again" from "this will never work" without matching
    /// on error text.
    #[error("HTTP {status}: {context}")]
    HttpStatus {
        status: u16,
        /// Seconds from a `Retry-After` header, when the server sent one.
        /// Honouring the server's own number beats guessing with backoff.
        retry_after_secs: Option<u64>,
        context: String,
    },

    #[error("WASM error: {0}")]
    Wasm(#[from] wasmtime::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid WASM file")]
    InvalidWasm,

    #[error("WASM memory access error: {0}")]
    WasmMemoryAccess(String),

    #[error("Request error: {0}")]
    Request(#[from] rquest::Error),

    #[error("Lock poisoned")]
    LockPoisoned,

    #[error("Channel send error: {0}")]
    ChannelSend(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Integer conversion error: {0}")]
    IntConversion(#[from] std::num::TryFromIntError),

    #[error("Extension error: {0}")]
    Extension(kani_shared::extension::ExtensionError),

    #[error("{0}")]
    Other(String),
}

impl<T> From<std::sync::PoisonError<T>> for Error {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        Error::LockPoisoned
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for Error {
    fn from(error: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Error::ChannelSend(error.to_string())
    }
}

/// Convenient Result type alias.
pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for String {
    fn from(value: Error) -> Self {
        value.to_string()
    }
}

impl From<crate::transform::TransformError> for Error {
    fn from(value: crate::transform::TransformError) -> Self {
        Error::Other(value.to_string())
    }
}
