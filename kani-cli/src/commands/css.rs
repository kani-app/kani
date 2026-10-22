use crate::error::CliError;
use std::path::Path;
use std::process::Command;

pub fn run(watch: bool, prod: bool) -> Result<(), CliError> {
    let binary = if cfg!(windows) {
        "tools/tailwindcss.exe"
    } else {
        "tools/tailwindcss"
    };

    if !Path::new(binary).exists() {
        return Err(CliError::Other(format!(
            "Tailwind CLI not found at {binary} — run `kani-cli setup --tailwind`"
        )));
    }

    let mut cmd = Command::new(binary);
    cmd.args(["-i", "static/css/app.css", "-o", "static/css/main.css"]);

    if watch {
        println!("Watching CSS (dev)...");
        cmd.arg("--watch");
    } else if prod {
        println!("Building CSS (production, minified)...");
        cmd.arg("--minify");
    } else {
        println!("Building CSS (dev)...");
    }

    let status = cmd.status()?;
    if !status.success() {
        return Err(CliError::Other(
            "tailwindcss exited with a non-zero status".into(),
        ));
    }

    if !watch {
        println!("Done: static/css/main.css");
    }

    Ok(())
}
