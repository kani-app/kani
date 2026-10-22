//! Subsystems that are running, but not fully.
//!
//! Kani prefers to keep serving when a part of it fails: a missing `secret.key`
//! disables encrypted settings rather than refusing to boot, an unwritable cache
//! directory costs speed rather than correctness.
//!
//! Anything that degrades instead of failing registers here, and the registry is
//! surfaced in Settings → Diagnostics and (for `Error`) in an admin banner.

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::RwLock;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
/// Operator-visible impact level of a subsystem degradation.
pub enum Severity {
    /// Working, but worse — slower, or with a feature unavailable.
    Warn,
    /// A feature the operator configured is not functioning.
    Error,
}

/// A stable identifier per condition, so re-registering updates in place and a
/// subsystem that recovers can clear itself.
pub mod ids {
    pub(crate) const CREDENTIAL_KEY: &str = "credential_key";
    pub const ENCRYPTED_SETTINGS: &str = "encrypted_settings";
    pub(crate) const CREDENTIAL_MIGRATION: &str = "credential_migration";
    pub const TRACKER_CREDENTIALS: &str = "tracker_credentials";
    pub const WASM_MODULE_CACHE: &str = "wasm_module_cache";
    pub(crate) const STORAGE_DIRECTORY: &str = "storage_directory";
    pub const LIBRARY_PATH: &str = "library_path";
    pub(crate) const SOURCE_REGISTRY: &str = "source_registry";
    pub const SOLVER_EGRESS_GUARD: &str = "solver_egress_guard";

    /// One id per source, so several broken extensions each report themselves
    /// instead of overwriting one shared entry.
    pub(crate) fn source_load(source_name: &str) -> String {
        format!("source_load:{source_name}")
    }
}

#[derive(Debug, Clone, Serialize)]
/// Current reduced-service condition with an operator-facing remedy.
pub struct Degradation {
    pub id: String,
    pub severity: Severity,
    /// Short subsystem name, e.g. "Encrypted settings".
    pub title: String,
    /// What is actually wrong, including the underlying error.
    pub detail: String,
    /// What the operator can do about it. Every degradation has one — a report
    /// with no remedy is just a different place to be told bad news.
    pub remedy: String,
    #[serde(with = "time::serde::rfc3339")]
    pub since: OffsetDateTime,
}

#[derive(Debug, Default)]
/// Thread-safe registry keyed by stable degradation identifier.
/// Re-registering an identifier replaces its details; recovery removes it.
pub struct DegradationRegistry {
    entries: RwLock<BTreeMap<String, Degradation>>,
}

impl DegradationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a degradation, replacing any previous entry with the same id.
    ///
    /// Also logs at the matching level for operators monitoring the journal.
    pub fn register(
        &self,
        id: &str,
        severity: Severity,
        title: impl Into<String>,
        detail: impl Into<String>,
        remedy: impl Into<String>,
    ) {
        let entry = Degradation {
            id: id.to_string(),
            severity,
            title: title.into(),
            detail: detail.into(),
            remedy: remedy.into(),
            since: OffsetDateTime::now_utc(),
        };

        match severity {
            Severity::Error => tracing::error!(
                degraded = id,
                remedy = %entry.remedy,
                "{}: {}",
                entry.title,
                entry.detail
            ),
            Severity::Warn => tracing::warn!(
                degraded = id,
                remedy = %entry.remedy,
                "{}: {}",
                entry.title,
                entry.detail
            ),
        }

        if let Ok(mut map) = self.entries.write() {
            map.insert(id.to_string(), entry);
        }
    }

    /// Drop a degradation once the subsystem is healthy again.
    pub fn clear(&self, id: &str) {
        if let Ok(mut map) = self.entries.write()
            && map.remove(id).is_some()
        {
            tracing::info!(recovered = id, "Degraded subsystem recovered");
        }
    }

    /// Worst first, so the UI can lead with what matters.
    pub fn list(&self) -> Vec<Degradation> {
        let Ok(map) = self.entries.read() else {
            return Vec::new();
        };
        let mut out: Vec<_> = map.values().cloned().collect();
        out.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.id.cmp(&b.id)));
        out
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().map(|m| m.is_empty()).unwrap_or(true)
    }
}

/// Emitted once at the end of startup so a degraded boot is visible in the log
/// without reading every preceding line.
pub(crate) fn log_startup_summary(registry: &DegradationRegistry) {
    let list = registry.list();
    if list.is_empty() {
        return;
    }
    let errors = list
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let names: Vec<&str> = list.iter().map(|d| d.title.as_str()).collect();
    tracing::error!(
        degraded_count = list.len(),
        error_count = errors,
        "Started with {} degraded subsystem(s): {}. See Settings → Diagnostics.",
        list.len(),
        names.join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registering_the_same_id_twice_replaces_rather_than_duplicates() {
        let r = DegradationRegistry::new();
        r.register(
            ids::WASM_MODULE_CACHE,
            Severity::Warn,
            "Cache",
            "first",
            "fix",
        );
        r.register(
            ids::WASM_MODULE_CACHE,
            Severity::Warn,
            "Cache",
            "second",
            "fix",
        );
        let list = r.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].detail, "second");
    }

    #[test]
    fn a_recovered_subsystem_clears() {
        let r = DegradationRegistry::new();
        r.register(ids::LIBRARY_PATH, Severity::Error, "Library", "gone", "fix");
        assert!(!r.is_empty());
        r.clear(ids::LIBRARY_PATH);
        assert!(r.is_empty());
    }

    #[test]
    fn clearing_something_never_registered_is_harmless() {
        let r = DegradationRegistry::new();
        r.clear(ids::LIBRARY_PATH);
        assert!(r.is_empty());
    }

    #[test]
    fn errors_sort_before_warnings() {
        let r = DegradationRegistry::new();
        r.register(ids::WASM_MODULE_CACHE, Severity::Warn, "Cache", "d", "fix");
        r.register(
            ids::ENCRYPTED_SETTINGS,
            Severity::Error,
            "Settings",
            "d",
            "fix",
        );
        let list = r.list();
        assert_eq!(list[0].severity, Severity::Error);
        assert_eq!(list[1].severity, Severity::Warn);
    }

    #[test]
    fn an_empty_registry_lists_nothing() {
        assert!(DegradationRegistry::new().list().is_empty());
    }
}
