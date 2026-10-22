use crate::error::Result;
use crate::service::AppService;
use crate::service::encryption::PREFIX;
use kani_shared::types::CredentialEncryptionStatus;
use sqlx::Row;

impl AppService {
    pub fn encryption_enabled(&self) -> bool {
        self.encryption.is_some()
    }

    /// Returns the current encryption state: whether the key is loaded and how many
    /// credential values are still stored in plaintext.
    pub async fn get_encryption_status(&self) -> Result<CredentialEncryptionStatus> {
        let mut plaintext_count: i64 = 0;

        let email_config: String =
            sqlx::query_scalar("SELECT email_provider_config FROM settings WHERE id='singleton'")
                .fetch_one(&self.db_read)
                .await
                .unwrap_or_default();

        if !email_config.is_empty() && !email_config.starts_with(PREFIX) {
            plaintext_count += 1;
        }

        let secret_rows = sqlx::query("SELECT client_secret FROM tracker_app_config")
            .fetch_all(&self.db_read)
            .await
            .unwrap_or_default();

        for row in &secret_rows {
            let secret: Option<String> = row.try_get("client_secret").ok().flatten();
            if let Some(s) = secret
                && !s.is_empty()
                && !s.starts_with(PREFIX)
            {
                plaintext_count += 1;
            }
        }

        let token_rows =
            sqlx::query("SELECT access_token, refresh_token FROM user_tracker_credentials")
                .fetch_all(&self.db_read)
                .await
                .unwrap_or_default();

        for row in &token_rows {
            let at: Option<String> = row.try_get("access_token").ok().flatten();
            let rt: Option<String> = row.try_get("refresh_token").ok().flatten();
            if let Some(t) = at
                && !t.is_empty()
                && !t.starts_with(PREFIX)
            {
                plaintext_count += 1;
            }
            if let Some(t) = rt
                && !t.is_empty()
                && !t.starts_with(PREFIX)
            {
                plaintext_count += 1;
            }
        }

        Ok(CredentialEncryptionStatus {
            encryption_enabled: self.encryption.is_some(),
            plaintext_count,
        })
    }

    /// Encrypt all plaintext credentials in-place. No-op if encryption is disabled.
    pub async fn migrate_credentials_to_encrypted(&self) -> Result<()> {
        let Some(cipher) = self.encryption.as_deref() else {
            return Ok(());
        };

        let email_config: String =
            sqlx::query_scalar("SELECT email_provider_config FROM settings WHERE id='singleton'")
                .fetch_one(&self.db_read)
                .await
                .unwrap_or_default();

        if !email_config.is_empty() && !email_config.starts_with(PREFIX) {
            let encrypted = cipher.encrypt(&email_config);
            sqlx::query("UPDATE settings SET email_provider_config=? WHERE id='singleton'")
                .bind(&encrypted)
                .execute(&self.db)
                .await?;
            // In-memory stays plaintext (the settings struct was already decrypted on startup).
        }

        let secret_rows = sqlx::query("SELECT tracker_id, client_secret FROM tracker_app_config")
            .fetch_all(&self.db_read)
            .await?;

        let mut secrets_migrated = 0u32;
        for row in &secret_rows {
            let tracker_id: i64 = row.try_get("tracker_id")?;
            let secret: Option<String> = row.try_get("client_secret").ok().flatten();
            if let Some(s) = secret
                && !s.is_empty()
                && !s.starts_with(PREFIX)
            {
                let encrypted = cipher.encrypt(&s);
                sqlx::query("UPDATE tracker_app_config SET client_secret=? WHERE tracker_id=?")
                    .bind(&encrypted)
                    .bind(tracker_id)
                    .execute(&self.db)
                    .await?;
                secrets_migrated += 1;
            }
        }

        let token_rows = sqlx::query(
            "SELECT user_id, tracker_id, access_token, refresh_token FROM user_tracker_credentials",
        )
        .fetch_all(&self.db_read)
        .await?;

        for row in &token_rows {
            let user_id: i64 = row.try_get("user_id")?;
            let tracker_id: i64 = row.try_get("tracker_id")?;
            let at: Option<String> = row.try_get("access_token").ok().flatten();
            let rt: Option<String> = row.try_get("refresh_token").ok().flatten();

            let at_needs = at
                .as_deref()
                .map(|t| !t.is_empty() && !t.starts_with(PREFIX))
                .unwrap_or(false);
            let rt_needs = rt
                .as_deref()
                .map(|t| !t.is_empty() && !t.starts_with(PREFIX))
                .unwrap_or(false);

            if at_needs || rt_needs {
                let new_at = at.as_deref().map(|t| {
                    if t.starts_with(PREFIX) {
                        t.to_string()
                    } else {
                        cipher.encrypt(t)
                    }
                });
                let new_rt = rt.as_deref().map(|t| {
                    if t.starts_with(PREFIX) {
                        t.to_string()
                    } else {
                        cipher.encrypt(t)
                    }
                });
                sqlx::query(
                    "UPDATE user_tracker_credentials SET access_token=?, refresh_token=? WHERE user_id=? AND tracker_id=?",
                )
                .bind(&new_at)
                .bind(&new_rt)
                .bind(user_id)
                .bind(tracker_id)
                .execute(&self.db)
                .await?;
            }
        }

        if secrets_migrated > 0 {
            self.reload_tracker_registry().await?;
        }

        Ok(())
    }
}
