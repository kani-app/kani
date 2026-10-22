use crate::error::{Result, ServiceError};
use crate::ids::UserId;
use crate::service::AppService;
use crate::service::email::{generate_token, hash_token};
use crate::service::email_templates;

impl AppService {
    /// Requests a password reset for the given email address.
    ///
    /// Always returns `Ok(())` — never reveals whether the email is registered.
    /// Rate-limited to 3 requests per email per hour.
    pub async fn request_password_reset(&self, email: &str) -> Result<()> {
        let user = sqlx::query!(
            "SELECT id, username, email FROM users WHERE email = ? AND is_active = TRUE",
            email
        )
        .fetch_optional(&self.db_read)
        .await?;

        let Some(user) = user else {
            return Ok(());
        };

        let settings = self.settings.read().await.clone();
        if !settings.password_reset_enabled {
            return Ok(());
        }

        let recent_count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM password_reset_tokens
             WHERE user_id = ? AND created_at > datetime('now', '-1 hour') AND used_at IS NULL",
            user.id
        )
        .fetch_one(&self.db_read)
        .await?;

        if recent_count >= 3 {
            return Ok(());
        }

        sqlx::query!(
            "UPDATE password_reset_tokens SET used_at = CURRENT_TIMESTAMP
             WHERE user_id = ? AND used_at IS NULL",
            user.id
        )
        .execute(&self.db)
        .await?;

        let (raw_token, token_hash) = generate_token();

        sqlx::query!(
            "INSERT INTO password_reset_tokens (user_id, token_hash, expires_at)
             VALUES (?, ?, datetime('now', '+1 hour'))",
            user.id,
            token_hash,
        )
        .execute(&self.db)
        .await?;

        let reset_url = format!("{}/reset-password?token={}", settings.app_url, raw_token);
        let (subject, html) = email_templates::password_reset_email(&user.username, &reset_url);
        self.send_email_bg(user.email.clone(), subject, html);

        Ok(())
    }

    /// Validates a reset token and returns the user ID if valid.
    /// Does not consume the token — call `consume_reset_token` separately.
    pub(crate) async fn validate_reset_token(&self, raw_token: &str) -> Result<(i64, String)> {
        let hash = hash_token(raw_token);
        let row = sqlx::query!(
            "SELECT prt.user_id, u.email FROM password_reset_tokens prt
             JOIN users u ON u.id = prt.user_id
             WHERE prt.token_hash = ?
               AND prt.used_at IS NULL
               AND prt.expires_at > CURRENT_TIMESTAMP",
            hash
        )
        .fetch_optional(&self.db_read)
        .await?;

        let row =
            row.ok_or_else(|| ServiceError::Validation("Token is invalid or has expired.".into()))?;

        Ok((row.user_id, row.email))
    }

    /// Atomically validates and marks a reset token as used in one UPDATE … RETURNING statement.
    /// Returns the user_id, or an error if the token is invalid, already used, or expired.
    /// Using RETURNING eliminates the validate-then-consume TOCTOU window.
    pub async fn consume_reset_token(&self, raw_token: &str) -> Result<UserId> {
        let hash = hash_token(raw_token);
        let row = sqlx::query!(
            "UPDATE password_reset_tokens SET used_at = CURRENT_TIMESTAMP
             WHERE token_hash = ? AND used_at IS NULL AND expires_at > CURRENT_TIMESTAMP
             RETURNING user_id",
            hash
        )
        .fetch_optional(&self.db)
        .await?
        .ok_or_else(|| {
            ServiceError::Validation("Token is invalid or has already been used.".into())
        })?;
        Ok(UserId(row.user_id))
    }

    /// Returns an obfuscated hint of the email for the validate endpoint.
    pub async fn reset_token_email_hint(&self, raw_token: &str) -> Result<String> {
        let (_user_id, email) = self.validate_reset_token(raw_token).await?;
        Ok(obfuscate_email(&email))
    }

    /// Admin-triggered reset: generates a token and emails it to the user.
    pub async fn admin_trigger_password_reset(
        &self,
        user_id: UserId,
        admin_id: UserId,
    ) -> Result<()> {
        let user = sqlx::query!(
            "SELECT id, username, email FROM users WHERE id = ? AND is_active = TRUE",
            user_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound("User not found".into()))?;

        sqlx::query!(
            "UPDATE password_reset_tokens SET used_at = CURRENT_TIMESTAMP
             WHERE user_id = ? AND used_at IS NULL",
            user.id
        )
        .execute(&self.db)
        .await?;

        let (raw_token, token_hash) = generate_token();
        sqlx::query!(
            "INSERT INTO password_reset_tokens (user_id, token_hash, expires_at)
             VALUES (?, ?, datetime('now', '+1 hour'))",
            user.id,
            token_hash,
        )
        .execute(&self.db)
        .await?;

        let settings = self.settings.read().await.clone();
        let reset_url = format!("{}/reset-password?token={}", settings.app_url, raw_token);

        let (subject, html) = email_templates::password_reset_email(&user.username, &reset_url);
        self.send_email_bg(user.email.clone(), subject, html);

        let (notif_subject, notif_html) =
            email_templates::admin_password_reset_email(&user.username);
        self.send_email_bg(user.email, notif_subject, notif_html);

        self.audit(
            Some(admin_id),
            "auth.admin_password_reset",
            Some(&user.username),
            None,
        )
        .await;

        Ok(())
    }

    /// Sends a password-changed security notification to the user (fire-and-forget).
    pub fn notify_password_changed(&self, user_id: UserId) {
        let svc = self.clone();
        tokio::spawn(async move {
            let user = sqlx::query!("SELECT username, email FROM users WHERE id = ?", user_id)
                .fetch_optional(&svc.db_read)
                .await;
            if let Ok(Some(u)) = user {
                let (subject, html) = email_templates::password_changed_email(&u.username);
                svc.send_email_bg(u.email, subject, html);
            }
        });
    }
}

fn obfuscate_email(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return "***".to_string();
    };
    let first = local.chars().next().unwrap_or('*');
    format!("{first}***@{domain}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::obfuscate_email;

    #[test]
    fn typical_email_hides_local_part() {
        let result = obfuscate_email("alice@example.com");
        assert_eq!(result, "a***@example.com");
    }

    #[test]
    fn single_char_local_part() {
        let result = obfuscate_email("a@b.com");
        assert_eq!(result, "a***@b.com");
    }

    #[test]
    fn domain_preserved_exactly() {
        let result = obfuscate_email("user@mail.example.org");
        assert_eq!(result, "u***@mail.example.org");
    }

    #[test]
    fn no_at_sign_returns_stars() {
        let result = obfuscate_email("notanemail");
        assert_eq!(result, "***");
    }

    #[test]
    fn empty_string_returns_stars() {
        let result = obfuscate_email("");
        assert_eq!(result, "***");
    }
}
