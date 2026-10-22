//! Ed25519 artifact signing and verification for extension distribution.
//!
//! Shared by the host, which verifies what it installs, and the CLI, which signs
//! what it publishes. One implementation because a verifier that drifts from its
//! signer accepts artifacts the signer never produced, or rejects ones it did.

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
/// Failure to decode, validate, or verify repository signing material.
pub enum SigningError {
    #[error("invalid base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid key length (expected 32 bytes)")]
    InvalidKeyLength,
    #[error("invalid signature length (expected 64 bytes)")]
    InvalidSignatureLength,
    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("signature verification failed")]
    VerificationFailed,
    #[error("sha256 mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },
}

pub(crate) fn sha256_digest(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha256_digest(data))
}

/// Returns the SSH-style fingerprint of a raw 32-byte Ed25519 public key.
/// The format is `SHA256:<base64(sha256(pubkey_bytes))>`, matching `ssh-keygen -l`.
pub fn key_fingerprint(pubkey_bytes: &[u8; 32]) -> String {
    format!("SHA256:{}", B64.encode(sha256_digest(pubkey_bytes)))
}

pub fn pubkey_b64(signing_key: &SigningKey) -> String {
    B64.encode(signing_key.verifying_key().as_bytes())
}

pub fn signature_b64(sig_bytes: &[u8; 64]) -> String {
    B64.encode(sig_bytes)
}

pub fn sign_artifact(artifact: &[u8], signing_key: &SigningKey) -> [u8; 64] {
    use ed25519_dalek::Signer as _;
    signing_key.sign(artifact).to_bytes()
}

/// Verifies that `artifact` bytes were signed by the key identified by `pubkey_b64`
/// (standard base64 of the 32-byte Ed25519 public key). Uses `verify_strict`, which
/// rejects weak and low-order keys. The signature must cover the raw artifact bytes.
pub fn verify_artifact(
    artifact: &[u8],
    pubkey_b64: &str,
    sig_b64: &str,
) -> Result<(), SigningError> {
    let pubkey_bytes = B64.decode(pubkey_b64)?;
    let pubkey_arr: &[u8; 32] = pubkey_bytes
        .as_slice()
        .try_into()
        .map_err(|_| SigningError::InvalidKeyLength)?;
    let verifying_key = VerifyingKey::from_bytes(pubkey_arr)
        .map_err(|e| SigningError::InvalidPublicKey(e.to_string()))?;
    let sig_bytes = B64.decode(sig_b64)?;
    let sig_arr: &[u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| SigningError::InvalidSignatureLength)?;
    let signature = Signature::from_bytes(sig_arr);
    verifying_key
        .verify_strict(artifact, &signature)
        .map_err(|_| SigningError::VerificationFailed)
}

pub fn verify_sha256(artifact: &[u8], expected_hex: &str) -> Result<(), SigningError> {
    let actual = sha256_hex(artifact);
    if actual != expected_hex {
        return Err(SigningError::DigestMismatch {
            expected: expected_hex.to_string(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn gen_key() -> SigningKey {
        let bytes: [u8; 32] = rand::random();
        SigningKey::from_bytes(&bytes)
    }

    #[test]
    fn sign_verify_roundtrip() {
        let sk = gen_key();
        let pk = pubkey_b64(&sk);
        let artifact = b"test artifact bytes for roundtrip";
        let sig = sign_artifact(artifact, &sk);
        verify_artifact(artifact, &pk, &signature_b64(&sig)).unwrap();
    }

    #[test]
    fn tampered_artifact_fails_verification() {
        let sk = gen_key();
        let pk = pubkey_b64(&sk);
        let artifact = b"original artifact content";
        let sig_str = signature_b64(&sign_artifact(artifact, &sk));
        let mut tampered = artifact.to_vec();
        tampered[0] ^= 0x01;
        let result = verify_artifact(&tampered, &pk, &sig_str);
        assert!(matches!(result, Err(SigningError::VerificationFailed)));
    }

    #[test]
    fn wrong_key_fails_verification() {
        let sk1 = gen_key();
        let sk2 = gen_key();
        let artifact = b"some content";
        let sig_str = signature_b64(&sign_artifact(artifact, &sk1));
        let result = verify_artifact(artifact, &pubkey_b64(&sk2), &sig_str);
        assert!(matches!(result, Err(SigningError::VerificationFailed)));
    }

    #[test]
    fn sha256_correct_digest_passes() {
        let artifact = b"file content";
        let hex = sha256_hex(artifact);
        verify_sha256(artifact, &hex).unwrap();
    }

    #[test]
    fn sha256_tampered_file_detected() {
        let artifact = b"file content";
        let hex = sha256_hex(artifact);
        let mut tampered = artifact.to_vec();
        tampered[0] ^= 0x01;
        let result = verify_sha256(&tampered, &hex);
        assert!(matches!(result, Err(SigningError::DigestMismatch { .. })));
    }

    #[test]
    fn key_fingerprint_has_sha256_prefix() {
        let sk = gen_key();
        let fp = key_fingerprint(&sk.verifying_key().to_bytes());
        assert!(
            fp.starts_with("SHA256:"),
            "expected SHA256: prefix, got: {fp}"
        );
        assert!(fp.len() > 8);
    }

    #[test]
    fn invalid_pubkey_length_is_error() {
        let result = verify_artifact(b"data", "aGVsbG8=", "c2ln");
        assert!(matches!(result, Err(SigningError::InvalidKeyLength)));
    }

    #[test]
    fn invalid_signature_length_is_error() {
        let sk = gen_key();
        let pk = pubkey_b64(&sk);
        let result = verify_artifact(b"data", &pk, "dG9vc2hvcnQ=");
        assert!(matches!(result, Err(SigningError::InvalidSignatureLength)));
    }
}
