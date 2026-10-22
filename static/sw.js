// @ts-check
// Kani service worker — shell caching + page-image caching.

const SHELL_CACHE  = 'kani-shell-v5';
const PAGE_CACHE   = 'kani-pages-v1';
const KNOWN_CACHES = [SHELL_CACHE, PAGE_CACHE];

/** Cache key the app shell is stored under for offline navigation. */
const SHELL_FALLBACK = '/';

/** Last-resort page when even the shell was never cached. */
const OFFLINE_HTML = `<!doctype html><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Offline — Kani</title>
<style>
  body { margin:0; min-height:100dvh; display:grid; place-items:center;
         font-family: 'DM Sans', system-ui, sans-serif; background:#111113; color:#e9e7e4; }
  .box { text-align:center; padding:2rem; }
  h1 { font-size:1.15rem; margin:0 0 .35rem; }
  p { margin:0; color:#8f8d95; font-size:.85rem; }
</style>
<div class="box"><h1>You're offline</h1><p>Reconnect to load Kani.</p></div>`;

/**
 * Precached so the app boots offline. Precaching is not permission to serve
 * these from cache first — most are rebuilt on every frontend change while
 * keeping the same path, so see the fetch handler for what is actually
 * cache-first. Bump SHELL_CACHE whenever this list or that routing changes.
 */
const SHELL_URLS = [
  '/',
  '/css/main.css',
  '/js/app.js',
  '/js/dist/app.js',
  '/js/vendor/preact.module.js',
  '/js/vendor/preact-hooks.module.js',
  '/js/vendor/htm.module.js',
  '/manifest.webmanifest',
];


self.addEventListener('install', e => {
  e.waitUntil(
    caches.open(SHELL_CACHE).then(cache =>
      cache.addAll(SHELL_URLS).catch(() => {})
    )
  );
  self.skipWaiting();
});


self.addEventListener('activate', e => {
  e.waitUntil(
    caches.keys().then(keys =>
      Promise.all(
        keys
          .filter(k => !KNOWN_CACHES.includes(k))
          .map(k => caches.delete(k))
      )
    )
  );
  self.clients.claim();
});


self.addEventListener('fetch', e => {
  const { request } = e;
  const url = new URL(request.url);

  // Only handle same-origin GET requests.
  if (request.method !== 'GET' || url.origin !== self.location.origin) return;

  const path = url.pathname;

  // Navigations — network-first, falling back to the cached app shell. Without
  // this, going offline on any route (or reloading) yields a bare 503: the SPA
  // shell is what boots the router, so every route needs it as its fallback.
  if (request.mode === 'navigate') {
    e.respondWith(_navigate(request));
    return;
  }

  // Page images — cache-first, persistent.
  if (path.match(/^\/rest\/chapter\/\d+\/page\/\d+/)) {
    e.respondWith(_cacheFirst(PAGE_CACHE, request));
    return;
  }

  // Icons — cache-first. Immutable by construction: a change ships under a new
  // filename, so a cached copy can never be the wrong one.
  if (path.startsWith('/icons/')) {
    e.respondWith(_cacheFirst(SHELL_CACHE, request));
    return;
  }

  // Everything the app is built from — network-first, so a deploy is picked up
  // whole. Serving one of these from cache while its siblings update leaves the
  // entry module importing chunk hashes the server no longer has.
  if (path.startsWith('/css/') || path.startsWith('/js/') || path === '/manifest.webmanifest') {
    e.respondWith(_networkFirst(SHELL_CACHE, request));
    return;
  }

  // Everything else (API, auth, etc.) — network-first, no caching.
});


self.addEventListener('message', e => {
  if (!e.data) return;

  if (e.data.type === 'SKIP_WAITING') {
    self.skipWaiting();
    return;
  }

  if (e.data.type === 'CACHE_CHAPTER') {
    const { chapterId, pageCount, maxBytes } = e.data;
    e.waitUntil(_cacheChapter(chapterId, pageCount, maxBytes));
    return;
  }

  if (e.data.type === 'EVICT_CHAPTER') {
    const { chapterId } = e.data;
    e.waitUntil(_evictChapter(chapterId));
    return;
  }
});


/**
 * Navigations: try the network, fall back to the cached shell so the SPA can
 * still boot offline. `/rest/**` and SSE never reach here — they are not
 * navigations and fall through to the network untouched.
 */
async function _navigate(request) {
  const cache = await caches.open(SHELL_CACHE);
  try {
    const response = await fetch(request);
    // Keep the shell fresh, but only from a real page response.
    if (response.ok && response.headers.get('Content-Type')?.includes('text/html')) {
      cache.put(SHELL_FALLBACK, response.clone());
    }
    return response;
  } catch {
    return (await cache.match(SHELL_FALLBACK))
        ?? (await cache.match('/'))
        ?? new Response(OFFLINE_HTML, {
             status: 200,
             headers: { 'Content-Type': 'text/html; charset=utf-8' },
           });
  }
}

/**
 * `cache: 'no-cache'` revalidates with the server rather than trusting the HTTP
 * cache. Releases once sent `immutable` for a year on these paths, so a plain
 * fetch would be answered from that entry and a stranded client could never
 * reach a newer build. The revalidation costs a 304.
 */
async function _networkFirst(cacheName, request) {
  const cache = await caches.open(cacheName);
  try {
    const response = await fetch(request, { cache: 'no-cache' });
    if (response.ok) cache.put(request, response.clone());
    return response;
  } catch {
    const cached = await cache.match(request);
    return cached ?? new Response('Offline', { status: 503 });
  }
}

async function _cacheFirst(cacheName, request) {
  const cache = await caches.open(cacheName);
  const cached = await cache.match(request);
  if (cached) return cached;
  try {
    const response = await fetch(request);
    if (response.ok) cache.put(request, response.clone());
    return response;
  } catch {
    return new Response('Offline', { status: 503 });
  }
}

async function _cacheChapter(chapterId, pageCount, maxBytes) {
  const cache = await caches.open(PAGE_CACHE);

  if (maxBytes) {
    const est = await navigator.storage?.estimate?.();
    if (est?.usage != null && est.usage >= maxBytes) return;
  }

  for (let i = 0; i < pageCount; i++) {
    const url = `/rest/chapter/${chapterId}/page/${i}`;
    if (await cache.match(url)) continue;
    try {
      const resp = await fetch(url);
      if (resp.ok) await cache.put(url, resp);
    } catch { }
  }

  const clients = await self.clients.matchAll();
  for (const client of clients) {
    client.postMessage({ type: 'CHAPTER_CACHED', chapterId });
  }
}

async function _evictChapter(chapterId) {
  const cache = await caches.open(PAGE_CACHE);
  const keys = await cache.keys();
  await Promise.all(
    keys
      .filter(r => r.url.includes(`/chapter/${chapterId}/page/`))
      .map(r => cache.delete(r))
  );
}
