//! Server-specific error types with Axum `IntoResponse` implementation.

use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

use kani_shared::extension::ExtensionErrorKind;

/// Maps a source failure to a status the caller can act on. Modelled on
/// `classify_download_error`, which already routes the same enum.
fn source_error_status(kind: ExtensionErrorKind) -> StatusCode {
    match kind {
        ExtensionErrorKind::Network | ExtensionErrorKind::Parse => StatusCode::BAD_GATEWAY,
        ExtensionErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
        ExtensionErrorKind::Updating => StatusCode::SERVICE_UNAVAILABLE,
        ExtensionErrorKind::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        ExtensionErrorKind::NotFound | ExtensionErrorKind::ContentUnavailable => {
            StatusCode::NOT_FOUND
        }
        ExtensionErrorKind::Auth => StatusCode::UNAUTHORIZED,
        ExtensionErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
        ExtensionErrorKind::Internal | ExtensionErrorKind::Unknown => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

fn source_error_code(kind: ExtensionErrorKind) -> &'static str {
    match kind {
        ExtensionErrorKind::Network => "source_network",
        ExtensionErrorKind::Timeout => "source_timeout",
        ExtensionErrorKind::Updating => "source_updating",
        ExtensionErrorKind::RateLimited => "source_rate_limited",
        ExtensionErrorKind::NotFound => "source_not_found",
        ExtensionErrorKind::ContentUnavailable => "content_unavailable",
        ExtensionErrorKind::Auth => "source_auth_required",
        ExtensionErrorKind::Parse => "source_parse",
        ExtensionErrorKind::InvalidInput => "invalid_input",
        ExtensionErrorKind::Internal => "source_internal",
        ExtensionErrorKind::Unknown => "source_error",
    }
}

/// Non-actionable kinds are logged, not surfaced: their text is internal
/// detail the caller can do nothing with.
fn source_error_message(e: &kani_shared::extension::ExtensionError) -> String {
    match e.kind {
        ExtensionErrorKind::Internal | ExtensionErrorKind::Unknown => {
            tracing::error!(kind = ?e.kind, "source error: {}", e.message);
            "The source failed unexpectedly".to_string()
        }
        _ => e.message.clone(),
    }
}

fn source_error_hint(kind: ExtensionErrorKind) -> Option<&'static str> {
    match kind {
        ExtensionErrorKind::Auth => {
            Some("This source requires login. Configure credentials in source settings.")
        }
        ExtensionErrorKind::RateLimited => {
            Some("The source is rate limiting Kani. Try again soon.")
        }
        ExtensionErrorKind::Updating => Some("The source is updating. Try again shortly."),
        ExtensionErrorKind::Parse => {
            Some("The source's page layout changed. The extension may need an update.")
        }
        _ => None,
    }
}

pub type Result<T, E = AppError> = std::result::Result<T, E>;

#[derive(Error, Debug)]
/// Web-layer failure classified into an HTTP status and safe client response.
/// Internal variants are logged with detail but expose only a generic message.
pub enum AppError {
    #[error("Internal Server Error: {0}")]
    InternalServerError(String),

    #[error("Not Found: {0}")]
    NotFound(String),

    #[error("Other error: {0}")]
    Other(String),

    #[error("Database error: {0}")]
    SqlxError(#[from] sqlx::Error),

    #[error("Core error: {0}")]
    CoreError(#[from] kani_core::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Migration error: {0}")]
    MigrationError(#[from] sqlx::migrate::MigrateError),

    #[error("Request error: {0}")]
    RequestError(#[from] rquest::Error),

    #[error("Lock poisoned")]
    LockPoisoned,

    #[error("Channel send error: {0}")]
    ChannelSendError(String),

    #[error("TryFromIntError: {0}")]
    TryFromIntError(#[from] std::num::TryFromIntError),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("multipart error: {0}")]
    MultipartError(#[from] axum::extract::multipart::MultipartError),

    #[error("Invalid Header Value: {0}")]
    InvalidHeaderValue(#[from] rquest::header::InvalidHeaderValue),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Password hash error: {0}")]
    HashError(#[from] argon2::password_hash::Error),

    #[error("Password error: {0}")]
    PasswordError(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Permission error: {0}")]
    PermissionParseError(#[from] crate::permissions::PermissionParseError),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("{0}")]
    FlareSolverrRequired(String),

    #[error("{0}")]
    SourceExtension(kani_shared::extension::ExtensionError),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Source requires authentication: {0}")]
    SourceAuthRequired(String),

    #[error("Source {0} is disabled")]
    SourceDisabled(i64),

    #[error("Possible duplicate")]
    PossibleDuplicate(Vec<kani_app::SimilarMangaHit>),

    #[error("Email error: {0}")]
    EmailError(String),
}

impl AppError {
    /// Short machine-readable code included in JSON error bodies.
    pub(crate) fn error_code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Unauthorized(_) | Self::PasswordError(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::ValidationError(_) => "validation_error",
            Self::FlareSolverrRequired(_) => "flaresolverr_required",
            Self::SourceExtension(e) => source_error_code(e.kind),
            Self::RateLimitExceeded => "rate_limited",
            Self::SourceAuthRequired(_) => "source_auth_required",
            Self::SourceDisabled(_) => "source_disabled",
            Self::EmailError(_) => "email_error",
            _ => "internal_error",
        }
    }

    /// Optional actionable guidance shown to the user.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::FlareSolverrRequired(code) => Some(match code.as_str() {
                "solver_unauthorized" => {
                    "The solver rejected Kani's key. Check that KANI_SOLVER_SECRET matches the \
                     solver's API_KEY."
                }
                "solver_incompatible" => {
                    "This solver cannot run capture scripts. Switch it to the \
                     ghcr.io/kani-app/flaresolverr image in Settings > Advanced."
                }
                "solver_unreachable" => {
                    "No solver answered at the configured URL. Check it in Settings > Advanced."
                }
                _ => "This source needs a solver. Set a solver URL in Settings > Advanced.",
            }),
            Self::SourceExtension(e) => source_error_hint(e.kind),
            Self::RateLimitExceeded => Some("Too many requests. Please wait a moment."),
            Self::SourceAuthRequired(_) => {
                Some("This source requires login. Configure credentials in source settings.")
            }
            Self::Forbidden(_) => {
                Some("You don't have permission for this action. Contact an administrator.")
            }
            Self::Unauthorized(_) => Some("Please log in to continue."),
            _ => None,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Self::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            Self::Unauthorized(msg) | Self::PasswordError(msg) => {
                (StatusCode::UNAUTHORIZED, msg.clone())
            }
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            Self::ValidationError(msg) => (StatusCode::BAD_REQUEST, msg.clone()),

            Self::SqlxError(e) => {
                tracing::error!("Database error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::CoreError(e) => {
                tracing::error!("Core error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::IoError(e) => {
                tracing::error!("IO error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::TryFromIntError(e) => {
                tracing::error!("Integer conversion error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::MigrationError(e) => {
                tracing::error!("Migration error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::RequestError(e) => {
                tracing::error!("HTTP request error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::LockPoisoned => {
                tracing::error!("Mutex lock poisoned");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::InternalServerError(msg) => {
                tracing::error!("Internal error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::ChannelSendError(msg) => {
                tracing::error!("Channel send error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::JsonError(e) => {
                tracing::error!("JSON error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::MultipartError(e) => {
                tracing::error!("Multipart error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::InvalidHeaderValue(e) => {
                tracing::error!("Invalid header value: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::HashError(e) => {
                tracing::error!("Password hash error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::Other(msg) => {
                tracing::error!("Unclassified error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::PermissionParseError(e) => {
                tracing::error!("Permission parse error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            Self::FlareSolverrRequired(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
            Self::SourceExtension(e) => (source_error_status(e.kind), source_error_message(e)),
            Self::RateLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            Self::SourceAuthRequired(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            Self::SourceDisabled(id) => {
                tracing::debug!("Request to disabled source {id}");
                let body = Json(json!({
                    "error": format!("Source {id} is disabled"),
                    "code": "source_disabled",
                    "disabled": true,
                    "source_id": id,
                    "hint": null,
                }));
                return (StatusCode::CONFLICT, body).into_response();
            }
            Self::PossibleDuplicate(hits) => {
                let body = Json(json!({
                    "status": "possible_duplicate",
                    "suggestions": hits,
                }));
                return (StatusCode::CONFLICT, body).into_response();
            }
            Self::EmailError(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg.clone()),
        };

        let body = Json(json!({
            "error": message,
            "code": self.error_code(),
            "hint": self.hint(),
        }));

        if let Self::SourceExtension(e) = &self
            && let Some(seconds) = e.retry_after_secs
            && status == StatusCode::TOO_MANY_REQUESTS
        {
            return (status, [(header::RETRY_AFTER, seconds.to_string())], body).into_response();
        }

        (status, body).into_response()
    }
}

impl<T> From<std::sync::PoisonError<T>> for AppError {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        Self::LockPoisoned
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for AppError {
    fn from(error: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Self::ChannelSendError(error.to_string())
    }
}

impl From<kani_app::ServiceError> for AppError {
    fn from(e: kani_app::ServiceError) -> Self {
        match e {
            kani_app::ServiceError::NotFound(s) => Self::NotFound(s),
            kani_app::ServiceError::SourceDisabled(id) => Self::SourceDisabled(id),
            kani_app::ServiceError::Conflict(s) => Self::Conflict(s),
            kani_app::ServiceError::Forbidden(s) => Self::Forbidden(s),
            kani_app::ServiceError::Internal(s) => Self::InternalServerError(s),
            kani_app::ServiceError::Validation(s) => Self::ValidationError(s),
            kani_app::ServiceError::RateLimited { .. } => Self::RateLimitExceeded,
            // Not Unauthorized/Forbidden: the caller's own session is fine, it
            // is the linked tracker account that needs re-authorising.
            kani_app::ServiceError::TrackerAuthExpired(s) => Self::Conflict(s),
            kani_app::ServiceError::Core(kani_core::Error::BrowserCaptureUnavailable {
                code,
                ..
            }) => Self::FlareSolverrRequired(code),
            kani_app::ServiceError::Core(kani_core::Error::Extension(e)) => {
                Self::SourceExtension(e)
            }
            kani_app::ServiceError::Core(e) => Self::CoreError(e),
            kani_app::ServiceError::Db(e) => Self::SqlxError(e),
            kani_app::ServiceError::Migration(e) => Self::MigrationError(e),
            kani_app::ServiceError::Io(e) => Self::IoError(e),
            kani_app::ServiceError::TryFromInt(e) => Self::TryFromIntError(e),
            kani_app::ServiceError::RequestError(e) => Self::RequestError(e),
            kani_app::ServiceError::PossibleDuplicate(hits) => Self::PossibleDuplicate(hits),
        }
    }
}
