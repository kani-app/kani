//! Application-layer error type — no Axum dependency.

pub type Result<T, E = ServiceError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Source {0} is disabled")]
    SourceDisabled(i64),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Permission denied: {0}")]
    Forbidden(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Rate limited")]
    RateLimited { retry_after_secs: Option<u64> },

    /// The tracker rejected our credentials (401/403, or a refusal to refresh).
    /// Distinct from `Internal` because the caller must flag the link for
    /// re-authentication rather than retry it.
    #[error("Tracker authentication expired: {0}")]
    TrackerAuthExpired(String),

    #[error("Possible duplicate")]
    PossibleDuplicate(Vec<crate::service::dedup::SimilarMangaHit>),

    #[error(transparent)]
    Core(#[from] kani_core::Error),

    #[error(transparent)]
    Db(#[from] sqlx::Error),

    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    TryFromInt(#[from] std::num::TryFromIntError),

    #[error(transparent)]
    RequestError(#[from] rquest::Error),
}
