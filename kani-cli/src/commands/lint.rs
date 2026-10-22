use crate::error::CliError;
use std::process::Command;

/// Runs the workspace quality checks in sequence with clear per-step output.
/// Convenience wrapper around the individual `cargo` invocations; CI runs each
/// step separately for clearer failure attribution.
///
/// Requires `cargo-machete` and `cargo-deny` to be installed.
pub fn run() -> Result<(), CliError> {
    // clippy mirrors CI: no `--workspace` (extension crates can't compile
    // natively; default-members excludes them).
    run_step(
        "clippy",
        &["clippy", "--locked", "--no-deps", "--", "-D", "warnings"],
    )?;
    run_step("machete", &["machete"])?;
    run_step("deny", &["deny", "check"])?;
    run_step("fmt", &["fmt", "--all", "--check"])?;
    println!("\nAll lint steps passed.");
    Ok(())
}

fn run_step(name: &str, args: &[&str]) -> Result<(), CliError> {
    println!("\n── cargo {} ──", args.join(" "));
    let status = Command::new("cargo")
        .args(args)
        .status()
        .map_err(|e| CliError::Other(format!("could not run `cargo {}`: {e}", args[0])))?;
    if !status.success() {
        return Err(CliError::Other(format!("{name} failed")));
    }
    Ok(())
}
