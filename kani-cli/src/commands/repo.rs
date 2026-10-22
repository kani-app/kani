use std::path::Path;

use crate::{
    commands::publish::{RepoEntry, RepoIndex, load_index},
    error::CliError,
    signing,
};

pub fn run_init(repo_dir: &Path, name: &str, maintainer_key_path: &Path) -> Result<(), CliError> {
    std::fs::create_dir_all(repo_dir)?;
    std::fs::create_dir_all(repo_dir.join("extensions"))?;

    let (key_bytes, key_b64) = signing::load_verifying_key(maintainer_key_path)?;
    let fp = signing::key_fingerprint(&key_bytes);

    let index = crate::commands::publish::RepoIndex {
        name: name.to_string(),
        maintainer_key: key_b64,
        extensions: vec![],
    };

    let index_json = serde_json::to_string_pretty(&index)
        .map_err(|e| CliError::Other(format!("Failed to serialise index.json: {e}")))?;
    std::fs::write(repo_dir.join("index.json"), index_json.as_bytes())?;

    println!("Initialised repository '{name}'");
    println!("  Directory:   {}", repo_dir.display());
    println!("  Fingerprint: {fp}");
    println!();
    println!("Sign index.json by running: kani-cli publish --repo-sign-key <maintainer.key> ...");

    Ok(())
}

pub(crate) fn run_show_fingerprint(key_path: &Path) -> Result<(), CliError> {
    let (key_bytes, _) = signing::load_verifying_key(key_path)?;
    let fp = signing::key_fingerprint(&key_bytes);
    println!("{fp}");
    Ok(())
}

pub fn run_verify(repo_dir: &Path, repo_key_path: Option<&Path>) -> Result<(), CliError> {
    let (index, index_bytes) = load_index(repo_dir)?;

    let maintainer_pub_b64 = if let Some(key_path) = repo_key_path {
        let (_, b64) = signing::load_verifying_key(key_path)?;
        b64
    } else {
        index.maintainer_key.clone()
    };

    let sig_path = repo_dir.join("index.json.sig");
    if !sig_path.exists() {
        return Err(CliError::Other(
            "index.json.sig not found — run publish with --repo-sign-key to sign the index"
                .to_string(),
        ));
    }
    let sig_b64 = std::fs::read_to_string(&sig_path)
        .map_err(|e| CliError::Other(format!("Failed to read index.json.sig: {e}")))?;
    let sig_b64 = sig_b64.trim();

    signing::verify_artifact(&index_bytes, &maintainer_pub_b64, sig_b64)
        .map_err(|e| CliError::Other(format!("index.json signature invalid: {e}")))?;

    println!("index.json: OK (maintainer signature valid)");

    let mut failures = 0usize;

    for entry in &index.extensions {
        let artifact_path = if entry.url.starts_with("http://") || entry.url.starts_with("https://")
        {
            return Err(CliError::Other(format!(
                "Extension '{}' has an absolute URL '{}'; repo verify only works on local repositories",
                entry.id, entry.url
            )));
        } else {
            repo_dir.join(&entry.url)
        };

        match std::fs::read(&artifact_path) {
            Ok(bytes) => {
                let mut ok = true;

                if let Err(e) = signing::verify_sha256(&bytes, &entry.sha256) {
                    eprintln!("FAIL {}@{}: {e}", entry.id, entry.version);
                    ok = false;
                } else if let Err(e) =
                    signing::verify_artifact(&bytes, &entry.author_key, &entry.signature)
                {
                    eprintln!(
                        "FAIL {}@{}: author signature invalid: {e}",
                        entry.id, entry.version
                    );
                    ok = false;
                }

                if ok {
                    println!("{}@{}: OK", entry.id, entry.version);
                } else {
                    failures += 1;
                }
            }
            Err(e) => {
                eprintln!(
                    "FAIL {}@{}: cannot read '{}': {e}",
                    entry.id,
                    entry.version,
                    artifact_path.display()
                );
                failures += 1;
            }
        }
    }

    if failures > 0 {
        return Err(CliError::Other(format!(
            "{failures} extension(s) failed verification"
        )));
    }

    println!(
        "\nAll {} extension(s) verified successfully.",
        index.extensions.len()
    );
    Ok(())
}

pub fn run_list(repo_dir: &Path) -> Result<(), CliError> {
    let (index, _) = load_index(repo_dir)?;
    if index.extensions.is_empty() {
        println!("Repository '{}' has no extensions.", index.name);
        return Ok(());
    }
    println!("Repository: {}", index.name);
    println!("\n{:<32} {:<12} {:<8}", "ID", "VERSION", "FORMAT");
    println!("{}", "-".repeat(54));
    for entry in &index.extensions {
        println!("{:<32} {:<12} {:<8}", entry.id, entry.version, entry.format);
    }
    println!("\n{} extension(s).", index.extensions.len());
    Ok(())
}

pub fn run_add(
    artifact_path: &Path,
    author_key_path: &Path,
    repo_dir: &Path,
    min_kani_version: Option<&str>,
    repo_sign_key_path: Option<&Path>,
) -> Result<(), CliError> {
    let format = artifact_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if format != "yaml" && format != "wasm" {
        return Err(CliError::Other(
            "artifact must be a .yaml or .wasm file".to_string(),
        ));
    }

    let artifact_bytes = std::fs::read(artifact_path)?;
    let sha256 = signing::sha256_hex(&artifact_bytes);

    let sig_path = {
        let fname = artifact_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        artifact_path.with_file_name(format!("{fname}.sig"))
    };
    let sig_b64 = std::fs::read_to_string(&sig_path).map_err(|e| {
        CliError::Other(format!(
            "Failed to read signature file '{}': {e}",
            sig_path.display()
        ))
    })?;
    let sig_b64 = sig_b64.trim().to_string();

    let (_, author_pub) = signing::load_verifying_key(author_key_path)?;
    signing::verify_artifact(&artifact_bytes, &author_pub, &sig_b64)
        .map_err(|e| CliError::Other(format!("Artifact signature invalid: {e}")))?;

    let (ext_id, ext_name, ext_version, description, language, nsfw) = if format == "yaml" {
        let text = std::str::from_utf8(&artifact_bytes)
            .map_err(|_| CliError::Other("YAML file is not valid UTF-8".to_string()))?;
        let validated = kani_yaml::parse_and_validate(text, artifact_path).map_err(|errs| {
            CliError::Other(
                errs.into_iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        (
            validated.id.clone(),
            validated.name.clone(),
            validated.version.clone(),
            validated.metadata.description.clone(),
            if validated.language.is_empty() {
                None
            } else {
                Some(validated.language.clone())
            },
            validated.nsfw,
        )
    } else {
        return Err(CliError::Other(
            "WASM artifact add requires --ext-id/--ext-name/--ext-version; not yet supported"
                .to_string(),
        ));
    };

    let artifact_dir = repo_dir.join("extensions").join(&ext_id).join(&ext_version);
    std::fs::create_dir_all(&artifact_dir)?;

    let artifact_filename = format!("extension.{format}");
    let dest_path = artifact_dir.join(&artifact_filename);
    let dest_sig_path = artifact_dir.join(format!("{artifact_filename}.sig"));

    std::fs::write(&dest_path, &artifact_bytes)?;
    std::fs::write(&dest_sig_path, sig_b64.as_bytes())?;

    let artifact_url = format!("extensions/{ext_id}/{ext_version}/{artifact_filename}");

    let version_override = min_kani_version.map(str::to_string).or_else(|| {
        env!("CARGO_PKG_VERSION")
            .parse::<semver::Version>()
            .ok()
            .map(|v| v.to_string())
    });

    let entry = RepoEntry {
        id: ext_id.clone(),
        name: ext_name,
        version: ext_version,
        format,
        description,
        language,
        nsfw,
        min_kani_version: version_override,
        sha256,
        signature: sig_b64,
        author_key: author_pub,
        url: artifact_url,
    };

    let index_path = repo_dir.join("index.json");
    let mut index: RepoIndex = if index_path.exists() {
        let raw = std::fs::read(&index_path)?;
        serde_json::from_slice(&raw)
            .map_err(|e| CliError::Other(format!("Failed to parse index.json: {e}")))?
    } else {
        RepoIndex::default()
    };

    let pos = index.extensions.iter().position(|e| e.id == ext_id);
    match pos {
        Some(i) => index.extensions[i] = entry,
        None => index.extensions.push(entry),
    }

    let index_json = serde_json::to_string_pretty(&index)
        .map_err(|e| CliError::Other(format!("Failed to serialise index.json: {e}")))?;
    std::fs::write(&index_path, index_json.as_bytes())?;

    if let Some(repo_key_path) = repo_sign_key_path {
        let repo_key = signing::load_signing_key(repo_key_path)?;
        let sig_bytes = signing::sign_artifact(index_json.as_bytes(), &repo_key);
        let index_sig = signing::signature_b64(&sig_bytes);
        std::fs::write(repo_dir.join("index.json.sig"), index_sig.as_bytes())?;
        println!("Signed index.json with maintainer key");
    }

    println!("Added {ext_id} to {}", dest_path.display());
    Ok(())
}
