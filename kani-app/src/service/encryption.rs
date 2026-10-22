use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, aead::Aead};

pub(crate) const PREFIX: &str = "enc:v1:";

pub struct CredentialCipher {
    key: [u8; 32],
}

impl CredentialCipher {
    /// Decode a 64-char lowercase hex string into a 32-byte key.
    pub(crate) fn from_hex(hex: &str) -> Result<Self, String> {
        let bytes = hex::decode(hex.trim()).map_err(|e| format!("Invalid key hex: {e}"))?;
        if bytes.len() != 32 {
            return Err(format!(
                "Key must be 32 bytes (64 hex chars), got {}",
                bytes.len()
            ));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Self { key })
    }

    /// Encrypts `plaintext` → `"enc:v1:<base64url(nonce_12 || ciphertext || tag_16)>"`.
    pub fn encrypt(&self, plaintext: &str) -> String {
        let cipher = ChaCha20Poly1305::new((&self.key).into());
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .expect("ChaCha20Poly1305 encryption is infallible");
        let mut combined = Vec::with_capacity(12 + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(&combined))
    }

    /// Decrypts a value produced by `encrypt`.
    /// Passes through any value that does NOT start with `"enc:v1:"` (backwards compat).
    pub fn decrypt(&self, stored: &str) -> Result<String, String> {
        if !stored.starts_with(PREFIX) {
            return Ok(stored.to_string());
        }
        let encoded = &stored[PREFIX.len()..];
        let combined = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|e| format!("Base64 decode failed: {e}"))?;
        if combined.len() < 12 {
            return Err("Stored value too short to contain nonce".into());
        }
        let (nonce_bytes, ciphertext) = combined.split_at(12);
        let cipher = ChaCha20Poly1305::new((&self.key).into());
        let nonce = Nonce::from_slice(nonce_bytes);
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| "Decryption failed — wrong key or corrupted value".to_string())?;
        String::from_utf8(plaintext).map_err(|e| format!("UTF-8 decode failed: {e}"))
    }
}

/// Encrypt if cipher is Some, else pass through unchanged.
pub(crate) fn maybe_encrypt(cipher: Option<&CredentialCipher>, value: &str) -> String {
    match cipher {
        Some(c) => c.encrypt(value),
        None => value.to_string(),
    }
}

/// Decrypt if cipher is Some, else pass through unchanged.
pub(crate) fn maybe_decrypt(
    cipher: Option<&CredentialCipher>,
    stored: &str,
) -> Result<String, String> {
    match cipher {
        Some(c) => c.decrypt(stored),
        None => Ok(stored.to_string()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn test_cipher() -> CredentialCipher {
        CredentialCipher::from_hex(
            "0000000000000000000000000000000000000000000000000000000000000001",
        )
        .unwrap()
    }

    #[test]
    fn roundtrip() {
        let c = test_cipher();
        let plaintext = "my-smtp-password!@#$%";
        let encrypted = c.encrypt(plaintext);
        assert!(encrypted.starts_with("enc:v1:"));
        let decrypted = c.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn passthrough_plaintext() {
        let c = test_cipher();
        let val = "plaintext-no-prefix";
        assert_eq!(c.decrypt(val).unwrap(), val);
    }

    #[test]
    fn empty_string_roundtrip() {
        let c = test_cipher();
        let encrypted = c.encrypt("");
        let decrypted = c.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn maybe_helpers_without_cipher() {
        let val = "test-value";
        assert_eq!(maybe_encrypt(None, val), val);
        assert_eq!(maybe_decrypt(None, val).unwrap(), val);
    }

    #[test]
    fn wrong_key_fails() {
        let c1 = test_cipher();
        let c2 = CredentialCipher::from_hex(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        )
        .unwrap();
        let encrypted = c1.encrypt("secret");
        assert!(c2.decrypt(&encrypted).is_err());
    }
}
