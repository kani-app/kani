//! Maintainer-key loading for the publishing commands.
//!
//! Signing and verification live in `kani-core::signing`, shared with the host
//! that verifies what this crate publishes.

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use ed25519_dalek::SigningKey;

pub use kani_core::signing::*;

pub fn load_signing_key(path: &std::path::Path) -> Result<SigningKey, crate::error::CliError> {
    let b64 = std::fs::read_to_string(path)?.trim().to_string();
    let seed_bytes = B64.decode(&b64).map_err(|e| {
        crate::error::CliError::Other(format!(
            "Invalid key file '{path}': {e}",
            path = path.display()
        ))
    })?;
    let seed_arr: [u8; 32] = seed_bytes.try_into().map_err(|_| {
        crate::error::CliError::Other(format!(
            "Key file '{}' must contain a 32-byte (44-char base64) seed",
            path.display()
        ))
    })?;
    Ok(SigningKey::from_bytes(&seed_arr))
}

pub fn load_verifying_key(
    path: &std::path::Path,
) -> Result<([u8; 32], String), crate::error::CliError> {
    let b64 = std::fs::read_to_string(path)?.trim().to_string();
    let key_bytes = B64.decode(&b64).map_err(|e| {
        crate::error::CliError::Other(format!("Invalid public key file '{}': {e}", path.display()))
    })?;
    let key_arr: [u8; 32] = key_bytes.try_into().map_err(|_| {
        crate::error::CliError::Other(format!(
            "Public key file '{}' must contain 32 bytes (44-char base64)",
            path.display()
        ))
    })?;
    Ok((key_arr, b64))
}
