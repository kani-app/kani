use arc_swap::ArcSwap;
use dashmap::DashMap;
use std::sync::Arc;

use super::SourceBackend;

pub struct SourceRegistry {
    slots: DashMap<i64, Arc<ArcSwap<SourceBackend>>>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self {
            slots: DashMap::new(),
        }
    }

    pub fn get_backend(&self, id: i64) -> Option<Arc<SourceBackend>> {
        self.slots.get(&id).map(|r| r.value().load_full())
    }

    /// Every registered source, for passes that must visit all of them.
    pub(crate) fn entries(&self) -> Vec<(i64, Arc<SourceBackend>)> {
        self.slots
            .iter()
            .map(|entry| (*entry.key(), entry.value().load_full()))
            .collect()
    }

    pub fn insert(&self, id: i64, backend: SourceBackend) {
        match self.slots.get(&id) {
            Some(slot) => slot.store(Arc::new(backend)),
            None => {
                self.slots
                    .insert(id, Arc::new(ArcSwap::new(Arc::new(backend))));
            }
        }
    }

    #[cfg(test)]
    pub fn remove(&self, id: i64) {
        self.slots.remove(&id);
    }

    pub(crate) async fn remove_and_shutdown(&self, id: i64, reason: &str) -> bool {
        let backend = self.slots.remove(&id).map(|(_, slot)| slot.load_full());
        if let Some(backend) = backend {
            backend.retire_v8(reason).await;
            true
        } else {
            false
        }
    }

    pub(crate) async fn shutdown_all(&self, reason: &str) {
        let backends: Vec<_> = self
            .slots
            .iter()
            .map(|entry| entry.value().load_full())
            .collect();
        futures::future::join_all(backends.iter().map(|backend| backend.shutdown_v8(reason))).await;
    }

    pub async fn retire_all(&self, reason: &str) {
        let backends: Vec<_> = self
            .slots
            .iter()
            .map(|entry| entry.value().load_full())
            .collect();
        futures::future::join_all(backends.iter().map(|backend| backend.retire_v8(reason))).await;
    }

    pub fn contains_key(&self, id: i64) -> bool {
        self.slots.contains_key(&id)
    }

    pub fn active_ids(&self) -> Vec<i64> {
        self.slots.iter().map(|r| *r.key()).collect()
    }

    pub fn update_preferences(&self, id: i64, prefs: std::collections::HashMap<String, String>) {
        if let Some(backend) = self.get_backend(id) {
            backend.update_preferences(prefs);
        }
    }

    pub async fn hot_swap(&self, id: i64, new_backend: SourceBackend) {
        const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

        // Clone the Arc out of the DashMap shard immediately so the shard lock
        // (a !Send parking_lot guard) is not held across any .await point.
        let slot = self.slots.get(&id).map(|r| Arc::clone(r.value()));

        if let Some(slot) = slot {
            let old = slot.load_full();
            if let SourceBackend::Wasm(ref w) = *old {
                w.drain(DRAIN_TIMEOUT).await;
            }
            old.retire_v8("source-hot-swap").await;
            slot.store(Arc::new(new_backend));
        } else {
            self.slots
                .insert(id, Arc::new(ArcSwap::new(Arc::new(new_backend))));
        }
    }
}

impl Default for SourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::source::{SourceBackend, YamlSource};

    fn yaml() -> SourceBackend {
        SourceBackend::Yaml(Box::new(YamlSource::for_test()))
    }

    #[test]
    fn insert_makes_backend_retrievable() {
        let reg = SourceRegistry::new();
        reg.insert(1, yaml());
        assert!(reg.get_backend(1).is_some());
    }

    #[test]
    fn get_backend_returns_none_for_unknown_id() {
        let reg = SourceRegistry::new();
        assert!(reg.get_backend(99).is_none());
    }

    #[test]
    fn remove_deletes_entry() {
        let reg = SourceRegistry::new();
        reg.insert(1, yaml());
        reg.remove(1);
        assert!(reg.get_backend(1).is_none());
    }

    #[tokio::test]
    async fn remove_and_shutdown_deletes_entry() {
        let reg = SourceRegistry::new();
        reg.insert(1, yaml());
        assert!(reg.remove_and_shutdown(1, "test-remove").await);
        assert!(!reg.contains_key(1));
        assert!(!reg.remove_and_shutdown(1, "test-remove-again").await);
    }

    #[test]
    fn contains_key_reflects_presence() {
        let reg = SourceRegistry::new();
        assert!(!reg.contains_key(1));
        reg.insert(1, yaml());
        assert!(reg.contains_key(1));
        reg.remove(1);
        assert!(!reg.contains_key(1));
    }

    #[test]
    fn active_ids_returns_all_inserted_keys() {
        let reg = SourceRegistry::new();
        reg.insert(10, yaml());
        reg.insert(20, yaml());
        reg.insert(30, yaml());
        let mut ids = reg.active_ids();
        ids.sort_unstable();
        assert_eq!(ids, vec![10, 20, 30]);
    }

    #[test]
    fn active_ids_excludes_removed_keys() {
        let reg = SourceRegistry::new();
        reg.insert(1, yaml());
        reg.insert(2, yaml());
        reg.remove(1);
        assert_eq!(reg.active_ids(), vec![2]);
    }

    #[tokio::test]
    async fn hot_swap_replaces_existing_backend() {
        let reg = SourceRegistry::new();
        reg.insert(1, yaml());
        let before = reg.get_backend(1).unwrap();
        reg.hot_swap(1, yaml()).await;
        let after = reg.get_backend(1).unwrap();
        assert!(!Arc::ptr_eq(&before, &after));
    }

    #[tokio::test]
    async fn hot_swap_inserts_when_slot_absent() {
        let reg = SourceRegistry::new();
        assert!(!reg.contains_key(5));
        reg.hot_swap(5, yaml()).await;
        assert!(reg.contains_key(5));
    }

    #[test]
    fn second_insert_replaces_backend_atomically() {
        let reg = SourceRegistry::new();
        reg.insert(1, yaml());
        let first = reg.get_backend(1).unwrap();
        reg.insert(1, yaml());
        let second = reg.get_backend(1).unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
    }
}
