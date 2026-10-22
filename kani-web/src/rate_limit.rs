//! Per-identity and per-IP rate limiting for auth endpoints.
//!
//! Two-phase design:
//!   1. `check_lockout` — read-only: is this identity/IP already locked?
//!   2. `record_and_check` — write then read: record actual outcome, return new lockout state.
//!
//! The pre-check must remain read-only so successful authentication never contributes
//! to the failure window.
//!
//! Identity is stored as SHA-256 hex — the plaintext username/email is never persisted.

use kani_app::models::Settings;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::sync::Arc;
use time::OffsetDateTime;
use tokio::sync::RwLock;

/// Result of a rate-limit check.
#[derive(Debug, PartialEq)]
pub(crate) enum RateLimitResult {
    Allowed,
    LockedOutByIdentity { retry_after_secs: i64 },
    LockedOutByIp { retry_after_secs: i64 },
}

/// Snapshot of the rate-limit tunables, read live from settings per check.
struct Limits {
    max_attempts_identity: i64,
    max_attempts_ip: i64,
    lockout_seconds: i64,
}

/// Tracks login attempts and enforces per-identity and per-IP lockouts.
///
/// The limits (`max_login_attempts`, `max_ip_attempts`, `login_lockout_seconds`)
/// are read from the live `Settings` snapshot on every check, so changes made via
/// the settings API apply without a restart.
#[derive(Clone)]
pub struct AuthRateLimiter {
    db: SqlitePool,
    settings: Arc<RwLock<Settings>>,
}

impl AuthRateLimiter {
    pub fn new(db: SqlitePool, settings: Arc<RwLock<Settings>>) -> Self {
        Self { db, settings }
    }

    async fn limits(&self) -> Limits {
        let s = self.settings.read().await;
        Limits {
            max_attempts_identity: s.max_login_attempts,
            max_attempts_ip: s.max_ip_attempts,
            lockout_seconds: s.login_lockout_seconds,
        }
    }

    fn hash_identity(identity: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(identity.to_lowercase().as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Read-only lockout check performed before authentication.
    ///
    /// Does NOT write to the database. Returns `Locked*` if the identity or IP
    /// is already within a lockout window from prior failures.
    pub(crate) async fn check_lockout(&self, identity: &str, ip_addr: &str) -> RateLimitResult {
        let identity_hash = Self::hash_identity(identity);
        let limits = self.limits().await;
        let window_start = OffsetDateTime::now_utc().unix_timestamp() - limits.lockout_seconds;
        self.lockout_state(&identity_hash, ip_addr, window_start, &limits)
            .await
    }

    /// Records the authentication outcome and returns the resulting lockout state.
    ///
    /// Call this AFTER authentication completes. On success the row is recorded
    /// with `succeeded = 1` and `Allowed` is returned. On failure the row is
    /// recorded with `succeeded = 0` and the lockout state is rechecked.
    pub(crate) async fn record_and_check(
        &self,
        identity: &str,
        ip_addr: &str,
        succeeded: bool,
    ) -> RateLimitResult {
        let identity_hash = Self::hash_identity(identity);
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let limits = self.limits().await;
        let window_start = now - limits.lockout_seconds;

        let succeeded_int = succeeded as i64;
        let _ = sqlx::query(
            "INSERT INTO login_attempts (identity_hash, ip_addr, succeeded, attempted_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(&identity_hash)
        .bind(ip_addr)
        .bind(succeeded_int)
        .bind(now)
        .execute(&self.db)
        .await;

        if succeeded {
            return RateLimitResult::Allowed;
        }

        self.lockout_state(&identity_hash, ip_addr, window_start, &limits)
            .await
    }

    /// Shared lockout computation: query failure counts and derive retry-after.
    async fn lockout_state(
        &self,
        identity_hash: &str,
        ip_addr: &str,
        window_start: i64,
        limits: &Limits,
    ) -> RateLimitResult {
        let now = OffsetDateTime::now_utc().unix_timestamp();

        // Per-identity lockout.
        let identity_failures: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM login_attempts \
             WHERE identity_hash = ? AND succeeded = 0 AND attempted_at >= ?",
        )
        .bind(identity_hash)
        .bind(window_start)
        .fetch_one(&self.db)
        .await
        .unwrap_or(0);

        if identity_failures >= limits.max_attempts_identity {
            let oldest: i64 = sqlx::query_scalar(
                "SELECT MIN(attempted_at) FROM login_attempts \
                 WHERE identity_hash = ? AND succeeded = 0 AND attempted_at >= ?",
            )
            .bind(identity_hash)
            .bind(window_start)
            .fetch_one(&self.db)
            .await
            .unwrap_or(window_start);

            let retry_after_secs = (oldest + limits.lockout_seconds - now).max(1);
            return RateLimitResult::LockedOutByIdentity { retry_after_secs };
        }

        // Per-IP lockout.
        let ip_failures: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM login_attempts \
             WHERE ip_addr = ? AND succeeded = 0 AND attempted_at >= ?",
        )
        .bind(ip_addr)
        .bind(window_start)
        .fetch_one(&self.db)
        .await
        .unwrap_or(0);

        if ip_failures >= limits.max_attempts_ip {
            let oldest: i64 = sqlx::query_scalar(
                "SELECT MIN(attempted_at) FROM login_attempts \
                 WHERE ip_addr = ? AND succeeded = 0 AND attempted_at >= ?",
            )
            .bind(ip_addr)
            .bind(window_start)
            .fetch_one(&self.db)
            .await
            .unwrap_or(window_start);

            let retry_after_secs = (oldest + limits.lockout_seconds - now).max(1);
            return RateLimitResult::LockedOutByIp { retry_after_secs };
        }

        RateLimitResult::Allowed
    }

    /// Prune attempts older than the lockout window. Called by the daily background job.
    pub(crate) async fn prune_old_attempts(&self) {
        let lockout_seconds = self.settings.read().await.login_lockout_seconds;
        let cutoff = OffsetDateTime::now_utc().unix_timestamp() - lockout_seconds;
        let _ = sqlx::query("DELETE FROM login_attempts WHERE attempted_at < ?")
            .bind(cutoff)
            .execute(&self.db)
            .await;
    }
}
