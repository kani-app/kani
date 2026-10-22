//! Hot and persistent caching for compiled Wasmtime components.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use wasmtime::Engine;
use wasmtime::component::Component;

use crate::error::Error;

/// Two-level cache of compiled WASM components, keyed by the source component's SHA-256 digest.
/// Entries are held in memory and persisted as Wasmtime serialized components.
pub struct WasmModuleCache {
    cache_dir: PathBuf,
    hot: HashMap<String, Component>,
}

impl WasmModuleCache {
    pub fn new(cache_dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self {
            cache_dir,
            hot: HashMap::new(),
        })
    }

    /// Resolve the cache directory for a given data directory.
    pub fn resolve_dir(data_dir: &std::path::Path) -> PathBuf {
        std::env::var("KANI_WASM_MODULE_CACHE_DIR")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join(".wasm_cache"))
    }

    /// Returns the cache, or the directory and error that stopped it being
    /// created so the caller can report the degradation.
    pub fn open(data_dir: &std::path::Path) -> Result<Self, (PathBuf, std::io::Error)> {
        let dir = Self::resolve_dir(data_dir);
        Self::new(dir.clone()).map_err(|e| (dir, e))
    }

    pub fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    pub fn try_get(&mut self, engine: &Engine, sha256: &str) -> Result<Option<Component>, Error> {
        if let Some(c) = self.hot.get(sha256) {
            return Ok(Some(c.clone()));
        }

        let path = self.cache_dir.join(format!("{sha256}.cwasm"));
        if path.exists() {
            // SAFETY: file was written by this process using the same engine and wasmtime
            // version. If deserialization fails (stale or corrupt), we remove it and return
            // None so the caller falls through to a cold compile.
            match unsafe { Component::deserialize_file(engine, &path) } {
                Ok(c) => {
                    self.hot.insert(sha256.to_string(), c.clone());
                    return Ok(Some(c));
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }

        Ok(None)
    }

    pub fn insert(&mut self, sha256: &str, component: Component) {
        let path = self.cache_dir.join(format!("{sha256}.cwasm"));
        if let Ok(serialized) = component.serialize() {
            let _ = std::fs::write(&path, &serialized);
        }
        self.hot.insert(sha256.to_string(), component);
    }

    pub fn prune(&self, live_hashes: &HashSet<String>) {
        let Ok(entries) = std::fs::read_dir(&self.cache_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("cwasm") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && !live_hashes.contains(stem)
            {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}
