#![allow(clippy::unwrap_used)]

//! A WASM source across the paths that load its artifact: install, reload, and a restart of the
//! service over the same data directory.

use kani_app::service::AppService;
use std::path::Path;

fn fixture_wasm() -> Vec<u8> {
    std::fs::read(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("wasm_sources")
            .join("fixture.wasm"),
    )
    .expect("wasm_sources/fixture.wasm: cargo run -p kani-cli -- build kani-fixture-source")
}

async fn start(data_dir: &Path) -> AppService {
    let svc = AppService::new(data_dir).await.unwrap();
    let wasm = data_dir.join("wasm");
    let library = data_dir.join("library");
    std::fs::create_dir_all(&wasm).unwrap();
    std::fs::create_dir_all(&library).unwrap();
    let (wasm, library) = (wasm.to_string_lossy(), library.to_string_lossy());
    let unchanged = svc.settings.read().await.wasm_storage_path == Path::new(wasm.as_ref());
    sqlx::query("UPDATE settings SET wasm_storage_path = ?, library_path = ?")
        .bind(wasm.as_ref())
        .bind(library.as_ref())
        .execute(&svc.db)
        .await
        .unwrap();
    if unchanged {
        return svc;
    }
    drop(svc);
    AppService::new(data_dir).await.unwrap()
}

async fn load_problems(svc: &AppService) -> Vec<String> {
    svc.get_diagnostics()
        .await
        .unwrap()
        .degradations
        .into_iter()
        .filter(|d| d.title.starts_with("Source '"))
        .map(|d| d.detail)
        .collect()
}

#[tokio::test]
async fn an_installed_wasm_source_survives_reload_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let svc = start(dir.path()).await;

    let id = svc.install_wasm_source(&fixture_wasm()).await.unwrap();
    svc.reload_source(id).await.unwrap();
    drop(svc);

    let svc = start(dir.path()).await;
    assert_eq!(load_problems(&svc).await, Vec::<String>::new());
    assert!(
        svc.sources.contains_key(id),
        "the restarted service loaded the installed source"
    );
}

async fn leave_row_stale(svc: &AppService, id: i64) {
    sqlx::query(
        "UPDATE sources SET version = '0.0.1', base_url = 'https://stale.example', \
         unrestricted_http = NOT unrestricted_http WHERE id = ?",
    )
    .bind(id)
    .execute(&svc.db)
    .await
    .unwrap();
}

async fn cache_something(svc: &AppService, extension_id: &str) {
    svc.ext_cache
        .put(
            &format!("{extension_id}:auth"),
            "k",
            b"v".to_vec(),
            std::time::Duration::from_secs(600),
        )
        .await;
}

#[tokio::test]
async fn a_row_left_stale_by_an_interrupted_install_is_reconciled_at_startup() {
    let dir = tempfile::tempdir().unwrap();
    let svc = start(dir.path()).await;
    let id = svc.install_wasm_source(&fixture_wasm()).await.unwrap();
    let installed = svc.get_source(id).await.unwrap();
    leave_row_stale(&svc, id).await;
    cache_something(&svc, &installed.name).await;
    drop(svc);

    let svc = start(dir.path()).await;

    let row = svc.get_source(id).await.unwrap();
    assert_eq!(
        (row.version, row.base_url, row.unrestricted_http),
        (
            installed.version,
            installed.base_url,
            installed.unrestricted_http
        ),
        "the artifact on disk is authoritative over the row"
    );
    assert!(
        svc.ext_cache
            .get(&format!("{}:auth", installed.name), "k")
            .await
            .is_none(),
        "a version reconciled at startup clears what the old version cached"
    );
    assert!(svc.sources.contains_key(id));
}

#[tokio::test]
async fn a_reload_reconciles_a_stale_row_and_clears_its_cache() {
    let svc_dir = tempfile::tempdir().unwrap();
    let svc = start(svc_dir.path()).await;
    let id = svc.install_wasm_source(&fixture_wasm()).await.unwrap();
    let installed = svc.get_source(id).await.unwrap();
    leave_row_stale(&svc, id).await;
    cache_something(&svc, &installed.name).await;

    svc.reload_source(id).await.unwrap();

    let row = svc.get_source(id).await.unwrap();
    assert_eq!(
        (row.version, row.base_url, row.unrestricted_http),
        (
            installed.version,
            installed.base_url,
            installed.unrestricted_http
        )
    );
    assert!(
        svc.ext_cache
            .get(&format!("{}:auth", installed.name), "k")
            .await
            .is_none(),
        "a version change found on reload clears the cache"
    );
}

#[tokio::test]
async fn a_file_declaring_another_sources_id_is_not_loaded_under_its_row() {
    let dir = tempfile::tempdir().unwrap();
    let svc = start(dir.path()).await;
    let id = svc.install_wasm_source(&fixture_wasm()).await.unwrap();
    let installed = svc.get_source(id).await.unwrap();
    let wasm = dir.path().join("wasm");
    std::fs::copy(
        wasm.join(format!("{}.wasm", installed.name)),
        wasm.join("someone-else.wasm"),
    )
    .unwrap();
    let impostor: i64 = sqlx::query_scalar(
        "INSERT INTO sources (name, version, enabled) VALUES ('someone-else', '1.0.0', 1) \
         RETURNING id",
    )
    .fetch_one(&svc.db)
    .await
    .unwrap();
    drop(svc);

    let svc = start(dir.path()).await;

    let problems = load_problems(&svc).await;
    assert!(
        problems.iter().any(|p| p.contains(&format!(
            "declares id '{}' but is stored as source 'someone-else'",
            installed.name
        ))),
        "the mismatch is reported, got {problems:?}"
    );
    assert!(
        !svc.sources.contains_key(impostor),
        "the file must not load under the row it is named after"
    );
    assert!(
        svc.sources.contains_key(id),
        "the source it claims to be still loads"
    );
}

#[derive(Clone, Copy, Debug)]
enum Crash {
    BeforeJournal,
    AfterJournal,
    AfterRename,
    AfterOtherDeleted,
    AfterRowUpdated,
    CorruptStaged,
}

fn fixture_yaml(version: &str) -> String {
    format!(
        "id: fixture\nname: Fixture\nversion: \"{version}\"\nbase_url: \"https://fixture.example\"\n"
    )
}

async fn crash_during_install(
    svc: &AppService,
    storage: &Path,
    crash: Crash,
    new: (kani_core::file_storage::ArtifactFormat, &str, &[u8]),
) {
    use kani_core::file_storage::{ArtifactFormat, stage_install};
    let (format, version, bytes) = new;
    let ext = if format == ArtifactFormat::Yaml {
        "yaml"
    } else {
        "wasm"
    };
    let dir = storage.to_string_lossy().to_string();
    if matches!(crash, Crash::BeforeJournal) {
        std::fs::write(storage.join(format!("fixture.{ext}.staged")), bytes).unwrap();
        return;
    }
    let staged = stage_install(&dir, "fixture", format, version, bytes)
        .await
        .unwrap();
    match crash {
        Crash::AfterJournal => {}
        Crash::CorruptStaged => {
            std::fs::write(storage.join(format!("fixture.{ext}.staged")), b"torn write").unwrap();
        }
        Crash::AfterRename => std::fs::rename(
            storage.join(format!("fixture.{ext}.staged")),
            storage.join(format!("fixture.{ext}")),
        )
        .unwrap(),
        Crash::AfterOtherDeleted => staged.commit_files().await.unwrap(),
        Crash::AfterRowUpdated => {
            staged.commit_files().await.unwrap();
            sqlx::query("UPDATE sources SET version = ? WHERE name = 'fixture'")
                .bind(version)
                .execute(&svc.db)
                .await
                .unwrap();
        }
        Crash::BeforeJournal => unreachable!(),
    }
}

async fn interrupted_install(yaml_to_wasm: bool, crash: Crash) {
    use kani_core::file_storage::ArtifactFormat;
    let wasm = fixture_wasm();
    let yaml_v1 = fixture_yaml("0.0.1");
    let yaml_v2 = fixture_yaml("0.2.0");
    let (new, old_version): ((ArtifactFormat, &str, &[u8]), &str) = if yaml_to_wasm {
        ((ArtifactFormat::Wasm, "0.1.0", &wasm), "0.0.1")
    } else {
        ((ArtifactFormat::Yaml, "0.2.0", yaml_v2.as_bytes()), "0.1.0")
    };

    let dir = tempfile::tempdir().unwrap();
    let svc = start(dir.path()).await;
    if yaml_to_wasm {
        svc.install_yaml_source(yaml_v1.as_bytes()).await.unwrap();
    } else {
        svc.install_wasm_source(&wasm).await.unwrap();
    }
    let storage = dir.path().join("wasm");
    crash_during_install(&svc, &storage, crash, new).await;
    drop(svc);

    let svc = start(dir.path()).await;
    let (version,): (String,) =
        sqlx::query_as("SELECT version FROM sources WHERE name = 'fixture'")
            .fetch_one(&svc.db)
            .await
            .unwrap();
    let rolled_back = matches!(crash, Crash::BeforeJournal | Crash::CorruptStaged);
    let (expected_version, expected_yaml) = if rolled_back {
        (old_version, yaml_to_wasm)
    } else {
        (new.1, !yaml_to_wasm)
    };
    let case = format!("yaml_to_wasm={yaml_to_wasm}, crash {crash:?}");
    assert_eq!(version, expected_version, "{case}");
    assert_eq!(
        storage.join("fixture.yaml").exists(),
        expected_yaml,
        "{case}"
    );
    assert_eq!(
        storage.join("fixture.wasm").exists(),
        !expected_yaml,
        "{case}"
    );
    let leftovers: Vec<String> = std::fs::read_dir(&storage)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".staged") || n.contains(".install"))
        .collect();
    assert!(leftovers.is_empty(), "{case}: {leftovers:?}");
}

#[tokio::test]
async fn yaml_to_wasm_interrupted_before_the_journal() {
    interrupted_install(true, Crash::BeforeJournal).await;
}

#[tokio::test]
async fn yaml_to_wasm_interrupted_after_the_journal() {
    interrupted_install(true, Crash::AfterJournal).await;
}

#[tokio::test]
async fn yaml_to_wasm_interrupted_after_the_rename() {
    interrupted_install(true, Crash::AfterRename).await;
}

#[tokio::test]
async fn yaml_to_wasm_interrupted_after_the_other_format_is_deleted() {
    interrupted_install(true, Crash::AfterOtherDeleted).await;
}

#[tokio::test]
async fn yaml_to_wasm_interrupted_after_the_row_is_updated() {
    interrupted_install(true, Crash::AfterRowUpdated).await;
}

#[tokio::test]
async fn yaml_to_wasm_interrupted_with_a_torn_staged_file() {
    interrupted_install(true, Crash::CorruptStaged).await;
}

#[tokio::test]
async fn wasm_to_yaml_interrupted_before_the_journal() {
    interrupted_install(false, Crash::BeforeJournal).await;
}

#[tokio::test]
async fn wasm_to_yaml_interrupted_after_the_journal() {
    interrupted_install(false, Crash::AfterJournal).await;
}

#[tokio::test]
async fn wasm_to_yaml_interrupted_after_the_rename() {
    interrupted_install(false, Crash::AfterRename).await;
}

#[tokio::test]
async fn wasm_to_yaml_interrupted_after_the_other_format_is_deleted() {
    interrupted_install(false, Crash::AfterOtherDeleted).await;
}

#[tokio::test]
async fn wasm_to_yaml_interrupted_after_the_row_is_updated() {
    interrupted_install(false, Crash::AfterRowUpdated).await;
}

#[tokio::test]
async fn wasm_to_yaml_interrupted_with_a_torn_staged_file() {
    interrupted_install(false, Crash::CorruptStaged).await;
}
