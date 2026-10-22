use crate::error::CliError;
use std::path::Path;

/// Re-hashes every file `ARCHIVE.json` claims. Exits non-zero on any mismatch,
/// so this doubles as the check that an exported archive is still readable
/// without Kani.
pub fn verify(path: &Path) -> Result<(), CliError> {
    let report = kani_core::archive::verify_archive(path)
        .map_err(|e| CliError::Other(format!("cannot read archive: {e}")))?;

    println!(
        "archive schema {} — {} files checked, {} ok, {} failed",
        report.schema,
        report.checked,
        report.ok,
        report.failures.len()
    );
    for (file, why) in &report.failures {
        println!("  FAIL {file}: {why}");
    }

    if report.is_ok() {
        Ok(())
    } else {
        Err(CliError::Other(format!(
            "{} file(s) failed verification",
            report.failures.len()
        )))
    }
}

/// Prints the manifest computed from a CBZ as it sits on disk.
pub fn manifest(path: &Path) -> Result<(), CliError> {
    let m = kani_core::manifest::manifest_for_cbz(path)
        .map_err(|e| CliError::Other(format!("cannot read {}: {e}", path.display())))?;
    let json = serde_json::to_string_pretty(&m)
        .map_err(|e| CliError::Other(format!("cannot serialise manifest: {e}")))?;
    println!("{json}");
    Ok(())
}
