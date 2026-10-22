use crate::error::{Result, ServiceError};
use crate::ids::UserId;
use crate::service::AppService;
use crate::service::email::{generate_token, hash_token};
use crate::service::email_templates;

impl AppService {
    /// Sends a verification email to the user. No-op if email is disabled or already verified.
    pub async fn send_verification_email(&self, user_id: UserId) -> Result<()> {
        let user = sqlx::query!(
            "SELECT username, email, email_verified_at FROM users WHERE id = ?",
            user_id
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::NotFound("User not found".into()))?;

        if user.email_verified_at.is_some() {
            return Ok(());
        }

        let settings = self.settings.read().await.clone();
        if !settings.email_enabled {
            return Ok(());
        }

        sqlx::query!(
            "UPDATE email_verification_tokens SET used_at = CURRENT_TIMESTAMP
             WHERE user_id = ? AND used_at IS NULL",
            user_id
        )
        .execute(&self.db)
        .await?;

        let (raw_token, token_hash) = generate_token();
        sqlx::query!(
            "INSERT INTO email_verification_tokens (user_id, token_hash, expires_at)
             VALUES (?, ?, datetime('now', '+24 hours'))",
            user_id,
            token_hash,
        )
        .execute(&self.db)
        .await?;

        let verify_url = format!("{}/verify-email?token={}", settings.app_url, raw_token);
        let (subject, html) =
            email_templates::email_verification_email(&user.username, &verify_url);
        self.send_email_bg(user.email, subject, html);

        Ok(())
    }

    /// Verifies an email verification token and marks the user's email as verified.
    pub async fn verify_email_token(&self, raw_token: &str) -> Result<()> {
        let hash = hash_token(raw_token);

        let row = sqlx::query!(
            "SELECT user_id FROM email_verification_tokens
             WHERE token_hash = ? AND used_at IS NULL AND expires_at > CURRENT_TIMESTAMP",
            hash
        )
        .fetch_optional(&self.db_read)
        .await?
        .ok_or_else(|| ServiceError::Validation("Token is invalid or has expired.".into()))?;

        sqlx::query!(
            "UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = ?",
            row.user_id
        )
        .execute(&self.db)
        .await?;

        sqlx::query!(
            "UPDATE email_verification_tokens SET used_at = CURRENT_TIMESTAMP
             WHERE token_hash = ?",
            hash
        )
        .execute(&self.db)
        .await?;

        self.audit(Some(UserId(row.user_id)), "auth.email_verified", None, None)
            .await;

        Ok(())
    }

    /// Resends a verification email. Rate-limited to 3 sends per user per hour.
    pub async fn resend_verification_email(&self, user_id: UserId) -> Result<()> {
        let recent: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM email_verification_tokens
             WHERE user_id = ? AND created_at > datetime('now', '-1 hour')",
            user_id
        )
        .fetch_one(&self.db_read)
        .await?;

        if recent >= 3 {
            return Err(ServiceError::Validation(
                "Too many verification emails sent recently. Please wait before requesting another.".into(),
            ));
        }

        self.send_verification_email(user_id).await
    }

    /// Sends a welcome email to a newly registered user (fire-and-forget).
    pub fn send_welcome_email(&self, user_id: UserId) {
        let svc = self.clone();
        tokio::spawn(async move {
            let user = sqlx::query!("SELECT username, email FROM users WHERE id = ?", user_id)
                .fetch_optional(&svc.db_read)
                .await;
            if let Ok(Some(u)) = user {
                let (subject, html) = email_templates::welcome_email(&u.username);
                svc.send_email_bg(u.email, subject, html);
            }
        });
    }
}
