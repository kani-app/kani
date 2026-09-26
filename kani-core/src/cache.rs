//! Bounded, namespace-aware binary caching used by extension host imports.

use dashmap::DashMap;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const DEFAULT_MAX_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_NAMESPACE_MAX_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_NAMESPACE_MAX_ENTRIES: usize = 4096;

const NO_EXPIRY: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);

/// Maps a zero TTL to "never expires"; such an entry leaves only by eviction or deletion.
pub fn effective_ttl(ttl: Duration) -> Duration {
    if ttl.is_zero() { NO_EXPIRY } else { ttl }
}

/// Async cache backend trait. Implementations may be in-memory or persistent.
#[async_trait::async_trait]
pub trait CacheBackend: Send + Sync + 'static {
    async fn get(&self, namespace: &str, key: &str) -> Option<Vec<u8>>;
    async fn put(&self, namespace: &str, key: &str, value: Vec<u8>, ttl: Duration) {
        self.put_limited(
            namespace,
            key,
            value,
            ttl,
            kani_shared::MAX_CACHE_NAMESPACE_ENTRIES as usize,
        )
        .await
    }
    /// As [`CacheBackend::put`], evicting down to `max_entries` (never above the host limit).
    async fn put_limited(
        &self,
        namespace: &str,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
        max_entries: usize,
    );
    async fn delete(&self, namespace: &str, key: &str);
    async fn clear_namespace(&self, namespace: &str);
    /// Removes every namespace whose name starts with `prefix`.
    async fn clear_namespaces_with_prefix(&self, prefix: &str);
    async fn prune_expired(&self);
}

pub async fn get_or_insert<F, Fut>(
    cache: &dyn CacheBackend,
    namespace: &str,
    key: &str,
    ttl: Duration,
    compute: F,
) -> Vec<u8>
where
    F: FnOnce() -> Fut + Send,
    Fut: std::future::Future<Output = Vec<u8>> + Send,
{
    if let Some(cached) = cache.get(namespace, key).await {
        return cached;
    }
    let value = compute().await;
    cache.put(namespace, key, value.clone(), ttl).await;
    value
}

struct Entry {
    key: String,
    value: Vec<u8>,
    expires_at: Instant,
}

struct NamespaceState {
    entries: VecDeque<Entry>,
    total_bytes: usize,
}

impl NamespaceState {
    fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            total_bytes: 0,
        }
    }

    fn get(&self, key: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|e| e.key == key && e.expires_at > Instant::now())
            .map(|e| e.value.as_slice())
    }

    fn put(&mut self, key: String, value: Vec<u8>, expires_at: Instant, max_entries: usize) {
        self.delete(&key);
        let byte_len = value.len();
        let max_entries = max_entries.clamp(1, DEFAULT_NAMESPACE_MAX_ENTRIES);
        while self.total_bytes + byte_len >= DEFAULT_NAMESPACE_MAX_BYTES
            || self.entries.len() >= max_entries
        {
            if let Some(evicted) = self.entries.pop_front() {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.value.len());
            } else {
                break;
            }
        }
        self.total_bytes += byte_len;
        self.entries.push_back(Entry {
            key,
            value,
            expires_at,
        });
    }

    fn delete(&mut self, key: &str) {
        if let Some(pos) = self.entries.iter().position(|e| e.key == key) {
            let removed = self.entries.remove(pos).expect("just indexed");
            self.total_bytes = self.total_bytes.saturating_sub(removed.value.len());
        }
    }

    fn prune(&mut self) {
        let now = Instant::now();
        self.entries.retain(|e| {
            if e.expires_at > now {
                true
            } else {
                self.total_bytes = self.total_bytes.saturating_sub(e.value.len());
                false
            }
        });
    }

    fn byte_size(&self) -> usize {
        self.total_bytes
    }
}

/// TTL cache with per-namespace entry/byte caps and a configurable global byte cap.
/// Eviction follows insertion order within the selected namespace.
pub struct InMemoryCache {
    namespaces: Arc<DashMap<String, Mutex<NamespaceState>>>,
    max_global_bytes: usize,
}

impl InMemoryCache {
    pub fn new() -> Self {
        Self::with_max_bytes(DEFAULT_MAX_BYTES)
    }

    pub(crate) fn with_max_bytes(max_global_bytes: usize) -> Self {
        Self {
            namespaces: Arc::new(DashMap::new()),
            max_global_bytes,
        }
    }

    fn total_bytes(&self) -> usize {
        self.namespaces
            .iter()
            .filter_map(|ns| ns.value().lock().ok().map(|s| s.byte_size()))
            .sum()
    }

    fn evict_lru_globally(&self) {
        let ns_entry = self
            .namespaces
            .iter()
            .max_by_key(|ns| ns.value().lock().ok().map(|s| s.byte_size()).unwrap_or(0));
        if let Some(ns) = ns_entry
            && let Ok(mut state) = ns.value().lock()
            && let Some(evicted) = state.entries.pop_front()
        {
            state.total_bytes = state.total_bytes.saturating_sub(evicted.value.len());
        }
    }
}

impl Default for InMemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl CacheBackend for InMemoryCache {
    async fn get(&self, namespace: &str, key: &str) -> Option<Vec<u8>> {
        self.namespaces.get(namespace).and_then(|ns| {
            ns.value()
                .lock()
                .ok()
                .and_then(|s| s.get(key).map(|v| v.to_vec()))
        })
    }

    async fn put_limited(
        &self,
        namespace: &str,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
        max_entries: usize,
    ) {
        let expires_at = Instant::now() + effective_ttl(ttl);
        while self.total_bytes() + value.len() > self.max_global_bytes {
            self.evict_lru_globally();
        }
        self.namespaces
            .entry(namespace.to_string())
            .or_insert_with(|| Mutex::new(NamespaceState::new()))
            .value()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .put(key.to_string(), value, expires_at, max_entries);
    }

    async fn delete(&self, namespace: &str, key: &str) {
        if let Some(ns) = self.namespaces.get(namespace)
            && let Ok(mut state) = ns.value().lock()
        {
            state.delete(key);
        }
    }

    async fn clear_namespace(&self, namespace: &str) {
        self.namespaces.remove(namespace);
    }

    async fn clear_namespaces_with_prefix(&self, prefix: &str) {
        self.namespaces.retain(|name, _| !name.starts_with(prefix));
    }

    async fn prune_expired(&self) {
        for ns in self.namespaces.iter() {
            if let Ok(mut state) = ns.value().lock() {
                state.prune();
            }
        }
        self.namespaces
            .retain(|_, v| v.get_mut().map(|s| !s.entries.is_empty()).unwrap_or(true));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    async fn put_and_get() {
        let cache = InMemoryCache::new();
        cache
            .put("ns", "key", b"hello".to_vec(), Duration::from_secs(60))
            .await;
        assert_eq!(cache.get("ns", "key").await, Some(b"hello".to_vec()));
    }

    #[tokio::test]
    async fn expired_entry_returns_none() {
        let cache = InMemoryCache::new();
        cache
            .put("ns", "key", b"v".to_vec(), Duration::from_millis(1))
            .await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(cache.get("ns", "key").await, None);
    }

    #[tokio::test]
    async fn prefix_clear_spares_other_extensions() {
        let cache = InMemoryCache::new();
        let ttl = Duration::from_secs(60);
        for ns in ["a:", "a:auth", "ab:", "b:a:"] {
            cache.put(ns, "k", b"v".to_vec(), ttl).await;
        }
        cache.clear_namespaces_with_prefix("a:").await;
        assert_eq!(cache.get("a:", "k").await, None);
        assert_eq!(cache.get("a:auth", "k").await, None);
        assert!(cache.get("ab:", "k").await.is_some());
        assert!(cache.get("b:a:", "k").await.is_some());
    }

    #[tokio::test]
    async fn put_limited_evicts_down_to_max_entries() {
        let cache = InMemoryCache::new();
        let ttl = Duration::from_secs(60);
        for key in ["a", "b", "c"] {
            cache.put_limited("ns", key, b"v".to_vec(), ttl, 2).await;
        }
        assert_eq!(
            cache.get("ns", "a").await,
            None,
            "the oldest entry is evicted"
        );
        assert!(cache.get("ns", "b").await.is_some());
        assert!(cache.get("ns", "c").await.is_some());
    }

    #[tokio::test]
    async fn zero_ttl_never_expires() {
        let cache = InMemoryCache::new();
        cache.put("ns", "key", b"v".to_vec(), Duration::ZERO).await;
        cache.prune_expired().await;
        assert_eq!(cache.get("ns", "key").await, Some(b"v".to_vec()));
    }

    #[tokio::test]
    async fn delete_removes_entry() {
        let cache = InMemoryCache::new();
        cache
            .put("ns", "key", b"v".to_vec(), Duration::from_secs(60))
            .await;
        cache.delete("ns", "key").await;
        assert_eq!(cache.get("ns", "key").await, None);
    }

    #[tokio::test]
    async fn clear_namespace_removes_all() {
        let cache = InMemoryCache::new();
        cache
            .put("ns", "k1", b"v1".to_vec(), Duration::from_secs(60))
            .await;
        cache
            .put("ns", "k2", b"v2".to_vec(), Duration::from_secs(60))
            .await;
        cache.clear_namespace("ns").await;
        assert_eq!(cache.get("ns", "k1").await, None);
        assert_eq!(cache.get("ns", "k2").await, None);
    }

    #[tokio::test]
    async fn namespace_isolation() {
        let cache = InMemoryCache::new();
        cache
            .put("ns1", "key", b"a".to_vec(), Duration::from_secs(60))
            .await;
        cache
            .put("ns2", "key", b"b".to_vec(), Duration::from_secs(60))
            .await;
        assert_eq!(cache.get("ns1", "key").await, Some(b"a".to_vec()));
        assert_eq!(cache.get("ns2", "key").await, Some(b"b".to_vec()));
    }

    #[tokio::test]
    async fn namespace_cap_evicts_lru() {
        let cache = InMemoryCache::new();
        let large_value = vec![0u8; DEFAULT_NAMESPACE_MAX_BYTES - 1];
        cache
            .put("ns", "big", large_value, Duration::from_secs(60))
            .await;
        cache
            .put("ns", "small", b"x".to_vec(), Duration::from_secs(60))
            .await;
        assert_eq!(
            cache.get("ns", "big").await,
            None,
            "big entry should be evicted to make room"
        );
        assert_eq!(cache.get("ns", "small").await, Some(b"x".to_vec()));
    }

    #[tokio::test]
    async fn prune_removes_expired() {
        let cache = InMemoryCache::new();
        cache
            .put("ns", "key", b"v".to_vec(), Duration::from_millis(1))
            .await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        cache.prune_expired().await;
        assert_eq!(cache.get("ns", "key").await, None);
    }

    #[tokio::test]
    async fn version_scoped_namespace_isolation() {
        let cache = InMemoryCache::new();
        let ns_v1 = "ext:1.0:session";
        let ns_v2 = "ext:2.0:session";
        cache
            .put(ns_v1, "k", b"v1".to_vec(), Duration::from_secs(60))
            .await;
        cache
            .put(ns_v2, "k", b"v2".to_vec(), Duration::from_secs(60))
            .await;
        assert_eq!(cache.get(ns_v1, "k").await, Some(b"v1".to_vec()));
        assert_eq!(cache.get(ns_v2, "k").await, Some(b"v2".to_vec()));
    }

    #[tokio::test]
    async fn get_or_insert_computes_on_miss() {
        let cache = InMemoryCache::new();
        let mut computed = false;
        let value = get_or_insert(&cache, "ns", "key", Duration::from_secs(60), || {
            computed = true;
            async { b"computed".to_vec() }
        })
        .await;
        assert_eq!(value, b"computed".to_vec());
        assert!(computed);
        assert_eq!(cache.get("ns", "key").await, Some(b"computed".to_vec()));
    }

    #[tokio::test]
    async fn get_or_insert_returns_cached_on_hit() {
        let cache = InMemoryCache::new();
        cache
            .put("ns", "key", b"cached".to_vec(), Duration::from_secs(60))
            .await;
        let mut computed = false;
        let value = get_or_insert(&cache, "ns", "key", Duration::from_secs(60), || {
            computed = true;
            async { b"should_not_compute".to_vec() }
        })
        .await;
        assert_eq!(value, b"cached".to_vec());
        assert!(!computed, "compute must not be called on cache hit");
    }
}
