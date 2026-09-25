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
