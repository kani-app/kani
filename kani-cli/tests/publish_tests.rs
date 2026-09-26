#![allow(clippy::unwrap_used)]

use std::path::Path;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use ed25519_dalek::SigningKey;
use kani_cli::{
    commands::publish::{RepoIndex, load_index},
    signing,
};

fn gen_signing_key() -> SigningKey {
    let mut seed = [0u8; 32];
    use std::io::Read as _;
    std::fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut seed)
        .unwrap();
    SigningKey::from_bytes(&seed)
}

fn write_key_file(
    dir: &Path,
    name: &str,
    key: &SigningKey,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let pub_path = dir.join(format!("{name}.pub"));
    let key_path = dir.join(format!("{name}.key"));
    std::fs::write(&pub_path, signing::pubkey_b64(key)).unwrap();
    std::fs::write(&key_path, B64.encode(key.to_bytes())).unwrap();
    (pub_path, key_path)
}

const MINIMAL_YAML: &str = r#"
id: test-source
name: Test Source
base_url: https://example.com
version: "1.0.0"
"#;

#[test]
fn publish_yaml_creates_artifact_and_signature() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let (_, author_key_path) = write_key_file(tmp.path(), "author", &author);

    let yaml_path = tmp.path().join("test-source.yaml");
    std::fs::write(&yaml_path, MINIMAL_YAML).unwrap();

    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();

    kani_cli::commands::publish::run(
        &yaml_path,
        &author_key_path,
        &repo_dir,
        None,
        None,
        &Default::default(),
    )
    .unwrap();

    let artifact_path = repo_dir.join("extensions/test-source/1.0.0/extension.yaml");
    assert!(artifact_path.exists(), "artifact file should exist");

    let sig_path = repo_dir.join("extensions/test-source/1.0.0/extension.yaml.sig");
    assert!(sig_path.exists(), "signature file should exist");

    let artifact_bytes = std::fs::read(&artifact_path).unwrap();
    let sig_b64 = std::fs::read_to_string(&sig_path).unwrap();
    let sig_b64 = sig_b64.trim();
    let author_pub = signing::pubkey_b64(&author);

    signing::verify_artifact(&artifact_bytes, &author_pub, sig_b64).unwrap();
}

#[test]
fn publish_upserts_index_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let (_, author_key_path) = write_key_file(tmp.path(), "author", &author);

    let yaml_path = tmp.path().join("test-source.yaml");
    std::fs::write(&yaml_path, MINIMAL_YAML).unwrap();

    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();

    kani_cli::commands::publish::run(
        &yaml_path,
        &author_key_path,
        &repo_dir,
        None,
        None,
        &Default::default(),
    )
    .unwrap();

    let (index, _) = load_index(&repo_dir).unwrap();
    assert_eq!(index.extensions.len(), 1);
    assert_eq!(index.extensions[0].id, "test-source");
    assert_eq!(index.extensions[0].version, "1.0.0");
    assert_eq!(index.extensions[0].format, "yaml");
}

#[test]
fn publish_with_repo_sign_key_creates_index_sig() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let maintainer = gen_signing_key();
    let (maintainer_pub_path, maintainer_key_path) =
        write_key_file(tmp.path(), "maintainer", &maintainer);
    let (_, author_key_path) = write_key_file(tmp.path(), "author", &author);

    let yaml_path = tmp.path().join("test-source.yaml");
    std::fs::write(&yaml_path, MINIMAL_YAML).unwrap();

    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();

    let index = RepoIndex {
        name: "Test Repo".to_string(),
        maintainer_key: signing::pubkey_b64(&maintainer),
        extensions: vec![],
    };
    std::fs::write(
        repo_dir.join("index.json"),
        serde_json::to_string(&index).unwrap(),
    )
    .unwrap();

    kani_cli::commands::publish::run(
        &yaml_path,
        &author_key_path,
        &repo_dir,
        Some(&maintainer_key_path),
        None,
        &Default::default(),
    )
    .unwrap();

    let sig_path = repo_dir.join("index.json.sig");
    assert!(sig_path.exists(), "index.json.sig should exist");

    let (_, index_bytes) = load_index(&repo_dir).unwrap();
    let sig_b64 = std::fs::read_to_string(&sig_path).unwrap();
    let maintainer_pub = signing::pubkey_b64(&maintainer);
    signing::verify_artifact(&index_bytes, &maintainer_pub, sig_b64.trim()).unwrap();

    kani_cli::commands::repo::run_verify(&repo_dir, Some(&maintainer_pub_path)).unwrap();
}

#[test]
fn repo_verify_detects_tampered_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let maintainer = gen_signing_key();
    let (maintainer_pub_path, maintainer_key_path) =
        write_key_file(tmp.path(), "maintainer", &maintainer);
    let (_, author_key_path) = write_key_file(tmp.path(), "author", &author);

    let yaml_path = tmp.path().join("test-source.yaml");
    std::fs::write(&yaml_path, MINIMAL_YAML).unwrap();

    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();

    let index = RepoIndex {
        name: "Test Repo".to_string(),
        maintainer_key: signing::pubkey_b64(&maintainer),
        extensions: vec![],
    };
    std::fs::write(
        repo_dir.join("index.json"),
        serde_json::to_string(&index).unwrap(),
    )
    .unwrap();

    kani_cli::commands::publish::run(
        &yaml_path,
        &author_key_path,
        &repo_dir,
        Some(&maintainer_key_path),
        None,
        &Default::default(),
    )
    .unwrap();

    let artifact_path = repo_dir.join("extensions/test-source/1.0.0/extension.yaml");
    let mut artifact_bytes = std::fs::read(&artifact_path).unwrap();
    artifact_bytes[0] ^= 0x01;
    std::fs::write(&artifact_path, &artifact_bytes).unwrap();

    let result = kani_cli::commands::repo::run_verify(&repo_dir, Some(&maintainer_pub_path));
    assert!(result.is_err(), "verify should fail on tampered artifact");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("failed verification") || err.contains("mismatch") || err.contains("invalid"),
        "unexpected error: {err}"
    );
}

#[test]
fn keygen_creates_pub_and_key_files() {
    let tmp = tempfile::tempdir().unwrap();
    kani_cli::commands::keygen::run(&tmp.path().to_path_buf(), "author").unwrap();

    let pub_path = tmp.path().join("author.pub");
    let key_path = tmp.path().join("author.key");
    assert!(pub_path.exists());
    assert!(key_path.exists());

    let (_, _) = signing::load_verifying_key(&pub_path).unwrap();
    let _ = signing::load_signing_key(&key_path).unwrap();
}

#[cfg(unix)]
#[test]
fn keygen_writes_the_private_key_unreadable_to_other_accounts() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = tempfile::tempdir().unwrap();
    kani_cli::commands::keygen::run(&tmp.path().to_path_buf(), "author").unwrap();

    let mode = std::fs::metadata(tmp.path().join("author.key"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "private key is readable beyond its owner");
}

#[test]
fn keygen_produces_a_different_key_each_time() {
    let tmp = tempfile::tempdir().unwrap();
    kani_cli::commands::keygen::run(&tmp.path().to_path_buf(), "one").unwrap();
    kani_cli::commands::keygen::run(&tmp.path().to_path_buf(), "two").unwrap();

    let one = signing::load_signing_key(&tmp.path().join("one.key")).unwrap();
    let two = signing::load_signing_key(&tmp.path().join("two.key")).unwrap();
    assert_ne!(one.to_bytes(), two.to_bytes());
}

#[test]
fn keygen_pub_matches_private_key() {
    let tmp = tempfile::tempdir().unwrap();
    kani_cli::commands::keygen::run(&tmp.path().to_path_buf(), "test").unwrap();

    let signing_key = signing::load_signing_key(&tmp.path().join("test.key")).unwrap();
    let (verifying_bytes, verifying_b64) =
        signing::load_verifying_key(&tmp.path().join("test.pub")).unwrap();

    assert_eq!(signing_key.verifying_key().to_bytes(), verifying_bytes);
    assert_eq!(signing::pubkey_b64(&signing_key), verifying_b64);
}

#[test]
fn show_fingerprint_prints_sha256_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    kani_cli::commands::keygen::run(&tmp.path().to_path_buf(), "test").unwrap();

    let (key_bytes, _) = signing::load_verifying_key(&tmp.path().join("test.pub")).unwrap();
    let fp = signing::key_fingerprint(&key_bytes);
    assert!(
        fp.starts_with("SHA256:"),
        "fingerprint should start with SHA256:"
    );
}

#[test]
fn repo_list_shows_extensions() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let (_, author_key_path) = write_key_file(tmp.path(), "author", &author);

    let yaml_path = tmp.path().join("test-source.yaml");
    std::fs::write(&yaml_path, MINIMAL_YAML).unwrap();

    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();

    kani_cli::commands::publish::run(
        &yaml_path,
        &author_key_path,
        &repo_dir,
        None,
        None,
        &Default::default(),
    )
    .unwrap();

    kani_cli::commands::repo::run_list(&repo_dir).unwrap();

    let (index, _) = load_index(&repo_dir).unwrap();
    assert_eq!(index.extensions.len(), 1);
    assert_eq!(index.extensions[0].id, "test-source");
}

#[test]
fn repo_list_empty_repo() {
    let tmp = tempfile::tempdir().unwrap();
    kani_cli::commands::keygen::run(&tmp.path().to_path_buf(), "m").unwrap();

    let repo_dir = tmp.path().join("repo");
    kani_cli::commands::repo::run_init(&repo_dir, "Empty Repo", &tmp.path().join("m.pub")).unwrap();

    kani_cli::commands::repo::run_list(&repo_dir).unwrap();
}

#[test]
fn repo_add_copies_artifact_and_updates_index() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let (author_pub_path, author_key_path) = write_key_file(tmp.path(), "author", &author);

    let yaml_path = tmp.path().join("test-source.yaml");
    std::fs::write(&yaml_path, MINIMAL_YAML).unwrap();

    let staging_dir = tmp.path().join("staging");
    std::fs::create_dir_all(&staging_dir).unwrap();

    kani_cli::commands::publish::run(
        &yaml_path,
        &author_key_path,
        &staging_dir,
        None,
        None,
        &Default::default(),
    )
    .unwrap();

    let staged_artifact = staging_dir.join("extensions/test-source/1.0.0/extension.yaml");

    let repo_dir = tmp.path().join("repo");
    kani_cli::commands::repo::run_init(&repo_dir, "Curated Repo", &author_pub_path).unwrap();

    kani_cli::commands::repo::run_add(&staged_artifact, &author_pub_path, &repo_dir, None, None)
        .unwrap();

    let artifact_dest = repo_dir.join("extensions/test-source/1.0.0/extension.yaml");
    assert!(artifact_dest.exists(), "artifact should be copied to repo");

    let sig_dest = repo_dir.join("extensions/test-source/1.0.0/extension.yaml.sig");
    assert!(sig_dest.exists(), "sig should be copied to repo");

    let (index, _) = load_index(&repo_dir).unwrap();
    assert_eq!(index.extensions.len(), 1);
    assert_eq!(index.extensions[0].id, "test-source");
}

#[test]
fn repo_add_rejects_tampered_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let (author_pub_path, author_key_path) = write_key_file(tmp.path(), "author", &author);

    let yaml_path = tmp.path().join("test-source.yaml");
    std::fs::write(&yaml_path, MINIMAL_YAML).unwrap();

    let staging_dir = tmp.path().join("staging");
    std::fs::create_dir_all(&staging_dir).unwrap();
    kani_cli::commands::publish::run(
        &yaml_path,
        &author_key_path,
        &staging_dir,
        None,
        None,
        &Default::default(),
    )
    .unwrap();

    // Tamper in a way that leaves the YAML valid. Corrupting arbitrary bytes makes
    // parsing fail first, which passes this test even with signature verification
    // removed entirely — the signature has to be the only thing that can object.
    let artifact_path = staging_dir.join("extensions/test-source/1.0.0/extension.yaml");
    let text = std::fs::read_to_string(&artifact_path).unwrap();
    let tampered = text.replace("name: Test Source", "name: Evil Source");
    assert_ne!(
        tampered, text,
        "the tamper must actually change the artifact"
    );
    std::fs::write(&artifact_path, &tampered).unwrap();

    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();

    let result =
        kani_cli::commands::repo::run_add(&artifact_path, &author_pub_path, &repo_dir, None, None);
    let error = result
        .expect_err("add must fail on a tampered artifact")
        .to_string();
    assert!(
        error.contains("signature"),
        "the signature must be what rejects it, got: {error}"
    );
}

#[test]
fn repo_init_creates_index_and_extensions_dir() {
    let tmp = tempfile::tempdir().unwrap();
    kani_cli::commands::keygen::run(&tmp.path().to_path_buf(), "maintainer").unwrap();

    let repo_dir = tmp.path().join("my-repo");
    kani_cli::commands::repo::run_init(
        &repo_dir,
        "My Test Repo",
        &tmp.path().join("maintainer.pub"),
    )
    .unwrap();

    assert!(repo_dir.join("index.json").exists());
    assert!(repo_dir.join("extensions").is_dir());

    let raw = std::fs::read(repo_dir.join("index.json")).unwrap();
    let index: RepoIndex = serde_json::from_slice(&raw).unwrap();
    assert_eq!(index.name, "My Test Repo");
    assert!(index.extensions.is_empty());
}

/// Bytes that look enough like a component for publishing: the metadata string an extension
/// reports is embedded in the binary, which is what the ID sanity check looks for.
fn fake_wasm(id: &str) -> Vec<u8> {
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    bytes.extend_from_slice(format!(r#"{{"id":"{id}","name":"Whatever"}}"#).as_bytes());
    bytes
}

fn wasm_meta(id: &str, version: &str) -> kani_cli::commands::publish::WasmMetadata {
    kani_cli::commands::publish::WasmMetadata {
        id: Some(id.to_string()),
        name: Some("Example Source".to_string()),
        version: Some(version.to_string()),
        ..Default::default()
    }
}

fn wasm_fixture(dir: &std::path::Path, id: &str) -> std::path::PathBuf {
    let path = dir.join(format!("{id}.wasm"));
    std::fs::write(&path, fake_wasm(id)).unwrap();
    path
}

#[test]
fn publishing_a_wasm_with_metadata_flags_writes_a_signed_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let (_, author_key_path) = write_key_file(tmp.path(), "author", &author);
    let wasm_path = wasm_fixture(tmp.path(), "example-source");
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();

    kani_cli::commands::publish::run(
        &wasm_path,
        &author_key_path,
        &repo_dir,
        None,
        None,
        &wasm_meta("example-source", "2.1.0"),
    )
    .unwrap();

    let artifact = repo_dir.join("extensions/example-source/2.1.0/extension.wasm");
    assert!(artifact.exists(), "artifact should exist at {artifact:?}");
    assert!(
        repo_dir
            .join("extensions/example-source/2.1.0/extension.wasm.sig")
            .exists(),
        "signature should exist"
    );

    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo_dir.join("index.json")).unwrap()).unwrap();
    let entry = &index["extensions"][0];
    assert_eq!(entry["id"], "example-source");
    assert_eq!(entry["version"], "2.1.0");
    assert_eq!(entry["format"], "wasm");
}

#[test]
fn publishing_a_wasm_without_metadata_names_the_missing_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let (_, author_key_path) = write_key_file(tmp.path(), "author", &author);
    let wasm_path = wasm_fixture(tmp.path(), "example-source");
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();

    let err = kani_cli::commands::publish::run(
        &wasm_path,
        &author_key_path,
        &repo_dir,
        None,
        None,
        &Default::default(),
    )
    .unwrap_err()
    .to_string();

    for flag in ["--ext-id", "--ext-name", "--ext-version"] {
        assert!(err.contains(flag), "error should name {flag}, got: {err}");
    }
}

#[test]
fn a_wrong_ext_id_is_refused_rather_than_published() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let (_, author_key_path) = write_key_file(tmp.path(), "author", &author);
    let wasm_path = wasm_fixture(tmp.path(), "example-source");
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();

    let err = kani_cli::commands::publish::run(
        &wasm_path,
        &author_key_path,
        &repo_dir,
        None,
        None,
        &wasm_meta("exmaple-surce", "1.0.0"),
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("does not appear"),
        "a typo'd ID must be caught before it reaches the index, got: {err}"
    );
    assert!(
        !repo_dir.join("index.json").exists(),
        "nothing should be written when the ID is rejected"
    );
}

#[test]
fn a_non_semver_ext_version_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let author = gen_signing_key();
    let (_, author_key_path) = write_key_file(tmp.path(), "author", &author);
    let wasm_path = wasm_fixture(tmp.path(), "example-source");
    let repo_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();

    let err = kani_cli::commands::publish::run(
        &wasm_path,
        &author_key_path,
        &repo_dir,
        None,
        None,
        &wasm_meta("example-source", "latest"),
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("semver"),
        "update detection compares versions, so a non-semver value must fail: {err}"
    );
}
