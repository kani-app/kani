// @ts-check
// Offline / service-worker helpers shared across the app.

/**
 * Post a message to the active service worker, if one is registered.
 * @param {object} msg
 */
export function postToServiceWorker(msg) {
  navigator.serviceWorker?.controller?.postMessage(msg);
}

/**
 * Ask the service worker to pre-fetch and cache all pages for a chapter.
 * @param {number} chapterId
 * @param {number} pageCount
 */
export function cacheChapter(chapterId, pageCount) {
  const maxMb = Number(localStorage.getItem('kani_offline_max_mb') || '0') || null;
  const maxBytes = maxMb ? maxMb * 1024 * 1024 : null;
  postToServiceWorker({ type: 'CACHE_CHAPTER', chapterId, pageCount, maxBytes });
}

/**
 * Ask the service worker to evict all cached pages for a chapter.
 * @param {number} chapterId
 */
export function evictChapter(chapterId) {
  postToServiceWorker({ type: 'EVICT_CHAPTER', chapterId });
}

/**
 * Return the set of chapter IDs whose pages are currently in the page cache.
 * Resolves immediately to an empty set if the Cache Storage API is unavailable.
 * @returns {Promise<Set<number>>}
 */
export async function getCachedChapterIds() {
  if (!('caches' in window)) return new Set();
  try {
    const cache = await caches.open('kani-pages-v1');
    const keys = await cache.keys();
    const ids = new Set(/** @type {number[]} */ ([]));
    for (const req of keys) {
      const m = req.url.match(/\/chapter\/(\d+)\/page\//);
      if (m) ids.add(Number(m[1]));
    }
    return ids;
  } catch {
    return new Set();
  }
}

/**
 * Listen for CHAPTER_CACHED messages from the service worker.
 * @param {(chapterId: number) => void} cb
 * @returns {() => void} unsubscribe
 */
export function onChapterCached(cb) {
  if (!('serviceWorker' in navigator)) return () => {};
  const handler = (/** @type {MessageEvent} */ e) => {
    if (e.data?.type === 'CHAPTER_CACHED') cb(Number(e.data.chapterId));
  };
  navigator.serviceWorker.addEventListener('message', handler);
  return () => navigator.serviceWorker.removeEventListener('message', handler);
}
