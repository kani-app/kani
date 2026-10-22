//! Post-process generated Rust source through `rustfmt` if available.

pub(crate) fn try_rustfmt(source: &str) -> String {
    let result = std::process::Command::new("rustfmt")
        .args(["--edition", "2021", "--emit", "stdout"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.take() {
                let mut stdin = stdin;
                let _ = stdin.write_all(source.as_bytes());
            }
            child.wait_with_output()
        });

    match result {
        Ok(output) if output.status.success() => {
            String::from_utf8(output.stdout).unwrap_or_else(|_| source.to_owned())
        }
        _ => source.to_owned(),
    }
}
