//! Structured job failures and retry-policy classification.

use std::time::Duration;

#[derive(Debug, thiserror::Error, serde::Serialize)]
/// Failure boundary recorded by the job manager and exposed through job history.
pub enum JobError {
    #[error("Download error: {0}")]
    Download(DownloadErrorKind),
    #[error("Database error: {0}")]
    Db(String),
    #[error("Source not found: {0}")]
    SourceNotFound(String),
    #[error("Chapter not found: {0}")]
    ChapterNotFound(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("Job panicked: {0}")]
    Panic(String),
    #[error("Internal: {0}")]
    Internal(String),
}

impl JobError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Download(k) if k.is_retryable())
    }
}

impl From<sqlx::Error> for JobError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e.to_string())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
/// Download failure categories used to decide whether and how a job is retried.
pub enum DownloadErrorKind {
    Network { retryable: bool },
    RateLimited { retry_after_secs: Option<u64> },
    NotFound,
    AuthRequired,
    ParseError { message: String },
    ExtensionError { message: String },
    StorageError { path: String, message: String },
    Cancelled,
    Unknown { message: String },
}

impl std::fmt::Display for DownloadErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network { retryable } => write!(f, "Network error (retryable: {retryable})"),
            Self::RateLimited { .. } => write!(f, "Rate limited"),
            Self::NotFound => write!(f, "Not found"),
            Self::AuthRequired => write!(f, "Authentication required"),
            Self::ParseError { message } => write!(f, "Parse error: {message}"),
            Self::ExtensionError { message } => write!(f, "Extension error: {message}"),
            Self::StorageError { path, message } => write!(f, "Storage error at {path}: {message}"),
            Self::Cancelled => write!(f, "Cancelled"),
            Self::Unknown { message } => write!(f, "Unknown error: {message}"),
        }
    }
}

impl DownloadErrorKind {
    pub fn retry_policy(&self) -> Option<RetryPolicy> {
        match self {
            Self::Network { retryable: true } => Some(RetryPolicy {
                max_attempts: 3,
                base_delay: Duration::from_secs(5),
                backoff_multiplier: 6.0,
                jitter_fraction: 0.2,
            }),
            Self::RateLimited { retry_after_secs } => Some(RetryPolicy {
                max_attempts: 5,
                base_delay: retry_after_secs
                    .map(Duration::from_secs)
                    .unwrap_or(Duration::from_secs(60)),
                backoff_multiplier: 2.0,
                jitter_fraction: 0.1,
            }),
            Self::ParseError { .. } | Self::ExtensionError { .. } | Self::Unknown { .. } => {
                Some(RetryPolicy {
                    max_attempts: 1,
                    base_delay: Duration::from_secs(10),
                    backoff_multiplier: 1.0,
                    jitter_fraction: 0.0,
                })
            }
            _ => None,
        }
    }

    pub fn is_retryable(&self) -> bool {
        self.retry_policy().is_some()
    }
}

/// Exponential retry schedule for a classified job failure.
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub backoff_multiplier: f64,
    pub jitter_fraction: f64,
}

impl RetryPolicy {
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base = self.base_delay.as_secs_f64();
        let scaled = base * self.backoff_multiplier.powi(attempt as i32);
        let jitter = if self.jitter_fraction > 0.0 {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            attempt.hash(&mut hasher);
            let h = hasher.finish();
            let frac = (h as f64) / (u64::MAX as f64);
            let range = scaled * self.jitter_fraction * 2.0;
            range * frac - range / 2.0
        } else {
            0.0
        };
        Duration::from_secs_f64((scaled + jitter).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn network_retryable_has_retry_policy() {
        let kind = DownloadErrorKind::Network { retryable: true };
        assert!(kind.is_retryable());
        let policy = kind.retry_policy().unwrap();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.base_delay, Duration::from_secs(5));
    }

    #[test]
    fn network_not_retryable_has_no_retry() {
        assert!(!DownloadErrorKind::Network { retryable: false }.is_retryable());
    }

    #[test]
    fn not_found_has_no_retry() {
        assert!(!DownloadErrorKind::NotFound.is_retryable());
    }

    #[test]
    fn auth_required_has_no_retry() {
        assert!(!DownloadErrorKind::AuthRequired.is_retryable());
    }

    #[test]
    fn storage_error_has_no_retry() {
        let kind = DownloadErrorKind::StorageError {
            path: "/data".into(),
            message: "No space".into(),
        };
        assert!(!kind.is_retryable());
    }

    #[test]
    fn rate_limited_with_retry_after_uses_hint() {
        let kind = DownloadErrorKind::RateLimited {
            retry_after_secs: Some(120),
        };
        let policy = kind.retry_policy().unwrap();
        assert_eq!(policy.base_delay, Duration::from_secs(120));
        assert_eq!(policy.max_attempts, 5);
    }

    #[test]
    fn parse_error_retries_once() {
        let kind = DownloadErrorKind::ParseError {
            message: "bad html".into(),
        };
        assert!(kind.is_retryable());
        assert_eq!(kind.retry_policy().unwrap().max_attempts, 1);
    }

    #[test]
    fn unknown_retries_once() {
        let kind = DownloadErrorKind::Unknown {
            message: "oops".into(),
        };
        assert_eq!(kind.retry_policy().unwrap().max_attempts, 1);
    }

    #[test]
    fn cancelled_has_no_retry() {
        assert!(!DownloadErrorKind::Cancelled.is_retryable());
    }

    #[test]
    fn job_error_is_retryable_proxies_to_download_kind() {
        let retryable = JobError::Download(DownloadErrorKind::Network { retryable: true });
        assert!(retryable.is_retryable());

        let not = JobError::Download(DownloadErrorKind::NotFound);
        assert!(!not.is_retryable());

        assert!(!JobError::Cancelled.is_retryable());
        assert!(!JobError::Internal("boom".into()).is_retryable());
    }
}
