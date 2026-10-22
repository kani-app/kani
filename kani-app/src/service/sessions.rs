//! Per-user session inventory and revocation service.
//!
//! Works alongside the `axum_login`/`tower-sessions` session mechanism by maintaining a
//! sidecar `user_sessions` table that mirrors session metadata (creation time, last-seen,
//! user-agent, IP) and supports individual revocation.

use crate::error::Result;
use crate::ids::UserId;
use sqlx::Row;

#[derive(Debug, serde::Serialize)]
pub struct SessionRecord {
    pub id: String,
    pub user_id: i64,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub user_agent: Option<String>,
    pub ip_addr: Option<String>,
    pub revoked_at: Option<i64>,
}

use super::AppService;

impl AppService {
    /// Upsert a session row on each authenticated request.
    ///
    /// On first call for a session, inserts with `created_at = now`.
    /// On subsequent calls, updates `last_seen_at` only.
    pub async fn touch_session(
        &self,
        session_id: &str,
        user_id: UserId,
        user_agent: Option<&str>,
        ip_addr: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_sessions (id, user_id, user_agent, ip_addr) VALUES (?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET last_seen_at = unixepoch()",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(user_agent)
        .bind(ip_addr)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// List all active (non-revoked) sessions for a user, newest-first.
    pub async fn list_sessions(&self, user_id: UserId) -> Result<Vec<SessionRecord>> {
        let rows = sqlx::query(
            "SELECT id, user_id, created_at, last_seen_at, user_agent, ip_addr, revoked_at \
             FROM user_sessions \
             WHERE user_id = ? AND revoked_at IS NULL \
             ORDER BY last_seen_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.db_read)
        .await?;

        let records = rows
            .iter()
            .map(|r| SessionRecord {
                id: r.get("id"),
                user_id: r.get("user_id"),
                created_at: r.get("created_at"),
                last_seen_at: r.get("last_seen_at"),
                user_agent: r.get("user_agent"),
                ip_addr: r.get("ip_addr"),
                revoked_at: r.get("revoked_at"),
            })
            .collect();
        Ok(records)
    }

    /// Revoke a single session. Only permitted if the session belongs to `user_id`.
    pub async fn revoke_session(&self, session_id: &str, user_id: UserId) -> Result<bool> {
        let rows_affected = sqlx::query(
            "UPDATE user_sessions SET revoked_at = unixepoch() \
             WHERE id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(session_id)
        .bind(user_id)
        .execute(&self.db)
        .await?
        .rows_affected();
        Ok(rows_affected > 0)
    }

    /// Revoke all sessions for `user_id` except `current_session_id`.
    pub async fn revoke_other_sessions(
        &self,
        user_id: UserId,
        current_session_id: &str,
    ) -> Result<u64> {
        let rows_affected = sqlx::query(
            "UPDATE user_sessions SET revoked_at = unixepoch() \
             WHERE user_id = ? AND id != ? AND revoked_at IS NULL",
        )
        .bind(user_id)
        .bind(current_session_id)
        .execute(&self.db)
        .await?
        .rows_affected();
        Ok(rows_affected)
    }

    /// Returns `false` when the TOTP table is absent or the user has no verified row.
    pub async fn is_totp_enabled(&self, user_id: UserId) -> Result<bool> {
        let table_exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='user_totp'",
        )
        .fetch_one(&self.db_read)
        .await
        .unwrap_or(0);

        if table_exists == 0 {
            return Ok(false);
        }

        let enabled: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_totp WHERE user_id = ? AND verified_at IS NOT NULL",
        )
        .bind(user_id)
        .fetch_one(&self.db_read)
        .await
        .unwrap_or(0);

        Ok(enabled > 0)
    }

    /// Returns `true` only when this session has an explicit revocation on
    /// record.
    ///
    /// Deliberately *not* "is it valid": a session's row is created by the
    /// first `touch_session`, which runs after this check on the very first
    /// authenticated request. Treating "no row" as invalid would log every new
    /// session straight back out. Absence means "not yet recorded", and only a
    /// non-null `revoked_at` means "revoked".
    pub async fn is_session_revoked(&self, session_id: &str) -> bool {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM user_sessions WHERE id = ? AND revoked_at IS NOT NULL",
        )
        .bind(session_id)
        .fetch_one(&self.db_read)
        .await
        .unwrap_or(0)
            > 0
    }
}
