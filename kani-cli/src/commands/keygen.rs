use std::path::PathBuf;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use ed25519_dalek::SigningKey;

use crate::{error::CliError, signing::key_fingerprint};

pub fn run(out_dir: &PathBuf, name: &str) -> Result<(), CliError> {
    std::fs::create_dir_all(out_dir)?;

    let seed: [u8; 32] = {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes)
            .map_err(|e| CliError::Other(format!("failed to read random bytes: {e}")))?;
        bytes
    };

    let signing_key = SigningKey::from_bytes(&seed);
    let pub_b64 = crate::signing::pubkey_b64(&signing_key);
    let seed_b64 = B64.encode(seed);

    let pub_file = out_dir.join(format!("{name}.pub"));
    let key_file = out_dir.join(format!("{name}.key"));

    std::fs::write(&pub_file, pub_b64.as_bytes())?;
    std::fs::write(&key_file, seed_b64.as_bytes())?;

    // The private key is plaintext base64, so the file mode is the only thing
    // protecting it. Default umask gives 0644 — readable by every account on
    // the machine.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600))?;
    }

    let fp = key_fingerprint(&signing_key.verifying_key().to_bytes());

    println!("Generated {name} keypair:");
    println!("  Public key:  {}", pub_file.display());
    println!("  Private key: {}", key_file.display());
    println!("  Fingerprint: {fp}");
    println!();
    println!("Share the fingerprint alongside your repository URL so users can verify trust.");
    println!("Keep {} secret and back it up.", key_file.display());

    Ok(())
}
