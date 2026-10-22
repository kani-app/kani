//! TOTP (Time-Based One-Time Password) service.
//!
//! Provides TOTP setup, verification, and backup-code management. TOTP secrets
//! are stored encrypted via `CredentialCipher`. Backup codes are stored as
//! argon2id hashes.

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use secrecy::Secret;
use totp_rs::{Algorithm, TOTP};

use crate::{
    error::{Result, ServiceError},
    ids::UserId,
    service::AppService,
};

/// Characters used when generating backup codes.
const BACKUP_CODE_CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const BACKUP_CODE_LEN: usize = 8;
const BACKUP_CODE_COUNT: usize = 8;

impl AppService {
    /// Begin TOTP setup: generate a new secret, store it unverified, and return
    /// the base32 secret and otpauth URI for QR code display.
    ///
    /// If an unverified row already exists it is replaced. If TOTP is already
    /// fully verified, returns an error — disable first.
    pub async fn begin_totp_setup(
        &self,
        user_id: UserId,
        username: &str,
    ) -> Result<(Secret<String>, String, String)> {
        let verified: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_totp WHERE user_id = ? AND verified_at IS NOT NULL",
        )
        .bind(user_id)
        .fetch_one(&self.db_read)
        .await
        .unwrap_or(0);

        if verified > 0 {
            return Err(ServiceError::Conflict(
                "TOTP is already enabled. Disable it first.".into(),
            ));
        }

        let secret_bytes = totp_rs::Secret::generate_secret();
        let secret_b32 = secret_bytes.to_encoded().to_string();
        let totp = build_totp(&secret_b32, username)?;
        let otpauth_uri = totp.get_url();
        let qr_png_b64 = totp
            .get_qr_base64()
            .map_err(|e| ServiceError::Internal(format!("QR generation failed: {e}")))?;
        let qr_svg = format!("data:image/png;base64,{qr_png_b64}");

        let stored = self.maybe_encrypt(&secret_b32);
        sqlx::query(
            "INSERT INTO user_totp (user_id, secret, verified_at) VALUES (?, ?, NULL) \
             ON CONFLICT(user_id) DO UPDATE SET secret = excluded.secret, verified_at = NULL",
        )
        .bind(user_id)
        .bind(&stored)
        .execute(&self.db)
        .await?;

        Ok((Secret::new(secret_b32), otpauth_uri, qr_svg))
    }

    /// Verify the 6-digit code entered during setup. On success, marks the TOTP
    /// configuration as verified and returns 8 single-use backup codes.
    pub async fn verify_totp_setup(&self, user_id: UserId, code: &str) -> Result<Vec<String>> {
        let secret = self.get_unverified_totp_secret(user_id).await?;
        let totp = build_totp_anonymous(&secret)?;

        if !totp
            .check_current(code)
            .map_err(|_| ServiceError::Validation("TOTP clock error".into()))?
        {
            return Err(ServiceError::Validation(
                "Incorrect verification code".into(),
            ));
        }

        sqlx::query(
            "UPDATE user_totp SET verified_at = unixepoch() WHERE user_id = ? AND verified_at IS NULL",
        )
        .bind(user_id)
        .execute(&self.db)
        .await?;

        let codes = generate_backup_codes();
        self.store_backup_codes(user_id, &codes).await?;

        Ok(codes)
    }

    /// Verify a TOTP code for login step-up.
    pub async fn verify_totp_code(&self, user_id: UserId, code: &str) -> Result<bool> {
        let secret = self.get_verified_totp_secret(user_id).await?;
        let totp = build_totp_anonymous(&secret)?;
        totp.check_current(code)
            .map_err(|_| ServiceError::Internal("TOTP clock error".into()))
    }

    /// Verify a backup code (consuming it) for login step-up.
    pub async fn verify_totp_backup_code(&self, user_id: UserId, code: &str) -> Result<bool> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT id FROM user_backup_codes WHERE user_id = ? AND used_at IS NULL",
        )
        .bind(user_id)
        .fetch_all(&self.db_read)
        .await?;

        // IMPORTANT: we must check all rows to avoid early-exit timing oracles.
        // Only if exactly one matches do we consume it.
        let mut matched_id: Option<String> = None;
        for row_id in &rows {
            let hash: Option<String> =
                sqlx::query_scalar("SELECT code_hash FROM user_backup_codes WHERE id = ?")
                    .bind(row_id)
                    .fetch_optional(&self.db_read)
                    .await
                    .unwrap_or(None);
            if let Some(h) = hash
                && let Ok(parsed) = PasswordHash::new(&h)
                && Argon2::default()
                    .verify_password(code.as_bytes(), &parsed)
                    .is_ok()
                && matched_id.is_none()
            {
                matched_id = Some(row_id.clone());
            }
        }

        if let Some(id) = matched_id {
            sqlx::query("UPDATE user_backup_codes SET used_at = unixepoch() WHERE id = ?")
                .bind(&id)
                .execute(&self.db)
                .await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Disable TOTP after confirming the current TOTP code.
    pub async fn disable_totp(&self, user_id: UserId, totp_code: &str) -> Result<()> {
        if !self.verify_totp_code(user_id, totp_code).await? {
            return Err(ServiceError::Validation("Incorrect TOTP code".into()));
        }
        sqlx::query("DELETE FROM user_totp WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.db)
            .await?;
        sqlx::query("DELETE FROM user_backup_codes WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    /// Regenerate backup codes (invalidates all existing ones). Requires a valid TOTP code.
    pub async fn regenerate_backup_codes(
        &self,
        user_id: UserId,
        totp_code: &str,
    ) -> Result<Vec<String>> {
        if !self.verify_totp_code(user_id, totp_code).await? {
            return Err(ServiceError::Validation("Incorrect TOTP code".into()));
        }
        let codes = generate_backup_codes();
        self.store_backup_codes(user_id, &codes).await?;
        Ok(codes)
    }

    async fn get_unverified_totp_secret(&self, user_id: UserId) -> Result<String> {
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT secret FROM user_totp WHERE user_id = ? AND verified_at IS NULL",
        )
        .bind(user_id)
        .fetch_optional(&self.db_read)
        .await?;
        let stored =
            stored.ok_or_else(|| ServiceError::NotFound("No pending TOTP setup found".into()))?;
        self.maybe_decrypt(&stored)
    }

    async fn get_verified_totp_secret(&self, user_id: UserId) -> Result<String> {
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT secret FROM user_totp WHERE user_id = ? AND verified_at IS NOT NULL",
        )
        .bind(user_id)
        .fetch_optional(&self.db_read)
        .await?;
        let stored = stored
            .ok_or_else(|| ServiceError::NotFound("TOTP is not enabled for this user".into()))?;
        self.maybe_decrypt(&stored)
    }

    async fn store_backup_codes(&self, user_id: UserId, codes: &[String]) -> Result<()> {
        sqlx::query("DELETE FROM user_backup_codes WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.db)
            .await?;

        for code in codes {
            let salt = SaltString::generate(&mut OsRng);
            let hash = Argon2::default()
                .hash_password(code.as_bytes(), &salt)
                .map_err(|e| ServiceError::Internal(format!("backup code hash: {e}")))?
                .to_string();
            sqlx::query("INSERT INTO user_backup_codes (user_id, code_hash) VALUES (?, ?)")
                .bind(user_id)
                .bind(&hash)
                .execute(&self.db)
                .await?;
        }
        Ok(())
    }

    /// Encrypt a plaintext value if a cipher is loaded, otherwise pass through.
    fn maybe_encrypt(&self, plaintext: &str) -> String {
        if let Some(cipher) = &self.encryption {
            cipher.encrypt(plaintext)
        } else {
            plaintext.to_string()
        }
    }

    /// Decrypt a stored value if a cipher is loaded and the value has the encrypted prefix.
    fn maybe_decrypt(&self, stored: &str) -> Result<String> {
        if let Some(cipher) = &self.encryption {
            cipher
                .decrypt(stored)
                .map_err(|e| ServiceError::Internal(format!("TOTP secret decryption failed: {e}")))
        } else {
            Ok(stored.to_string())
        }
    }
}

fn build_totp(secret_b32: &str, username: &str) -> Result<TOTP> {
    let secret = totp_rs::Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|e| ServiceError::Internal(format!("TOTP secret decode: {e}")))?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret,
        Some("Kani".to_string()),
        username.to_string(),
    )
    .map_err(|e| ServiceError::Internal(format!("TOTP build: {e}")))
}

fn build_totp_anonymous(secret_b32: &str) -> Result<TOTP> {
    let secret = totp_rs::Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|e| ServiceError::Internal(format!("TOTP secret decode: {e}")))?;
    TOTP::new(Algorithm::SHA1, 6, 1, 30, secret, None, String::new())
        .map_err(|e| ServiceError::Internal(format!("TOTP build: {e}")))
}

fn generate_backup_codes() -> Vec<String> {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..BACKUP_CODE_COUNT)
        .map(|_| {
            (0..BACKUP_CODE_LEN)
                .map(|_| BACKUP_CODE_CHARS[rng.random_range(0..BACKUP_CODE_CHARS.len())] as char)
                .collect()
        })
        .collect()
}
