use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{error::CliError, signing};

#[derive(Debug, Serialize, Deserialize, Default)]
/// Signed repository index written as `index.json`.
pub struct RepoIndex {
    pub name: String,
    pub maintainer_key: String,
    #[serde(default)]
    pub extensions: Vec<RepoEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// One installable extension artifact and its integrity/signature metadata.
pub struct RepoEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nsfw: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_kani_version: Option<String>,
    pub sha256: String,
    pub signature: String,
    pub author_key: String,
    pub url: String,
}

/// Metadata a `.wasm` artifact cannot supply for itself.
///
/// A YAML extension declares its identity in the file, but reading a component's metadata means
/// instantiating it against async host imports, which this binary has no runtime for. These are
/// therefore operator-supplied and must match what the extension reports, or the index will
/// describe something different from what the server installs.
#[derive(Debug, Default, Clone)]
pub struct WasmMetadata {
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub language: Option<String>,
    pub nsfw: bool,
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

pub fn run(
    file: &Path,
    sign_key_path: &Path,
    repo_dir: &Path,
    repo_sign_key_path: Option<&Path>,
    min_kani_version: Option<&str>,
    wasm_meta: &WasmMetadata,
) -> Result<(), CliError> {
    let format = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if format != "yaml" && format != "wasm" {
        return Err(CliError::Other(
            "extension file must be a .yaml or .wasm file".to_string(),
        ));
    }

    let artifact_bytes = std::fs::read(file)?;

    let (ext_id, ext_name, ext_version, description, language, nsfw) = if format == "yaml" {
        let text = std::str::from_utf8(&artifact_bytes)
            .map_err(|_| CliError::Other("YAML file is not valid UTF-8".to_string()))?;
        let validated = kani_yaml::parse_and_validate(text, file).map_err(|errs| {
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
        let missing: Vec<&str> = [
            ("--ext-id", wasm_meta.id.is_none()),
            ("--ext-name", wasm_meta.name.is_none()),
            ("--ext-version", wasm_meta.version.is_none()),
        ]
        .into_iter()
        .filter_map(|(flag, absent)| absent.then_some(flag))
        .collect();
        if !missing.is_empty() {
            return Err(CliError::Other(format!(
                "publishing a .wasm needs {} — a component's metadata cannot be read without \
                 instantiating it. Use the values the extension itself reports.",
                missing.join(", ")
            )));
        }
        let id = wasm_meta.id.clone().unwrap_or_default();
        let version = wasm_meta.version.clone().unwrap_or_default();
        if semver::Version::parse(version.trim_start_matches('v')).is_err() {
            return Err(CliError::Other(format!(
                "--ext-version '{version}' is not valid semver; update detection compares these"
            )));
        }
        // The metadata string is embedded in the component, so an ID that appears nowhere in the
        // binary is a typo. This cannot prove the value is right, only catch it being wrong.
        if !contains_bytes(&artifact_bytes, id.as_bytes()) {
            return Err(CliError::Other(format!(
                "--ext-id '{id}' does not appear anywhere in {}; it will not match the metadata \
                 the server reads back at install time",
                file.display()
            )));
        }
        (
            id,
            wasm_meta.name.clone().unwrap_or_default(),
            version,
            wasm_meta.description.clone(),
            wasm_meta.language.clone(),
            wasm_meta.nsfw,
        )
    };

    let author_key = signing::load_signing_key(sign_key_path)?;
    let author_pub = signing::pubkey_b64(&author_key);

    let sha256 = signing::sha256_hex(&artifact_bytes);
    let sig_bytes = signing::sign_artifact(&artifact_bytes, &author_key);
    let signature = signing::signature_b64(&sig_bytes);

    let artifact_dir = repo_dir.join("extensions").join(&ext_id).join(&ext_version);
    std::fs::create_dir_all(&artifact_dir)?;

    let artifact_filename = format!("extension.{format}");
    let artifact_path = artifact_dir.join(&artifact_filename);
    let sig_path = artifact_dir.join(format!("{artifact_filename}.sig"));

    std::fs::write(&artifact_path, &artifact_bytes)?;
    std::fs::write(&sig_path, signature.as_bytes())?;

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
        signature,
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
        let sig_b64 = signing::signature_b64(&sig_bytes);
        std::fs::write(repo_dir.join("index.json.sig"), sig_b64.as_bytes())?;
        println!("Signed index.json with maintainer key");
    }

    println!("Published {ext_id} to {}", artifact_path.display());

    Ok(())
}

pub fn load_index(repo_dir: &Path) -> Result<(RepoIndex, Vec<u8>), CliError> {
    let index_path = repo_dir.join("index.json");
    let raw = std::fs::read(&index_path).map_err(|_| {
        CliError::Other(format!("index.json not found in '{}'", repo_dir.display()))
    })?;
    let index: RepoIndex = serde_json::from_slice(&raw)
        .map_err(|e| CliError::Other(format!("Failed to parse index.json: {e}")))?;
    Ok((index, raw))
}
