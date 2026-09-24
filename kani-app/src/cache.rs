//! Request-result caches and their cross-entity invalidation helpers.

use crate::models::ReadingStats;
use dashmap::DashMap;
use kani_core::cache::CacheBackend;
use moka::future::Cache;
use sqlx::SqlitePool;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

type LibraryListingCache =
    Cache<(i64, u64, i32, i32), Arc<(Vec<crate::models::LibraryManga>, bool, Option<u32>)>>;

#[derive(Clone)]
/// Bounded application read caches shared by cloned [`crate::AppService`] handles.
/// Mutating service methods must invalidate every affected key through this type's helpers.
pub struct RequestCache {
    manga_details: Cache<(i64, String), String>,
    popular_manga: Cache<(i64, i32, i32, String), String>,
    chapter_list: Cache<(i64, String, i32, i32, String), String>,
    pages: Cache<(i64, String, String), String>,
    search_results: Cache<(i64, String, i32, i32, String), String>,
    pub preference_schema: DashMap<i64, Vec<kani_core::PreferenceSpec>>,
    /// Filter defaults an extension declares, by source. Fixed until the
    /// extension is replaced, and consulted on every browse.
    filter_defaults: DashMap<i64, Vec<kani_shared::types::ActiveFilter>>,
    /// Reading stats keyed by (user_id, period_days). 10-minute TTL.
    pub stats: Cache<(i64, i32), Arc<ReadingStats>>,
    /// CBZ page-index lists keyed by (chapter_id, file_mtime_unix). A changed file
    /// changes the key, so no invalidation hook is needed.
    cbz_pages: Cache<(i64, i64), Arc<Vec<String>>>,
    library_listing: Option<LibraryListingCache>,
}

impl RequestCache {
    pub fn new() -> Self {
        let library_ttl: u64 = std::env::var("KANI_LIBRARY_CACHE_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        Self::build_with_library_ttl(library_ttl)
    }

    #[cfg(any(test, feature = "test-util"))]
    pub fn new_with_library_ttl(ttl_secs: u64) -> Self {
        Self::build_with_library_ttl(ttl_secs)
    }

    fn build_with_library_ttl(library_ttl: u64) -> Self {
        let library_listing = (library_ttl > 0).then(|| {
            Cache::builder()
                .max_capacity(2000)
                .time_to_live(Duration::from_secs(library_ttl))
                .support_invalidation_closures()
                .build()
        });
        Self {
            manga_details: Cache::builder()
                .max_capacity(500)
                .time_to_live(Duration::from_secs(15 * 60))
                .support_invalidation_closures()
                .build(),
            popular_manga: Cache::builder()
                .max_capacity(200)
                .time_to_live(Duration::from_secs(5 * 60))
                .support_invalidation_closures()
                .build(),
            chapter_list: Cache::builder()
                .max_capacity(500)
                .time_to_live(Duration::from_secs(10 * 60))
                .support_invalidation_closures()
                .build(),
            pages: Cache::builder()
                .max_capacity(50)
                .time_to_live(Duration::from_secs(10 * 60))
                .support_invalidation_closures()
                .build(),
            search_results: Cache::builder()
                .max_capacity(300)
                .time_to_live(Duration::from_secs(90))
                .support_invalidation_closures()
                .build(),
            preference_schema: DashMap::new(),
            filter_defaults: DashMap::new(),
            stats: Cache::builder()
                .max_capacity(500)
                .time_to_live(Duration::from_secs(10 * 60))
                .support_invalidation_closures()
                .build(),
            cbz_pages: Cache::builder()
                .max_capacity(crate::tuning::OPDS_PAGE_INDEX_CACHE_ENTRIES)
                .time_to_live(Duration::from_secs(10 * 60))
                .build(),
            library_listing,
        }
    }

    pub(crate) async fn cbz_pages_get(&self, key: (i64, i64)) -> Option<Arc<Vec<String>>> {
        self.cbz_pages.get(&key).await
    }

    pub(crate) async fn cbz_pages_put(&self, key: (i64, i64), value: Arc<Vec<String>>) {
        self.cbz_pages.insert(key, value).await;
    }

    pub(crate) async fn get_or_fetch_manga_details<F, E>(
        &self,
        source_id: i64,
        manga_id: &str,
        init: F,
    ) -> Result<String, Arc<E>>
    where
        F: Future<Output = Result<String, E>>,
        E: Send + Sync + 'static,
    {
        self.manga_details
            .try_get_with((source_id, manga_id.to_string()), init)
            .await
    }

    pub(crate) async fn get_or_fetch_popular_manga<F, E>(
        &self,
        source_id: i64,
        page: i32,
        page_size: i32,
        filters_key: String,
        init: F,
    ) -> Result<String, Arc<E>>
    where
        F: Future<Output = Result<String, E>>,
        E: Send + Sync + 'static,
    {
        self.popular_manga
            .try_get_with((source_id, page, page_size, filters_key), init)
            .await
    }

    pub(crate) async fn get_or_fetch_chapter_list<F, E>(
        &self,
        source_id: i64,
        manga_id: &str,
        page: i32,
        page_size: i32,
        sort: &str,
        init: F,
    ) -> Result<String, Arc<E>>
    where
        F: Future<Output = Result<String, E>>,
        E: Send + Sync + 'static,
    {
        self.chapter_list
            .try_get_with(
                (
                    source_id,
                    manga_id.to_string(),
                    page,
                    page_size,
                    sort.to_string(),
                ),
                init,
            )
            .await
    }

    pub(crate) async fn get_or_fetch_search_results<F, E>(
        &self,
        source_id: i64,
        query: &str,
        page: i32,
        page_size: i32,
        filters_key: String,
        init: F,
    ) -> Result<String, Arc<E>>
    where
        F: Future<Output = Result<String, E>>,
        E: Send + Sync + 'static,
    {
        self.search_results
            .try_get_with(
                (source_id, query.to_string(), page, page_size, filters_key),
                init,
            )
            .await
    }

    pub(crate) async fn get_or_fetch_pages<F, E>(
        &self,
        source_id: i64,
        manga_id: &str,
        chapter_id: &str,
        init: F,
    ) -> Result<String, Arc<E>>
    where
        F: Future<Output = Result<String, E>>,
        E: Send + Sync + 'static,
    {
        self.pages
            .try_get_with(
                (source_id, manga_id.to_string(), chapter_id.to_string()),
                init,
            )
            .await
    }

    pub(crate) fn get_preference_schema(
        &self,
        source_id: i64,
    ) -> Option<Vec<kani_core::PreferenceSpec>> {
        self.preference_schema
            .get(&source_id)
            .map(|r| r.value().clone())
    }

    pub(crate) fn insert_preference_schema(
        &self,
        source_id: i64,
        schema: Vec<kani_core::PreferenceSpec>,
    ) {
        self.preference_schema.insert(source_id, schema);
        // A replaced extension can declare different defaults.
        self.filter_defaults.remove(&source_id);
    }

    pub(crate) fn get_filter_defaults(
        &self,
        source_id: i64,
    ) -> Option<Vec<kani_shared::types::ActiveFilter>> {
        self.filter_defaults
            .get(&source_id)
            .map(|r| r.value().clone())
    }

    pub(crate) fn insert_filter_defaults(
        &self,
        source_id: i64,
        defaults: Vec<kani_shared::types::ActiveFilter>,
    ) {
        self.filter_defaults.insert(source_id, defaults);
    }

    pub(crate) async fn invalidate_chapter_list_for_manga(&self, source_id: i64, manga_id: &str) {
        let owned = manga_id.to_string();
        let _ = self.chapter_list.invalidate_entries_if(
            move |(sid, mid, _page, _page_size, _sort), _| *sid == source_id && *mid == owned,
        );
    }

    pub(crate) fn invalidate_stats(&self, user_id: crate::ids::UserId) {
        let raw = user_id.0;
        let _ = self
            .stats
            .invalidate_entries_if(move |(id, _period), _| *id == raw);
    }

    pub async fn get_library_listing(
        &self,
        user_id: i64,
        filter_hash: u64,
        page: i32,
        page_size: i32,
    ) -> Option<Arc<(Vec<crate::models::LibraryManga>, bool, Option<u32>)>> {
        self.library_listing
            .as_ref()?
            .get(&(user_id, filter_hash, page, page_size))
            .await
    }

    pub async fn insert_library_listing(
        &self,
        user_id: i64,
        filter_hash: u64,
        page: i32,
        page_size: i32,
        val: Arc<(Vec<crate::models::LibraryManga>, bool, Option<u32>)>,
    ) {
        if let Some(cache) = &self.library_listing {
            cache
                .insert((user_id, filter_hash, page, page_size), val)
                .await;
        }
    }

    pub fn invalidate_library(&self) {
        if let Some(cache) = &self.library_listing {
            cache.invalidate_all();
        }
    }

    pub fn clear_all(&self) {
        self.manga_details.invalidate_all();
        self.popular_manga.invalidate_all();
        self.chapter_list.invalidate_all();
        self.pages.invalidate_all();
        self.search_results.invalidate_all();
        self.stats.invalidate_all();
        self.invalidate_library();
    }

    pub(crate) fn invalidate_source(&self, source_id: i64) {
        let sid = source_id;
        let _ = self
            .manga_details
            .invalidate_entries_if(move |(id, _), _| *id == sid);
        let _ = self
            .popular_manga
            .invalidate_entries_if(move |(id, ..), _| *id == sid);
        let _ = self
            .chapter_list
            .invalidate_entries_if(move |(id, ..), _| *id == sid);
        let _ = self
            .pages
            .invalidate_entries_if(move |(id, ..), _| *id == sid);
        let _ = self
            .search_results
            .invalidate_entries_if(move |(id, ..), _| *id == sid);
        self.preference_schema.remove(&source_id);
    }
}

impl Default for RequestCache {
    fn default() -> Self {
        Self::new()
    }
}

const NS_MAX_BYTES: i64 = 4 * 1024 * 1024;
const NS_MAX_ROWS: i64 = 4096;

pub(crate) struct SqliteCache {
    pool: SqlitePool,
}

impl SqliteCache {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl CacheBackend for SqliteCache {
    async fn get(&self, namespace: &str, key: &str) -> Option<Vec<u8>> {
        let now = now_secs();
        sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT value FROM extension_cache WHERE namespace = ? AND key = ? AND expires_at > ?",
        )
        .bind(namespace)
        .bind(key)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .map(|(v,)| v)
    }

    async fn put(&self, namespace: &str, key: &str, value: Vec<u8>, ttl: Duration) {
        let expires_at = now_secs() + kani_core::cache::effective_ttl(ttl).as_secs() as i64;

        let _ = sqlx::query(
            "INSERT OR REPLACE INTO extension_cache (namespace, key, value, expires_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(namespace)
        .bind(key)
        .bind(&value)
        .bind(expires_at)
        .execute(&self.pool)
        .await;

        let _ = sqlx::query(
            "DELETE FROM extension_cache WHERE namespace = ? AND key IN (
                SELECT key FROM extension_cache WHERE namespace = ?
                ORDER BY expires_at ASC
                LIMIT MAX(0, (SELECT COUNT(*) FROM extension_cache WHERE namespace = ?) - ?)
             )",
        )
        .bind(namespace)
        .bind(namespace)
        .bind(namespace)
        .bind(NS_MAX_ROWS)
        .execute(&self.pool)
        .await;

        let _ = sqlx::query(
            "DELETE FROM extension_cache WHERE namespace = ? AND key IN (
                SELECT key FROM extension_cache WHERE namespace = ?
                ORDER BY expires_at ASC
                LIMIT MAX(0, (
                    SELECT MAX(0, SUM(LENGTH(value)) - ?) / MAX(1, AVG(LENGTH(value)))
                    FROM extension_cache WHERE namespace = ?
                ))
             )",
        )
        .bind(namespace)
        .bind(namespace)
        .bind(NS_MAX_BYTES)
        .bind(namespace)
        .execute(&self.pool)
        .await;
    }

    async fn delete(&self, namespace: &str, key: &str) {
        let _ = sqlx::query("DELETE FROM extension_cache WHERE namespace = ? AND key = ?")
            .bind(namespace)
            .bind(key)
            .execute(&self.pool)
            .await;
    }

    async fn clear_namespace(&self, namespace: &str) {
        let _ = sqlx::query("DELETE FROM extension_cache WHERE namespace = ?")
            .bind(namespace)
            .execute(&self.pool)
            .await;
    }

    async fn prune_expired(&self) {
        let now = now_secs();
        let _ = sqlx::query("DELETE FROM extension_cache WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await;
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn default_is_equivalent_to_new() {
        let _a = RequestCache::new();
        let _b = RequestCache::default();
    }

    #[tokio::test]
    async fn sqlite_cache_zero_ttl_never_expires() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE extension_cache (namespace TEXT NOT NULL, key TEXT NOT NULL, \
             value BLOB NOT NULL, expires_at INTEGER NOT NULL, PRIMARY KEY (namespace, key))",
        )
        .execute(&pool)
        .await
        .unwrap();
        let cache = SqliteCache::new(pool);

        cache.put("ns", "key", b"v".to_vec(), Duration::ZERO).await;
        cache.prune_expired().await;

        assert_eq!(cache.get("ns", "key").await, Some(b"v".to_vec()));
    }

    #[tokio::test]
    async fn manga_details_miss_calls_init() {
        let cache = RequestCache::new();
        let result = cache
            .get_or_fetch_manga_details(1, "m1", async {
                Ok::<_, std::io::Error>("data".to_string())
            })
            .await;
        assert_eq!(result.unwrap().as_str(), "data");
    }

    #[tokio::test]
    async fn manga_details_hit_returns_cached_without_reinvoking_init() {
        let cache = RequestCache::new();
        let calls = Arc::new(AtomicU32::new(0));

        for _ in 0..3 {
            let c = calls.clone();
            let _ = cache
                .get_or_fetch_manga_details(1, "m1", async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, std::io::Error>("data".to_string())
                })
                .await
                .unwrap();
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "init should only be called once"
        );
    }

    #[tokio::test]
    async fn manga_details_different_keys_are_independent() {
        let cache = RequestCache::new();
        let _ = cache
            .get_or_fetch_manga_details(1, "m1", async { Ok::<_, std::io::Error>("A".to_string()) })
            .await
            .unwrap();
        let v2 = cache
            .get_or_fetch_manga_details(1, "m2", async { Ok::<_, std::io::Error>("B".to_string()) })
            .await
            .unwrap();
        assert_eq!(v2.as_str(), "B");
    }

    #[tokio::test]
    async fn popular_manga_caches_per_page() {
        let cache = RequestCache::new();
        let _ = cache
            .get_or_fetch_popular_manga(1, 1, 20, "".to_string(), async {
                Ok::<_, std::io::Error>("p1".to_string())
            })
            .await
            .unwrap();
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let _ = cache
            .get_or_fetch_popular_manga(1, 2, 20, "".to_string(), async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>("p2".to_string())
            })
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn search_results_cached_for_same_query() {
        let cache = RequestCache::new();
        let calls = Arc::new(AtomicU32::new(0));

        for _ in 0..2 {
            let c = calls.clone();
            let _ = cache
                .get_or_fetch_search_results(1, "berserk", 1, 20, "".to_string(), async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, std::io::Error>("results".to_string())
                })
                .await
                .unwrap();
        }

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn invalidate_chapter_list_removes_matching_entries() {
        let cache = RequestCache::new();
        let calls = Arc::new(AtomicU32::new(0));

        let c = calls.clone();
        let _ = cache
            .get_or_fetch_chapter_list(1, "m1", 1, 20, "", async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>("chapters".to_string())
            })
            .await
            .unwrap();

        cache.invalidate_chapter_list_for_manga(1, "m1").await;
        cache.chapter_list.run_pending_tasks().await;
        std::thread::sleep(std::time::Duration::from_millis(200));
        cache.chapter_list.run_pending_tasks().await;

        let c = calls.clone();
        let _ = cache
            .get_or_fetch_chapter_list(1, "m1", 1, 20, "", async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>("chapters".to_string())
            })
            .await
            .unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "init should be called again after invalidation"
        );
    }

    #[tokio::test]
    async fn invalidate_chapter_list_does_not_affect_other_manga() {
        let cache = RequestCache::new();
        let calls = Arc::new(AtomicU32::new(0));

        let _ = cache
            .get_or_fetch_chapter_list(1, "m1", 1, 20, "", async {
                Ok::<_, std::io::Error>("m1".to_string())
            })
            .await
            .unwrap();
        let c = calls.clone();
        let _ = cache
            .get_or_fetch_chapter_list(1, "m2", 1, 20, "", async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>("m2".to_string())
            })
            .await
            .unwrap();

        cache.invalidate_chapter_list_for_manga(1, "m1").await;
        cache.chapter_list.run_pending_tasks().await;
        std::thread::sleep(std::time::Duration::from_millis(200));
        cache.chapter_list.run_pending_tasks().await;

        let c = calls.clone();
        let _ = cache
            .get_or_fetch_chapter_list(1, "m2", 1, 20, "", async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>("m2".to_string())
            })
            .await
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1, "m2 should remain cached");
    }

    #[tokio::test]
    async fn a_failed_page_fetch_is_not_cached_as_a_success() {
        let cache = RequestCache::new();
        let calls = Arc::new(AtomicU32::new(0));

        let c = calls.clone();
        let first = cache
            .get_or_fetch_pages(1, "m1", "c1", async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<String, std::io::Error>(std::io::Error::other("upstream 500"))
            })
            .await;
        assert!(first.is_err(), "the failing fetch surfaces its error");

        let c = calls.clone();
        let second = cache
            .get_or_fetch_pages(1, "m1", "c1", async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>("pages".to_string())
            })
            .await
            .unwrap();

        assert_eq!(
            second.as_str(),
            "pages",
            "the retry succeeds, not a cached error"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "init must run again after a failure — the error was not cached"
        );
    }

    #[tokio::test]
    async fn a_failed_chapter_list_fetch_is_not_cached_as_a_success() {
        let cache = RequestCache::new();
        let calls = Arc::new(AtomicU32::new(0));

        let c = calls.clone();
        let first = cache
            .get_or_fetch_chapter_list(1, "m1", 1, 20, "", async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err::<String, std::io::Error>(std::io::Error::other("upstream 500"))
            })
            .await;
        assert!(first.is_err());

        let c = calls.clone();
        let second = cache
            .get_or_fetch_chapter_list(1, "m1", 1, 20, "", async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>("chapters".to_string())
            })
            .await
            .unwrap();

        assert_eq!(second.as_str(), "chapters");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn preference_schema_insert_and_get() {
        let cache = RequestCache::new();
        cache.insert_preference_schema(1, vec![]);
        assert!(cache.get_preference_schema(1).is_some());
    }

    #[test]
    fn preference_schema_missing_source_returns_none() {
        let cache = RequestCache::new();
        assert!(cache.get_preference_schema(99).is_none());
    }

    #[test]
    fn invalidate_source_removes_preference_schema() {
        let cache = RequestCache::new();
        cache.insert_preference_schema(5, vec![]);
        cache.invalidate_source(5);
        assert!(cache.get_preference_schema(5).is_none());
    }

    #[tokio::test]
    async fn concurrent_fetches_for_same_key_call_init_once() {
        let cache = Arc::new(RequestCache::new());
        let calls = Arc::new(AtomicU32::new(0));

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let cache = cache.clone();
                let calls = calls.clone();
                tokio::spawn(async move {
                    let c = calls.clone();
                    cache
                        .get_or_fetch_manga_details(1, "concurrent", async move {
                            c.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                            Ok::<_, std::io::Error>("value".to_string())
                        })
                        .await
                        .unwrap()
                })
            })
            .collect();

        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
