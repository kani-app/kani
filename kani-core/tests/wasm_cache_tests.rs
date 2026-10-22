#![allow(clippy::unwrap_used)]

use kani_core::wasm::WasmRuntime;
use kani_core::wasm::cache::WasmModuleCache;
use std::collections::HashSet;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn load_wasm(name: &str) -> Option<Vec<u8>> {
    let path = workspace_root()
        .join("wasm_sources")
        .join(format!("{name}.wasm"));
    std::fs::read(&path).ok()
}

#[test]
fn corrupt_cwasm_falls_back_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let rt = WasmRuntime::new_on_demand().unwrap();

    let fake_hash = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";
    let cwasm_path = dir.path().join(format!("{fake_hash}.cwasm"));
    std::fs::write(&cwasm_path, b"this is not a valid cwasm file").unwrap();

    let mut cache = WasmModuleCache::new(dir.path().to_path_buf()).unwrap();
    let result = cache.try_get(rt.engine(), fake_hash).unwrap();

    assert!(
        result.is_none(),
        "corrupt cwasm must not return a component"
    );
    assert!(
        !cwasm_path.exists(),
        "corrupt cwasm file must be removed after failed deserialization"
    );
}

#[test]
fn prune_removes_unlisted_cwasm_files() {
    let dir = tempfile::tempdir().unwrap();

    let keep = "aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000";
    let remove = "bbbb1111bbbb1111bbbb1111bbbb1111bbbb1111bbbb1111bbbb1111bbbb1111";
    let other_ext = "cccc2222cccc2222cccc2222cccc2222cccc2222cccc2222cccc2222cccc2222";

    std::fs::write(dir.path().join(format!("{keep}.cwasm")), b"fake").unwrap();
    std::fs::write(dir.path().join(format!("{remove}.cwasm")), b"fake").unwrap();
    std::fs::write(dir.path().join(format!("{other_ext}.wasm")), b"fake").unwrap();

    let cache = WasmModuleCache::new(dir.path().to_path_buf()).unwrap();
    let live: HashSet<String> = std::iter::once(keep.to_string()).collect();
    cache.prune(&live);

    assert!(
        dir.path().join(format!("{keep}.cwasm")).exists(),
        "live hash must be kept"
    );
    assert!(
        !dir.path().join(format!("{remove}.cwasm")).exists(),
        "unlisted cwasm must be removed"
    );
    assert!(
        dir.path().join(format!("{other_ext}.wasm")).exists(),
        "non-cwasm files must not be touched"
    );
}

#[test]
fn sha256_hex_is_deterministic_and_64_chars() {
    let bytes = b"hello world";
    let h1 = WasmModuleCache::sha256_hex(bytes);
    let h2 = WasmModuleCache::sha256_hex(bytes);
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
    assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn warm_hit_is_significantly_faster_than_cold() {
    // Named `test-abi`, not `kani-test-abi`: the build strips the crate prefix. This
    // asked for a file that has never existed, so it silently returned and passed.
    let bytes = load_wasm("test-abi").expect(
        "wasm_sources/test-abi.wasm is missing — build it with: \
         cargo run -p kani-cli -- build --dev",
    );

    let dir = tempfile::tempdir().unwrap();
    let rt = WasmRuntime::new_on_demand().unwrap();

    let sha256 = WasmModuleCache::sha256_hex(&bytes);

    let cold_start = std::time::Instant::now();
    let compiled = rt.compile_component(&bytes).unwrap();
    let cold_elapsed = cold_start.elapsed();

    let mut seed_cache = WasmModuleCache::new(dir.path().to_path_buf()).unwrap();
    seed_cache.insert(&sha256, compiled);
    drop(seed_cache);

    let mut warm_cache = WasmModuleCache::new(dir.path().to_path_buf()).unwrap();
    let warm_start = std::time::Instant::now();
    let warm = warm_cache.try_get(rt.engine(), &sha256).unwrap();
    let warm_elapsed = warm_start.elapsed();

    assert!(warm.is_some(), "disk-cached component must deserialize");
    assert!(
        cold_elapsed > warm_elapsed * 10,
        "disk deserialize ({warm_elapsed:?}) should be >10× faster than cold compile ({cold_elapsed:?})"
    );
}
