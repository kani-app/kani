// @ts-check
// REST API client. All functions return Promises and throw on non-2xx responses.
// A 401 response redirects to /login automatically.

/** @param {Response} res */
async function _parseBody(res) {
  const ct = res.headers.get('content-type') ?? '';
  if (ct.includes('application/json')) return res.json();
  if (res.status === 204 || res.headers.get('content-length') === '0') return null;
  return res.text();
}

/**
 * Pages where a 401 is expected and must not trigger a login redirect.
 */
const UNAUTHENTICATED_PAGES = [
  '/login',
  '/setup',
  '/register',
  '/forgot-password',
  '/reset-password',
  '/verify-email',
];

function _onUnauthenticatedPage() {
  return UNAUTHENTICATED_PAGES.includes(location.pathname);
}

/**
 * @param {string} method
 * @param {string} path
 * @param {{ body?: any, params?: Record<string, any>, signal?: AbortSignal, timeoutMs?: number }} [opts]
 */
const SOLVER_TEST_TIMEOUT_MS = 70000;

/**
 * Reads the double-submit CSRF cookie the server refreshes on every response.
 * Deliberately read per request: logging in rotates the session, and with it
 * the token.
 * @returns {string}
 */
function _csrfToken() {
  const match = document.cookie.split(';').find((c) => c.trim().startsWith('kani_csrf='));
  return match ? decodeURIComponent(match.trim().slice('kani_csrf='.length)) : '';
}

async function _req(method, path, opts = {}) {
  let url = `/rest${path}`;

  if (opts.params) {
    const qs = new URLSearchParams();
    for (const [k, v] of Object.entries(opts.params)) {
      if (v != null && v !== '') {
        qs.set(k, typeof v === 'object' ? JSON.stringify(v) : String(v));
      }
    }
    const s = qs.toString();
    if (s) url += '?' + s;
  }

  /** @type {RequestInit} */
  const init = { method, credentials: 'include', headers: {} };
  if (!['GET', 'HEAD', 'OPTIONS'].includes(method)) {
    const token = _csrfToken();
    // @ts-ignore: RequestInit.headers is a union but this instance is a plain object.
    if (token) init.headers['X-CSRF-Token'] = token;
  }
  if (opts.body != null) {
    // @ts-ignore: RequestInit.headers is a union but this instance is a plain object.
    init.headers['Content-Type'] = 'application/json';
    init.body = JSON.stringify(opts.body);
  }

  // Apply timeout via a local AbortController; if the caller also passed a signal,
  // chain it so either abort source cancels the request.
  let timer;
  if (opts.timeoutMs && opts.timeoutMs > 0) {
    const ctrl = new AbortController();
    timer = setTimeout(() => ctrl.abort(new DOMException('Request timed out', 'TimeoutError')), opts.timeoutMs);
    if (opts.signal) {
      if (opts.signal.aborted) ctrl.abort(opts.signal.reason);
      else opts.signal.addEventListener('abort', () => ctrl.abort(opts.signal?.reason), { once: true });
    }
    init.signal = ctrl.signal;
  } else if (opts.signal) {
    init.signal = opts.signal;
  }

  let res;
  try {
    res = await fetch(url, init);
  } finally {
    if (timer) clearTimeout(timer);
  }

  if (res.status === 401) {
    if (!_onUnauthenticatedPage()) window.location.href = '/login';
    throw Object.assign(new Error('Unauthorized'), { status: 401 });
  }

  if (!res.ok) {
    let body;
    try { body = await res.json(); } catch { body = { error: await res.text().catch(() => res.statusText) }; }
    throw Object.assign(new Error(body?.error || `HTTP ${res.status}`), {
      status: res.status,
      code: body?.code ?? null,
      hint: body?.hint ?? null,
      suggestions: body?.suggestions ?? null,
      traceId: res.headers.get('x-request-id'),
      body,
    });
  }

  return _parseBody(res);
}


/** @type {Map<string, { data: any, ts: number, revalidating: boolean }>} */
const _swrCache = new Map();

/**
 * Stale-while-revalidate fetch helper. Returns cached data immediately if fresh
 * enough, revalidates in the background when stale, or fetches synchronously on
 * a cold miss.
 * @template T
 * @param {string} key
 * @param {() => Promise<T>} fetcher
 * @param {{ ttlMs?: number }} [opts]
 * @returns {Promise<T>}
 */
export async function fetchSWR(key, fetcher, opts = {}) {
  const ttl = opts.ttlMs ?? 30_000;
  const entry = _swrCache.get(key);
  const now = Date.now();
  if (entry) {
    if (now - entry.ts <= ttl) return entry.data;
    if (!entry.revalidating) {
      entry.revalidating = true;
      fetcher().then(data => {
        _swrCache.set(key, { data, ts: Date.now(), revalidating: false });
      }).catch(() => {
        const e = _swrCache.get(key);
        if (e) e.revalidating = false;
      });
    }
    return entry.data;
  }
  const data = await fetcher();
  _swrCache.set(key, { data, ts: Date.now(), revalidating: false });
  return data;
}

/**
 * Drops cached entries so the next read goes to the server. Without a prefix it
 * clears everything, which is what sign-out needs; with one it drops a single
 * family, which is what a server-pushed invalidation needs.
 * @param {string} [prefix]
 */
export function clearSWRCache(prefix) {
  if (!prefix) {
    _swrCache.clear();
    return;
  }
  for (const key of _swrCache.keys()) {
    if (key.startsWith(prefix)) _swrCache.delete(key);
  }
}


/**
 * Returns the authenticated user's permission list as an array of strings.
 * @returns {Promise<string[]>}
 */
export async function getPermissions() {
  return _req('GET', '/auth/permissions');
}

export async function getCurrentUser() {
  return _req('GET', '/auth/current_user');
}

/** @param {string} username @param {string} password */
export async function login(username, password) {
  return _req('POST', '/auth/login', { body: { username, password } });
}

export async function logout() {
  clearSWRCache();
  // Dynamic import: session.js imports this module, so a static one would cycle.
  await import('./session.js').then((m) => m.clearRememberedPermissions()).catch(() => {});
  await _forgetOfflinePages();
  return _req('POST', '/auth/logout');
}

/** Cached chapter pages are account data, so sign-out must remove the shared browser cache. */
async function _forgetOfflinePages() {
  try {
    if ('caches' in window) await caches.delete('kani-pages-v1');
  } catch { }
}

/** @param {string} currentPassword @param {string} newPassword */
export async function changePassword(currentPassword, newPassword) {
  return _req('POST', '/auth/change_password', {
    body: { current_password: currentPassword, new_password: newPassword },
  });
}

export async function logoutEverywhere() {
  clearSWRCache();
  await import('./session.js').then((m) => m.clearRememberedPermissions()).catch(() => {});
  await _forgetOfflinePages();
  return _req('POST', '/auth/logout_everywhere');
}

export async function getPasswordResetEnabled() {
  return _req('GET', '/auth/password-reset-enabled');
}


export async function listApiTokens() {
  return _req('GET', '/me/api-tokens');
}

/** @param {string} name @param {number|null} [expiresInDays] */
export async function createApiToken(name, expiresInDays, kind = 'opds', scopes = []) {
  return _req('POST', '/me/api-tokens', {
    body: { name, expires_in_days: expiresInDays ?? null, kind, scopes },
  });
}

/** @param {string} id */
export async function revokeApiToken(id) {
  return _req('DELETE', `/me/api-tokens/${id}`);
}

export async function getRegistrationEnabled() {
  return _req('GET', '/auth/registration-enabled');
}

/** Default timeout (ms) for auth-flow requests so a hanging server doesn't strand the submit button. */
const AUTH_TIMEOUT_MS = 15_000;

/** @param {string} email */
export async function requestPasswordReset(email) {
  return _req('POST', '/auth/password-reset/request', { body: { email }, timeoutMs: AUTH_TIMEOUT_MS });
}

/** @param {string} token */
export async function validateResetToken(token) {
  return _req('GET', '/auth/password-reset/validate', { params: { token }, timeoutMs: AUTH_TIMEOUT_MS });
}

/** @param {string} token @param {string} newPassword */
export async function confirmPasswordReset(token, newPassword) {
  return _req('POST', '/auth/password-reset/confirm', { body: { token, new_password: newPassword }, timeoutMs: AUTH_TIMEOUT_MS });
}

/** @param {string} token */
export async function verifyEmail(token) {
  return _req('POST', '/auth/verify-email', { body: { token }, timeoutMs: AUTH_TIMEOUT_MS });
}

export async function resendVerification() {
  return _req('POST', '/auth/resend-verification', { timeoutMs: AUTH_TIMEOUT_MS });
}

/** @param {string} to */
export async function sendTestEmail(to) {
  return _req('POST', '/admin/email/test', { body: { to } });
}

/** @param {number} userId */
export async function adminTriggerPasswordReset(userId) {
  return _req('POST', `/admin/users/${userId}/password-reset`);
}


/** @returns {Promise<{ boot_id: string }>} */
export async function getBootId() {
  return _req('GET', '/boot_id');
}


export async function getSources() {
  return _req('GET', '/sources');
}

export async function getSourcesHealth() {
  return _req('GET', '/sources/health');
}

export async function getSystemUpdate() {
  return _req('GET', '/system/update');
}

export async function getDiagnostics() {
  return _req('GET', '/admin/diagnostics');
}

export async function getSourceCircuits() {
  return _req('GET', '/admin/sources/circuits');
}

export async function resetSourceCircuit(host) {
  return _req('POST', `/admin/sources/circuits/${encodeURIComponent(host)}/reset`);
}

export async function getProxyStats() {
  return _req('GET', '/admin/proxy/stats');
}

export async function downloadSupportBundle() {
  const res = await fetch('/rest/admin/support-bundle', {
    method: 'GET',
    credentials: 'include',
  });
  if (!res.ok) {
    throw Object.assign(new Error(`HTTP ${res.status}`), {
      status: res.status,
      traceId: res.headers.get('x-request-id'),
    });
  }
  const blob = await res.blob();
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  const disp = res.headers.get('Content-Disposition') ?? '';
  const match = disp.match(/filename="?([^"]+)"?/);
  a.download = match?.[1] ?? 'kani-support.zip';
  a.click();
  URL.revokeObjectURL(a.href);
}

/** @param {number} id */
export async function getSource(id) {
  return _req('GET', `/sources/${id}`);
}

/** @param {number} id */
export async function deleteSource(id) {
  return _req('DELETE', `/sources/${id}`);
}

/** @param {number} id */
export async function getUiThemes() {
  return _req('GET', '/ui/themes');
}

export async function saveUiTheme(body) {
  return _req('POST', '/ui/themes', { body });
}

export async function activateUiTheme(id) {
  return _req('PUT', `/ui/themes/${id}/activate`);
}

export async function deactivateUiTheme() {
  return _req('PUT', '/ui/themes/deactivate');
}

export async function deleteUiTheme(id) {
  return _req('DELETE', `/ui/themes/${id}`);
}

export async function reloadSource(id) {
  return _req('POST', `/sources/${id}/reload`);
}

/**
 * Fetch and install a WASM extension from a URL (find-or-create by its id).
 * @param {string} url
 */
export async function fetchWasm(url) {
  return _req('POST', '/sources/wasm/fetch', { body: { url } });
}

/** Install an interpreted-YAML extension from raw YAML text (find-or-create by id). @param {string} content */
export async function installYaml(content) {
  return _req('POST', '/sources/yaml', { body: { content } });
}

/** Fetch and install an interpreted-YAML extension from a URL. @param {string} url */
export async function fetchYaml(url) {
  return _req('POST', '/sources/yaml/fetch', { body: { url } });
}

/**
 * Upload a .wasm file to install an extension.
 * @param {File} file
 */
export async function uploadWasm(file) {
  const body = new FormData();
  body.append('file', file);
  const res = await fetch('/rest/sources/wasm', {
    method: 'POST',
    credentials: 'include',
    headers: { 'X-CSRF-Token': _csrfToken() },
    body,
  });
  if (res.status === 401) {
    if (!_onUnauthenticatedPage()) window.location.href = '/login';
    throw Object.assign(new Error('Unauthorized'), { status: 401 });
  }
  if (!res.ok) {
    let body;
    try { body = await res.json(); } catch { body = { error: await res.text().catch(() => res.statusText) }; }
    throw Object.assign(new Error(body?.error || `HTTP ${res.status}`), {
      status: res.status,
      code: body?.code ?? null,
      hint: body?.hint ?? null,
      suggestions: body?.suggestions ?? null,
      traceId: res.headers.get('x-request-id'),
      body,
    });
  }
  return null;
}

/** @param {number} sid @param {number} page @param {number} size @param {string} [filters] @param {AbortSignal} [signal] */
export async function getPopularManga(sid, page, size, filters, signal) {
  const params = filters ? { filters } : undefined;
  return _req('GET', `/sources/${sid}/popular/${page}/${size}`, { params, signal });
}

/** @param {number} sid @param {string} query @param {number} page @param {number} size @param {string} [filters] @param {AbortSignal} [signal] */
export async function searchManga(sid, query, page, size, filters, signal) {
  const params = filters ? { query, filters } : { query };
  return _req('GET', `/sources/${sid}/search/${page}/${size}`, { params, signal });
}

/** @param {number} sid @param {AbortSignal} [signal] */
export async function getSourceFilters(sid, signal) {
  return _req('GET', `/sources/${sid}/filters`, { signal });
}

/** @param {number} sid @param {string} mangaId */
export async function getRemoteMangaDetails(sid, mangaId, signal) {
  return _req('GET', `/sources/${sid}/details/${encodeURIComponent(mangaId)}`, { signal });
}

/** @param {number} sid @param {string} mangaId */
export async function getSourceMangaUrl(sid, mangaId) {
  return _req('GET', `/sources/${sid}/url/${encodeURIComponent(mangaId)}`);
}

/** @param {number} sid @param {string} mangaId */
/** @param {number|string} sid @param {string} mangaId @param {boolean} [force] */
export async function saveToLibrary(sid, mangaId, force = false) {
  const qs = force ? '?force=true' : '';
  return _req('POST', `/sources/${sid}/save/${encodeURIComponent(mangaId)}${qs}`);
}

/** @param {number} sid @param {string} mangaId @param {number} page @param {number} size @param {AbortSignal|undefined} signal @param {string|null} [sort] */
export async function getRemoteChapters(sid, mangaId, page, size, signal, sort) {
  const qs = sort ? `?sort=${encodeURIComponent(sort)}` : '';
  return _req('GET', `/sources/${sid}/chapters/${encodeURIComponent(mangaId)}/${page}/${size}${qs}`, { signal });
}

/** @param {number} sid @param {string} mangaId */
export async function getRemoteChapterSorts(sid, mangaId) {
  return _req('GET', `/sources/${sid}/chapter-sorts/${encodeURIComponent(mangaId)}`);
}

/**
 * Returns the page manifest for a locally downloaded chapter.
 * @param {number} chapterId
 * @returns {Promise<{
 *   chapter_id: number,
 *   chapter_title: string,
 *   manga_id: number,
 *   manga_title: string,
 *   page_count: number,
 *   pages: Array<{
 *     index: number,
 *     filename: string,
 *     double_page: boolean,
 *   }>,
 *   prev_chapter_id: number | null,
 *   next_chapter_id: number | null,
 *   last_page_read: number | null,
 *   spread_analysed: boolean,
 * }>}
 */
export async function getChapterPages(chapterId) {
  return _req('GET', `/chapter/${chapterId}/pages`);
}

/**
 * Returns the URL for a single page image from a downloaded chapter.
 * Use directly as an <img src> value — auth cookies are sent automatically.
 * @param {number} chapterId
 * @param {number} pageNum
 * @returns {string}
 */
export function getChapterPageUrl(chapterId, pageNum) {
  return `/rest/chapter/${chapterId}/page/${pageNum}`;
}

/** @param {number} sid @param {string} mangaId @returns {Promise<{ db_id: number | null }>} */
export async function checkInLibrary(sid, mangaId) {
  return _req('GET', `/sources/${sid}/in_library/${encodeURIComponent(mangaId)}`);
}

/** @param {number} sid @param {boolean} enabled */
export async function toggleSourceEnabled(sid, enabled) {
  return _req('PATCH', `/sources/${sid}/toggle_enabled`, { body: { enabled } });
}

/** @param {number} sid @param {boolean} favourited */
export async function toggleSourceFavourite(sid, favourited) {
  return _req('PATCH', `/sources/${sid}/toggle_favourite`, { body: { favourited } });
}

/** @param {number} sid @param {number|null} value */
export async function setSourceDownloadConcurrency(sid, value) {
  return _req('PUT', `/sources/${sid}/download-concurrency`, { body: { value } });
}

/** @param {number} sid @param {boolean} enabled */
export async function setSourceBrowserEnabled(sid, enabled) {
  return _req('PUT', `/sources/${sid}/browser-enabled`, { body: { enabled } });
}

/** @returns {Promise<number[]>} */
export async function getActiveSourceIds() {
  return _req('GET', '/sources/active_ids');
}


export async function listRepos() {
  return _req('GET', '/sources/repos');
}

/**
 * Add a repository. Returns the repo on success or throws with status 428
 * (TOFU confirmation required) or 409 (key changed).
 * @param {string} url
 * @param {string} [confirmFingerprint]
 */
export async function addRepo(url, confirmFingerprint) {
  return _req('POST', '/sources/repos', {
    body: confirmFingerprint
      ? { url, confirm_fingerprint: confirmFingerprint }
      : { url },
  });
}

/** @param {number} id */
export async function refreshRepo(id) {
  return _req('POST', `/sources/repos/${id}/refresh`);
}

/** @param {number} id */
export async function removeRepo(id) {
  return _req('DELETE', `/sources/repos/${id}`);
}

/** @param {number} id */
export async function listRepoExtensions(id) {
  return _req('GET', `/sources/repos/${id}/extensions`);
}

/** @param {number} repoId @param {string} extensionId */
export async function installFromRepo(repoId, extensionId) {
  return _req('POST', '/sources/install', { body: { repo_id: repoId, extension_id: extensionId } });
}

/**
 * @param {number} repoId
 * @param {string} extensionId
 * @param {number} sourceId
 */
export async function updateFromRepo(repoId, extensionId, sourceId) {
  return _req('POST', `/sources/${sourceId}/update`, { body: { repo_id: repoId, extension_id: extensionId } });
}


/** @param {number} sid */
export async function getPreferenceSchema(sid) {
  return _req('GET', `/sources/${sid}/preference_schema`);
}

/** @param {number} sid */
export async function getPreferences(sid) {
  return _req('GET', `/sources/${sid}/preferences`);
}

/** @param {number} sid @param {string} key @param {string} value */
export async function setPreference(sid, key, value) {
  return _req('PUT', `/sources/${sid}/preferences/${encodeURIComponent(key)}`, { body: { value } });
}

/** @param {number} sid @param {string} key @param {string} item */
export async function appendPreferenceItem(sid, key, item) {
  return _req('POST', `/sources/${sid}/preferences/${encodeURIComponent(key)}/append`, { body: { item } });
}

/** @param {number} sid @param {string} key @param {string} item */
export async function removePreferenceItem(sid, key, item) {
  return _req('POST', `/sources/${sid}/preferences/${encodeURIComponent(key)}/remove_item`, { body: { item } });
}

/** @param {number} sid @param {string} key @param {string} item @param {boolean} selected */
export async function togglePreferenceSelect(sid, key, item, selected) {
  return _req('POST', `/sources/${sid}/preferences/${encodeURIComponent(key)}/toggle_select`, { body: { item, selected } });
}


/**
 * @param {{ page: number, page_size: number, search?: string, status_filter?: number|null,
 *           tag_filter?: number|null, author_filter?: number|null, artist_filter?: number|null,
 *           category_filter?: number|null, sort_by?: string }} params
 * @param {AbortSignal} [signal]
 */
export async function getLibrary(params, signal) {
  const key = 'library:' + JSON.stringify(params);
  return fetchSWR(key, () => _req('GET', '/library', { params, signal }));
}

/** @param {number} page @param {AbortSignal} [signal] */
export async function getRecentUpdates(page, signal) {
  return _req('GET', '/recent_updates', { params: { page }, signal });
}

/**
 * @param {string} query
 * @param {"FavouritedOnly"|"AllEnabled"|{Sources: number[]}} scope
 * @param {number} page
 * @param {number} pageSize
 * @param {AbortSignal} [signal]
 */
export async function globalSearch(query, scope, page, pageSize, signal) {
  const scopeParam = typeof scope === 'string' ? scope : JSON.stringify(scope);
  return _req('GET', '/global_search', {
    params: { query, scope: scopeParam, page, page_size: pageSize },
    signal,
  });
}


/** @param {number} id */
export async function deleteManga(id) {
  return _req('DELETE', `/manga/${id}`);
}

/**
 * Returns a URL string suitable for use as an img src — not a fetch.
 * @param {number} id
 * @returns {string}
 */
export function getMangaCoverUrl(id, size, hash) {
  let url = `/rest/manga/${id}/cover`;
  const params = [];
  if (size) params.push(`size=${encodeURIComponent(size)}`);
  if (hash) params.push(`h=${encodeURIComponent(hash.slice(0, 16))}`);
  if (params.length) url += '?' + params.join('&');
  return url;
}

/** @param {number} id @param {AbortSignal} [signal] */
export async function getMangaDetails(id, signal) {
  return _req('GET', `/manga/${id}/details`, { signal });
}

/** @param {number} id @param {number} page @param {number} pageSize @param {string} sortOrder @param {AbortSignal} [signal] */
/**
 * @param {number} id
 * @param {number} page
 * @param {number} pageSize
 * @param {string} sortOrder
 * @param {AbortSignal | undefined} signal
 * @param {{ filterDownloaded?: boolean|null, filterUnread?: boolean|null, filterScanlator?: string|null, filterOrphaned?: boolean|null }} [filters]
 */
export async function getLocalChapters(id, page, pageSize, sortOrder, signal, filters = {}) {
  const { filterDownloaded, filterUnread, filterScanlator, filterOrphaned } = filters;
  return _req('GET', `/manga/${id}/chapters`, {
    params: {
      page,
      page_size: pageSize,
      sort_order: sortOrder,
      ...(filterDownloaded != null && { filter_downloaded: filterDownloaded }),
      ...(filterUnread != null && { filter_unread: filterUnread }),
      ...(filterOrphaned != null && { filter_orphaned: filterOrphaned }),
      ...(filterScanlator != null && { filter_scanlator: filterScanlator }),
      ...(filterOrphaned != null && { filter_orphaned: filterOrphaned }),
    },
    signal,
  });
}

/**
 * Returns all chapter IDs matching the given filters (no pagination).
 * @param {number} id
 * @param {{ filterDownloaded?: boolean|null, filterUnread?: boolean|null, filterScanlator?: string|null, preferredOnly?: boolean, sortOrder?: string, filterOrphaned?: boolean|null }} [opts]
 * @returns {Promise<{ ids: number[] }>}
 */
export async function getChapterIds(id, opts = {}) {
  const { filterDownloaded, filterUnread, filterScanlator, preferredOnly, sortOrder, filterOrphaned } = opts;
  return _req('GET', `/manga/${id}/chapter_ids`, {
    params: {
      ...(filterDownloaded != null && { filter_downloaded: filterDownloaded }),
      ...(filterUnread != null && { filter_unread: filterUnread }),
      ...(filterOrphaned != null && { filter_orphaned: filterOrphaned }),
      ...(filterScanlator != null && { filter_scanlator: filterScanlator }),
      ...(preferredOnly && { preferred_only: true }),
      ...(sortOrder && { sort_order: sortOrder }),
    },
  });
}

/** @param {number} id @returns {Promise<{ job_id: string }>} */
export async function downloadAll(id) {
  return _req('POST', `/manga/${id}/download_all`);
}

/** @param {number} id */
export async function cancelAllDownloads(id) {
  return _req('POST', `/manga/${id}/cancel_all`);
}

/** @param {number} id */
export async function refreshManga(id, opts) {
  return _req('POST', `/manga/${id}/refresh`, opts ? { body: opts } : undefined);
}

/** @param {number} id @returns {Promise<{ job_id: string }>} */
export async function relinkManga(id) {
  return _req('POST', `/manga/${id}/relink`);
}

/** @param {number} id @returns {Promise<{ job_id: string }>} */
export async function scanManga(id) {
  return _req('POST', `/manga/${id}/scan`);
}

/** @param {number} id */
export async function dismissSuppressedChapters(id) {
  return _req('POST', `/manga/${id}/dismiss-suppressed`);
}

/** @returns {Promise<{ queued: number }>} */
export async function scanAllLibrary() {
  return _req('POST', '/library/scan-all');
}

/**
 * Unified scan: scan all library manga or a specific list of IDs.
 * Emits SSE Started/MangaRefreshed/Completed events identical to scan-all.
 * @param {number[] | 'all'} idsOrAll
 */
export async function scanMangaMultiple(idsOrAll) {
  return _req('POST', '/manga/scan', { body: { ids: idsOrAll } });
}

/** @param {number} id @param {boolean} enabled */
export async function toggleAutoDownload(id, enabled) {
  return _req('POST', `/manga/${id}/toggle_auto_download`, { body: { enabled } });
}

/** @param {number} id @param {boolean} enabled */
export async function toggleAutoScan(id, enabled) {
  return _req('POST', `/manga/${id}/toggle_auto_scan`, { body: { enabled } });
}

/** @param {number} id @param {string} notes */
export async function updateMangaNotes(id, notes) {
  return _req('PATCH', `/manga/${id}/notes`, { body: { notes } });
}

/**
 * @param {number} id
 * @param {{ local_name?: string|null, local_description?: string|null,
 *           local_status?: number|null, authors?: string[]|null,
 *           artists?: string[]|null, tags?: string[]|null }} data
 */
export async function updateLocalMetadata(id, data) {
  return _req('PATCH', `/manga/${id}/local_metadata`, { body: data });
}

/** @param {number} id @param {File} file */
export async function uploadMangaCover(id, file) {
  const body = new FormData();
  body.append('file', file);
  const res = await fetch(`/rest/manga/${id}/cover`, { method: 'POST', credentials: 'include', headers: { 'X-CSRF-Token': _csrfToken() }, body });
  if (res.status === 401) {
    if (!_onUnauthenticatedPage()) window.location.href = '/login';
    throw Object.assign(new Error('Unauthorized'), { status: 401 });
  }
  if (!res.ok) {
    let b;
    try { b = await res.json(); } catch { b = { error: res.statusText }; }
    throw Object.assign(new Error(b?.error || `HTTP ${res.status}`), { status: res.status });
  }
  return res.status === 204 ? null : res.json().catch(() => null);
}

/** @param {number} id */
export async function clearMangaCoverOverride(id) {
  return _req('DELETE', `/manga/${id}/cover`);
}

/** @returns {Promise<Array<{id: number, name: string}>>} */
export async function getFilterAuthors() { return _req('GET', '/filters/authors'); }

/** @returns {Promise<Array<{id: number, name: string}>>} */
export async function getFilterArtists() { return _req('GET', '/filters/artists'); }

/** @returns {Promise<Array<{id: number, name: string}>>} */
export async function getFilterTags() { return _req('GET', '/filters/tags'); }

/** @param {number} id */
export async function markMangaSeen(id) {
  return _req('PATCH', `/manga/${id}/seen`);
}

/** @param {number} id @param {boolean} enabled */
export async function toggleDownloadAllPreferred(id, enabled) {
  return _req('POST', `/manga/${id}/toggle_download_all_preferred`, { body: { enabled } });
}

/** @param {number} id @param {number} targetSourceId @param {string} targetMangaId */
export async function previewMigration(id, targetSourceId, targetMangaId) {
  return _req('POST', `/manga/${id}/preview_migration`, {
    body: { target_source_id: targetSourceId, target_source_manga_id: targetMangaId },
  });
}

/** @param {number} id @param {number} targetSourceId @param {string} targetMangaId @param {boolean} keepOrphaned */
export async function migrateManga(id, targetSourceId, targetMangaId, keepOrphaned) {
  return _req('POST', `/manga/${id}/migrate`, {
    body: {
      target_source_id: targetSourceId,
      target_source_manga_id: targetMangaId,
      keep_orphaned_downloads: keepOrphaned,
    },
  });
}


/**
 * @param {number} targetSourceId
 * @param {{ manga_id: number, query?: string }[]} items
 * @param {AbortSignal} [signal]
 */
export async function matchMigrationTargets(targetSourceId, items, signal) {
  return _req('POST', '/migrations/match', { body: { target_source_id: targetSourceId, items }, signal });
}

/**
 * @param {number} targetSourceId
 * @param {{ manga_id: number, target_source_manga_id: string }[]} items
 * @param {boolean} keepOrphaned
 */
export async function submitBulkMigration(targetSourceId, items, keepOrphaned) {
  return _req('POST', '/migrations/bulk', {
    body: { target_source_id: targetSourceId, items, keep_orphaned_downloads: keepOrphaned },
  });
}

/** @param {string[]} jobIds */
export async function getMigrationStatuses(jobIds) {
  return _req('POST', '/migrations/status', { body: { job_ids: jobIds } });
}

/** @param {number} mangaId */
export async function getDownloadRules(mangaId) {
  return _req('GET', `/manga/${mangaId}/download_rules`);
}

/**
 * @param {number} mangaId
 * @param {{ LanguageInclude: string }|{ LanguageExclude: string }|
 *          { TitleContains: string }|{ TitleExcludes: string }|
 *          { ChapterNumberMin: number }|{ ChapterNumberMax: number }|
 *          'ExcludeFractional'|{ MaxAgeDays: number }|{ PublishedAfter: number }} kind
 */
export async function addDownloadRule(mangaId, kind) {
  return _req('POST', `/manga/${mangaId}/download_rules`, { body: { kind } });
}

/** @param {number} ruleId */
export async function deleteDownloadRule(ruleId) {
  return _req('DELETE', `/download_rules/${ruleId}`);
}

/** @param {number} ruleId @param {any} kind */
export async function updateDownloadRule(ruleId, kind) {
  return _req('PATCH', `/download_rules/${ruleId}`, { body: { kind } });
}

/**
 * @param {number} mangaId
 * @param {number[]} orderedIds
 */
export async function reorderDownloadRules(mangaId, orderedIds) {
  return _req('PUT', `/manga/${mangaId}/download_rules/order`, { body: { ordered_ids: orderedIds } });
}

/**
 * @param {number} mangaId
 * @param {any[]} kinds
 * @returns {Promise<{matching: number, total: number}>}
 */
export async function previewDownloadRules(mangaId, kinds) {
  return _req('POST', `/manga/${mangaId}/download_rules/preview`, { body: { kinds } });
}


/** @param {number} mangaId */
export async function getScanlatorPrefs(mangaId) {
  return _req('GET', `/manga/${mangaId}/scanlator_preferences`);
}

/** @param {number} mangaId @param {string} scanlator @param {number} priority @param {boolean} [blocked] */
export async function setScanlatorPref(mangaId, scanlator, priority, blocked = false) {
  return _req('POST', `/manga/${mangaId}/scanlator_preferences`, { body: { scanlator, priority, blocked } });
}

/** @param {number} id */
export async function deleteScanlatorPref(id) {
  return _req('DELETE', `/scanlator_preferences/${id}`);
}

/** @param {number} mangaId @param {'priority'|'whitelist'} mode */
export async function setScanlatorMode(mangaId, mode) {
  return _req('PATCH', `/manga/${mangaId}/scanlator_mode`, { body: { mode } });
}

/** @param {number} mangaId @returns {Promise<string[]>} */
export async function getChapterScanlators(mangaId) {
  return _req('GET', `/manga/${mangaId}/scanlators`);
}

/** @param {number} mangaId @returns {Promise<string[]>} */
export async function getChapterLanguages(mangaId) {
  return _req('GET', `/manga/${mangaId}/languages`);
}


/** @param {number} id */
export async function downloadChapter(id) {
  return _req('POST', `/chapter/${id}/download`);
}

/** @param {number} id */
export async function deleteChapter(id) {
  return _req('DELETE', `/chapter/${id}/delete`);
}

/** @param {number} id */
export async function cancelDownload(id) {
  return _req('POST', `/chapter/${id}/cancel`);
}


/** @param {number} chapterId @param {number} page */
export async function setChapterProgress(chapterId, page) {
  return _req('PUT', `/chapter/${chapterId}/progress`, { body: { page } });
}

/** @param {number[]} chapterIds @param {boolean} isRead */
export async function setChapterReadStatus(chapterIds, isRead) {
  return _req('PUT', '/chapters/read_status', { body: { chapter_ids: chapterIds, is_read: isRead } });
}

/** @param {number} mangaId */
export async function getMangaTracking(mangaId) {
  return _req('GET', `/manga/${mangaId}/tracking`);
}

/**
 * @param {number} mangaId
 * @param {{ status?: string, score?: number }} data
 */
export async function setMangaTracking(mangaId, data) {
  return _req('PUT', `/manga/${mangaId}/tracking`, { body: data });
}


export async function getTags() {
  return _req('GET', '/filters/tags');
}

export async function getAuthors() {
  return _req('GET', '/filters/authors');
}

export async function getArtists() {
  return _req('GET', '/filters/artists');
}


export async function getCategories() {
  return _req('GET', '/categories');
}

/** @param {string} name @param {number} sortOrder */
export async function createCategory(name, sortOrder) {
  return _req('POST', '/categories', { body: { name, sort_order: sortOrder } });
}

/** @param {number[]} orderedIds */
export async function reorderCategories(orderedIds) {
  return _req('PUT', '/categories/reorder', { body: { ordered_ids: orderedIds } });
}

/** @param {number} id @param {string} name */
export async function renameCategory(id, name) {
  return _req('PATCH', `/categories/${id}`, { body: { name } });
}

/** @param {number} id */
export async function deleteCategory(id) {
  return _req('DELETE', `/categories/${id}`);
}

/** @param {number} mangaId */
export async function getMangaCategories(mangaId) {
  return _req('GET', `/manga/${mangaId}/categories`);
}

/** @param {number} mangaId @param {number[]} categoryIds */
export async function setMangaCategories(mangaId, categoryIds) {
  return _req('PUT', `/manga/${mangaId}/categories`, { body: { category_ids: categoryIds } });
}


export async function getSettings() {
  return _req('GET', '/settings');
}

/**
 * @param {{ Download: object } | { Scan: object } | { Advanced: object }} payload
 */
export async function updateSettings(payload) {
  return _req('PATCH', '/settings', { body: payload });
}

/**
 * Probes a solver URL before it is saved, so an admin can check a change first.
 * @param {string} url
 */
export async function testSolver(url) {
  return _req('POST', '/settings/solver/test', { body: { url }, timeoutMs: SOLVER_TEST_TIMEOUT_MS });
}

export async function startRefreshAll() {
  return _req('POST', '/refresh/start');
}

export async function serverStop() {
  return _req('POST', '/server/stop');
}

export async function serverRestart() {
  return _req('POST', '/server/restart');
}

export async function runMaintenance() {
  return _req('POST', '/admin/maintenance');
}

export async function clearCache() {
  return _req('POST', '/admin/cache/clear');
}

export async function getCredentialEncryptionStatus() {
  return _req('GET', '/admin/credentials/status');
}

export async function migrateCredentialsToEncrypted() {
  return _req('POST', '/admin/credentials/encrypt');
}

export async function stopScan() {
  return _req('POST', '/admin/scan/stop');
}

export async function cancelAllGlobalDownloads() {
  return _req('DELETE', '/downloads/active');
}


export async function getTrackers() {
  return _req('GET', '/trackers');
}

/** @param {number} trackerId @param {string} redirectUri */
export async function getTrackerAuthUrl(trackerId, redirectUri) {
  return _req('GET', `/trackers/${trackerId}/auth_url`, { params: { redirect_uri: redirectUri } });
}

/** @param {number} trackerId */
export async function getTrackerConfig(trackerId) {
  return _req('GET', `/trackers/${trackerId}/config`);
}

/**
 * @param {number} trackerId
 * @param {{ client_id: string, client_secret?: string }} config
 */
export async function setTrackerConfig(trackerId, config) {
  return _req('PUT', `/trackers/${trackerId}/config`, { body: config });
}

/** @param {number} trackerId */
export async function deleteTrackerConfig(trackerId) {
  return _req('DELETE', `/trackers/${trackerId}/config`);
}

/** @param {number} trackerId */
export async function unlinkTracker(trackerId) {
  return _req('POST', `/trackers/${trackerId}/unlink`);
}

/** @param {number} trackerId @param {string} query */
export async function searchTrackerManga(trackerId, query) {
  return _req('GET', `/trackers/${trackerId}/search`, { params: { query } });
}

/** @param {number} mangaId */
export async function getTrackerMappings(mangaId) {
  return _req('GET', `/manga/${mangaId}/tracker_mappings`);
}

/** @param {number} mangaId @param {number} trackerId @param {string} trackerMangaId */
export async function setTrackerMapping(mangaId, trackerId, trackerMangaId) {
  return _req('PUT', `/manga/${mangaId}/tracker_mappings`, {
    body: { tracker_id: trackerId, tracker_manga_id: trackerMangaId },
  });
}

/** @param {number} mangaId @param {number} trackerId */
export async function deleteTrackerMapping(mangaId, trackerId) {
  return _req('DELETE', `/manga/${mangaId}/tracker_mappings/${trackerId}`);
}

export async function syncAllTrackers() {
  return _req('POST', '/trackers/sync');
}

/** @param {number} mangaId */
export async function syncMangaTrackers(mangaId) {
  return _req('POST', `/manga/${mangaId}/sync`);
}


/** @param {number} mangaId */
export async function getContinueReading(mangaId) {
  return _req('GET', `/manga/${mangaId}/continue_reading`);
}

/** @param {number} [limit] */
export async function getContinueReadingShelf(limit = 12) {
  return _req('GET', '/library/continue_reading', { params: { limit } });
}

/**
 * @param {number} mangaId
 * @param {number} chapterNumber
 * @param {boolean} isRead
 */
export async function markChaptersUpTo(mangaId, chapterNumber, isRead) {
  return _req('POST', `/manga/${mangaId}/chapters/mark_up_to`, {
    body: { chapter_number: chapterNumber, is_read: isRead },
  });
}


export async function adminListUsers() {
  return _req('GET', '/admin/users');
}

/**
 * @param {{ username: string, email: string, password: string, roles?: string[] }} body
 */
export async function adminCreateUser(body) {
  return _req('POST', '/admin/users', { body });
}

/**
 * @param {number} userId
 * @param {{ username?: string, email?: string, is_active?: boolean, password?: string }} body
 */
export async function adminUpdateUser(userId, body) {
  return _req('PATCH', `/admin/users/${userId}`, { body });
}

/** @param {number} userId */
export async function adminDeleteUser(userId) {
  return _req('DELETE', `/admin/users/${userId}`);
}

/**
 * @param {number} userId
 * @param {string} roleSlug
 */
export async function adminGrantRole(userId, roleSlug) {
  return _req('POST', `/admin/users/${userId}/roles`, { body: { role_slug: roleSlug } });
}

/**
 * @param {number} userId
 * @param {string} roleSlug
 */
export async function adminRevokeRole(userId, roleSlug) {
  return _req('DELETE', `/admin/users/${userId}/roles/${roleSlug}`);
}

export async function adminListRoles() {
  return _req('GET', '/admin/roles');
}

/**
 * @param {{ slug: string, parent?: string, description?: string, permissions?: string[] }} body
 */
export async function adminCreateRole(body) {
  return _req('POST', '/admin/roles', { body });
}

/**
 * @param {string} slug
 * @param {{ description?: string, permissions?: string[] }} body
 */
export async function adminUpdateRole(slug, body) {
  return _req('PATCH', `/admin/roles/${slug}`, { body });
}

/** @param {string} slug */
export async function adminDeleteRole(slug) {
  return _req('DELETE', `/admin/roles/${slug}`);
}

/**
 * @param {number} userId
 * @param {{ before?: string, limit?: number }} [opts]
 */
export async function getUserActivity(userId, opts = {}) {
  const params = {};
  if (opts.before) params.before = opts.before;
  if (opts.limit)  params.limit  = opts.limit;
  return _req('GET', `/admin/users/${userId}/activity`, Object.keys(params).length ? { params } : {});
}

/** @param {number} [limit] */
export async function getDownloadHistory(limit) {
  return _req('GET', '/downloads/history', limit ? { params: { limit } } : {});
}


/**
 * @param {{ level?: string, source?: string, from?: string, to?: string,
 *           search?: string, page?: number, page_size?: number }} [params]
 */
export async function getAdminLogs(params = {}) {
  return _req('GET', '/admin/logs', { params });
}

/**
 * @param {{ user_id?: number, action?: string, from?: string, to?: string,
 *           search?: string, page?: number, page_size?: number }} [params]
 */
export async function getAdminAuditLog(params = {}) {
  return _req('GET', '/admin/audit-log', { params });
}


/** @param {number} [period] Number of days for the activity window (default 90). */
export async function getReadingStats(period) {
  return _req('GET', '/stats', period ? { params: { period } } : {});
}


/** Fetch as a File download (navigates browser). */
export function downloadBackup(includeChapterProgress = false) {
  const qs = includeChapterProgress ? '?include_chapter_progress=true' : '';
  window.location.href = `/rest/library/backup${qs}`;
}

/** @param {File} file */
export async function previewTachiyomiImport(file) {
  const body = new FormData();
  body.append('file', file);
  const res = await fetch('/rest/library/import/tachiyomi/preview', { method: 'POST', credentials: 'include', headers: { 'X-CSRF-Token': _csrfToken() }, body });
  if (!res.ok) { let b; try { b = await res.json(); } catch { b = {}; } throw Object.assign(new Error(b?.error || `HTTP ${res.status}`), { status: res.status }); }
  return res.json();
}

/**
 * @param {File} file
 * @param {{ import_manga?: boolean, import_categories?: boolean,
 *            import_tracking?: boolean, import_chapter_progress?: boolean }} [opts]
 */
export async function importTachiyomiBackup(file, opts = {}) {
  const body = new FormData();
  body.append('file', file);
  for (const [k, v] of Object.entries(opts)) body.append(k, String(v));
  const res = await fetch('/rest/library/import/tachiyomi', { method: 'POST', credentials: 'include', headers: { 'X-CSRF-Token': _csrfToken() }, body });
  if (!res.ok) { let b; try { b = await res.json(); } catch { b = {}; } throw Object.assign(new Error(b?.error || `HTTP ${res.status}`), { status: res.status }); }
  return res.json();
}


/** @returns {Promise<{db_size_bytes:number, wal_size_bytes:number}>} */
export async function getDbStats() {
  return _req('GET', '/admin/db/stats');
}

export async function analyzeDb() {
  return _req('POST', '/admin/db/analyze');
}

export async function vacuumDb() {
  return _req('POST', '/admin/db/vacuum');
}

/** @param {string} kind */
export async function triggerRecurring(kind) {
  return _req('POST', `/admin/recurring/${encodeURIComponent(kind)}/run`);
}


export async function getPendingImports() {
  return _req('GET', '/library/pending-imports');
}

/** @param {number} id */
export async function deletePendingImport(id) {
  return _req('DELETE', `/library/pending-imports/${id}`);
}

/** @param {number} id @param {number} sourceId @param {string} sourceMangaId */
export async function resolvePendingImport(id, sourceId, sourceMangaId) {
  return _req('POST', `/library/pending-imports/${id}/resolve`, { body: { source_id: sourceId, source_manga_id: sourceMangaId } });
}


export async function getOrphanedManga() {
  return _req('GET', '/library/orphaned');
}


export async function getDuplicates() {
  return _req('GET', '/library/duplicates');
}

/** Trigger a full-library rescan and persist any new pairs found. */
export async function rescanDuplicates() {
  return _req('POST', '/library/duplicates/scan');
}

/** @param {number} aId @param {number} bId */
export async function dismissDuplicate(aId, bId) {
  return _req('POST', `/library/duplicates/${aId}/${bId}/dismiss`);
}

/** @param {number} keepId @param {number} discardId */
export async function mergeDuplicate(keepId, discardId) {
  return _req('POST', '/library/duplicates/merge', { body: { keep_id: keepId, discard_id: discardId } });
}


/** @param {string} path */
export async function fsBrowse(path) {
  return _req('GET', '/admin/fs/browse', { params: { path } });
}

/** @param {string} path @param {string} name */
export async function fsMkdir(path, name) {
  return _req('POST', '/admin/fs/mkdir', { body: { path, name } });
}


/** @param {'library_path'|'wasm_storage_path'} field @param {string} newPath */
export async function estimatePathMigration(field, newPath) {
  return _req('POST', '/admin/path/estimate', { body: { field, new_path: newPath } });
}

/** @param {'library_path'|'wasm_storage_path'} field @param {string} newPath */
export async function startPathMigration(field, newPath) {
  return _req('POST', '/admin/path/migrate', { body: { field, new_path: newPath } });
}


export async function listWebhooks() {
  return _req('GET', '/webhooks');
}

/** @param {{ url: string, secret?: string, events?: string }} body */
export async function createWebhook(body) {
  return _req('POST', '/webhooks', { body });
}

/** @param {number} id @param {{ url?: string, secret?: string, events?: string, enabled?: boolean }} body */
export async function updateWebhook(id, body) {
  return _req('PATCH', `/webhooks/${id}`, { body });
}

/** @param {number} id */
export async function deleteWebhook(id) {
  return _req('DELETE', `/webhooks/${id}`);
}

/** @param {number} id */
export async function testWebhook(id) {
  return _req('POST', `/webhooks/${id}/test`);
}

/** @param {number} id */
export async function listWebhookDeliveries(id) {
  return _req('GET', `/webhooks/${id}/deliveries`);
}

/** @param {number} mangaId */
export async function getMangaWebhookNotify(mangaId) {
  return _req('GET', `/manga/${mangaId}/webhook-notify`);
}

/** @param {number} mangaId @param {boolean} enabled */
export async function setMangaWebhookNotify(mangaId, enabled) {
  return _req('PUT', `/manga/${mangaId}/webhook-notify`, { body: { enabled } });
}


/** @param {number} chapterId @returns {Promise<number[]>} */
export async function getBookmarks(chapterId) {
  return _req('GET', `/chapter/${chapterId}/bookmarks`);
}

/** @param {number} chapterId @param {number} pageIndex @returns {Promise<{bookmarked:boolean}>} */
export async function toggleBookmark(chapterId, pageIndex) {
  return _req('POST', `/chapter/${chapterId}/bookmarks`, { body: { page_index: pageIndex } });
}


/**
 * Returns chapter notes for a manga: `{ notes: [{chapter_id, chapter_number, note}] }`.
 * The chapter ID set for indicator display can be derived from the result.
 * @param {number} mangaId
 * @returns {Promise<{notes: Array<{chapter_id:number,chapter_number:number,note:string}>}>}
 */
export async function getMangaChapterNotes(mangaId) {
  return _req('GET', `/manga/${mangaId}/chapter-notes`);
}


/** @param {number} chapterId @returns {Promise<{note:string|null}>} */
export async function getChapterNote(chapterId) {
  return _req('GET', `/chapter/${chapterId}/note`);
}

/** @param {number} chapterId @param {string} note */
export async function setChapterNote(chapterId, note) {
  return _req('PUT', `/chapter/${chapterId}/note`, { body: { note } });
}



/** @returns {Promise<{sessions: Array<{id:string,created_at:number,last_seen_at:number,user_agent:string|null,ip_addr:string|null,is_current:boolean}>}>} */
export async function getSessions() {
  return _req('GET', '/auth/sessions');
}

/** @param {string} sessionId */
export async function revokeSession(sessionId) {
  return _req('DELETE', `/auth/sessions/${sessionId}`);
}

export async function revokeOtherSessions() {
  return _req('DELETE', '/auth/sessions');
}


/** @returns {Promise<{secret:string,otpauth_uri:string,qr_data_url:string}>} */
export async function beginTotpSetup() {
  return _req('POST', '/auth/totp/setup');
}

/** @param {string} code @returns {Promise<{backup_codes:string[]}>} */
export async function verifyTotpSetup(code) {
  return _req('POST', '/auth/totp/verify', { body: { code } });
}

/** @param {string} code */
export async function disableTotp(code) {
  return _req('POST', '/auth/totp/disable', { body: { code } });
}

/** @param {string} code */
export async function stepUpTotp(code) {
  return _req('POST', '/auth/totp/step-up', { body: { code } });
}

/** @param {string} code @returns {Promise<{backup_codes:string[]}>} */
export async function regenerateBackupCodes(code) {
  return _req('POST', '/auth/totp/backup-codes', { body: { totp_code: code } });
}


/** @param {string} password @param {string} [identity] */
export async function checkPasswordStrength(password, identity = '') {
  return _req('POST', '/auth/password-strength', { body: { password, identity } });
}


/**
 * @returns {Promise<{version:string,first_run:boolean,oidc_available:boolean,registration_enabled:boolean}>}
 */
export async function getSystemInfo() {
  return _req('GET', '/system/info');
}

/** Recent changelog, rendered to sanitised HTML server-side. */
export async function getChangelog() {
  return _req('GET', '/system/changelog');
}

/** @returns {Promise<void>} */
export async function markFirstRunComplete() {
  return _req('POST', '/system/first-run-complete');
}


/** @returns {Promise<{public_instance:boolean,totp_enabled:boolean}>} */
export async function getFeatures() {
  return _req('GET', '/features');
}


export async function purgeAdminLogs() {
  return _req('POST', '/admin/logs/purge');
}


/** @param {number} sourceId @returns {Promise<{streaming_chapters:boolean}>} */
export async function getSourceCapabilities(sourceId) {
  return _req('GET', `/sources/${sourceId}/capabilities`);
}


/** @returns {Promise<Array<{id:string,name:string}>>} */
export async function listMetadataProviders() {
  return _req('GET', '/sources/metadata-providers');
}

/**
 * @param {number} mangaId
 * @param {string} provider
 * @returns {Promise<{fields_updated:string[]}>}
 */
export async function enrichMangaMetadata(mangaId, provider) {
  return _req('POST', `/manga/${mangaId}/enrich-metadata`, { body: { provider } });
}


/** @param {{ job_type?: string, status?: string, limit?: number, offset?: number }} [params] */
export async function getJobs(params) {
  return _req('GET', '/jobs', { params });
}

/** @param {string} id */
export async function getJob(id) {
  return _req('GET', `/jobs/${id}`);
}

/** @param {string} id */
export async function cancelJob(id) {
  return _req('DELETE', `/jobs/${id}`);
}

/** @param {string} id */
export async function pauseJob(id) {
  return _req('POST', `/jobs/${id}/pause`);
}

/** @param {string} id */
export async function resumeJob(id) {
  return _req('POST', `/jobs/${id}/resume`);
}

/** @param {number} id */
export async function retryChapterDownload(id) {
  return _req('POST', `/chapter/${id}/download/retry`);
}


export async function listTrash() {
  return _req('GET', '/trash');
}

/** @param {number} id */
export async function untrashManga(id) {
  return _req('POST', `/manga/${id}/untrash`);
}

/** @param {string} token */
export async function untrashMangaByToken(token) {
  return _req('POST', '/manga/untrash', { body: { token } });
}

export async function purgeTrashAll() {
  return _req('DELETE', '/trash');
}

/** @param {number} id */
export async function purgeTrashOne(id) {
  return _req('DELETE', `/trash/${id}`);
}


/** @param {number} mangaId */
export async function listVolumes(mangaId) {
  return _req('GET', `/manga/${mangaId}/volumes`);
}

/** @param {number} mangaId @param {{ name?: string, volume_num?: number }} body */
export async function createVolume(mangaId, body) {
  return _req('POST', `/manga/${mangaId}/volumes`, { body });
}

/** @param {number} mangaId @param {number} volumeId @param {{ name?: string, volume_num?: number }} body */
export async function updateVolume(mangaId, volumeId, body) {
  return _req('PUT', `/manga/${mangaId}/volumes/${volumeId}`, { body });
}

/** @param {number} mangaId @param {number} volumeId */
export async function deleteVolume(mangaId, volumeId) {
  return _req('DELETE', `/manga/${mangaId}/volumes/${volumeId}`);
}

/** @param {number} mangaId @param {number} chapterId @param {number|null} volumeId */
export async function assignChapterVolume(mangaId, chapterId, volumeId) {
  return _req('PUT', `/manga/${mangaId}/chapters/${chapterId}/volume`, { body: { volume_id: volumeId } });
}


export async function listCollections() {
  return _req('GET', '/collections');
}

/** @param {{ name: string, rule: object, sort_order?: number }} body */
export async function createCollection(body) {
  return _req('POST', '/collections', { body });
}

/** @param {number} id @param {{ name?: string, rule?: object, sort_order?: number }} body */
export async function updateCollection(id, body) {
  return _req('PUT', `/collections/${id}`, { body });
}

/** @param {number} id */
export async function deleteCollection(id) {
  return _req('DELETE', `/collections/${id}`);
}

/** @param {number} id */
export async function getCollectionManga(id) {
  return _req('GET', `/collections/${id}/manga`);
}


export async function listSavedSearches() {
  return _req('GET', '/saved-searches');
}

/** @param {{ name: string, query_json: string }} body */
export async function createSavedSearch(body) {
  return _req('POST', '/saved-searches', { body });
}

/** @param {number} id @param {{ name?: string, query_json?: string }} body */
export async function updateSavedSearch(id, body) {
  return _req('PUT', `/saved-searches/${id}`, { body });
}

/** @param {number} id */
export async function deleteSavedSearch(id) {
  return _req('DELETE', `/saved-searches/${id}`);
}


export async function getAdminStorageStats() {
  return _req('GET', '/admin/storage/stats');
}

export async function getAdminStorageStatsHistory() {
  return _req('GET', '/admin/storage/stats/history');
}


/** @returns {Promise<{muted:number[]}>} */
export async function getNotifyPrefs() {
  return _req('GET', '/me/notify-prefs');
}

export async function getAllUpgrades() {
  return _req('GET', '/me/upgrades');
}

/** @param {number|string} chapterId */
export async function applyChapterUpgrade(chapterId) {
  return _req('POST', `/chapters/${chapterId}/upgrade`);
}

/** @param {number|string} chapterId */
export async function dismissChapterUpgrade(chapterId) {
  return _req('POST', `/chapters/${chapterId}/upgrade/dismiss`);
}

/** @param {number|string} mangaId @param {boolean} enabled */
export async function setUpgradeAutoReplace(mangaId, enabled) {
  return _req('PUT', `/manga/${mangaId}/upgrade-auto-replace`, { body: { enabled } });
}


/** @param {{ manga_ids?: number[]|null, zip?: boolean, include_viewer?: boolean }} spec */
export async function exportArchive(spec) {
  return _req('POST', '/admin/library/archive', { body: spec });
}

/** @param {string} jobId */
export function archiveDownloadUrl(jobId) {
  return `/rest/admin/library/archive/${jobId}/download`;
}


export async function getGlobalScanlatorPrefs() {
  return _req('GET', '/scanlator_preferences/global');
}

/** @param {string} scanlator @param {number} priority @param {boolean} blocked */
export async function setGlobalScanlatorPref(scanlator, priority, blocked) {
  return _req('POST', '/scanlator_preferences/global', {
    body: { scanlator, priority, blocked },
  });
}

export async function getKnownScanlators() {
  return _req('GET', '/scanlator_preferences/known');
}


/**
 * @param {'quick'|'deep'} depth
 * @param {boolean} [fix]
 */
export async function runScrub(depth, fix = false) {
  return _req('POST', '/admin/library/scrub', { body: { depth, fix } });
}

export async function getLastScrub() {
  return _req('GET', '/admin/library/scrub/last');
}

/**
 * @param {string[]} paths
 * @param {boolean} dryRun
 */
export async function deleteOrphans(paths, dryRun) {
  return _req('POST', '/admin/library/orphans/delete', { body: { paths, dry_run: dryRun } });
}


export async function getBackupSchedule() {
  return _req('GET', '/admin/backup/schedule');
}

/** @param {object} config */
export async function setBackupSchedule(config) {
  return _req('PUT', '/admin/backup/schedule', { body: config });
}

export async function runBackupNow() {
  return _req('POST', '/admin/backup/run-now');
}

/**
 * Download a backup, optionally encrypted.
 * @param {boolean} includeChapterProgress
 * @param {string} [passphrase]
 */
export function downloadBackupEncrypted(includeChapterProgress = false, passphrase = '') {
  if (!passphrase) {
    downloadBackup(includeChapterProgress);
    return;
  }
  fetch(`/rest/library/backup${includeChapterProgress ? '?include_chapter_progress=true' : ''}`, {
    method: 'GET',
    credentials: 'include',
    headers: passphrase ? { 'X-Backup-Passphrase': passphrase } : {},
  }).then(async res => {
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const blob = await res.blob();
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    const disp = res.headers.get('Content-Disposition') ?? '';
    const match = disp.match(/filename="?([^"]+)"?/);
    a.download = match?.[1] ?? 'kani-backup.zip';
    a.click();
    URL.revokeObjectURL(a.href);
  }).catch(e => { throw e; });
}

/** @param {File} file @param {string} [passphrase] */
export async function previewBackupEncrypted(file, passphrase = '') {
  const body = new FormData();
  body.append('file', file);
  if (passphrase) body.append('passphrase', passphrase);
  const res = await fetch('/rest/library/backup/preview', { method: 'POST', credentials: 'include', headers: { 'X-CSRF-Token': _csrfToken() }, body });
  if (!res.ok) { let b; try { b = await res.json(); } catch { b = {}; } throw Object.assign(new Error(b?.error || `HTTP ${res.status}`), { status: res.status }); }
  return res.json();
}

/**
 * @param {File} file
 * @param {{ merge?: boolean, import_manga?: boolean, import_categories?: boolean,
 *            import_download_rules?: boolean, import_tracking?: boolean,
 *            import_chapter_progress?: boolean, import_settings?: boolean }} [opts]
 * @param {string} [passphrase]
 */
export async function restoreBackupEncrypted(file, opts = {}, passphrase = '') {
  const body = new FormData();
  body.append('file', file);
  for (const [k, v] of Object.entries(opts)) body.append(k, String(v));
  if (passphrase) body.append('passphrase', passphrase);
  const res = await fetch('/rest/library/restore', { method: 'POST', credentials: 'include', headers: { 'X-CSRF-Token': _csrfToken() }, body });
  if (!res.ok) { let b; try { b = await res.json(); } catch { b = {}; } throw Object.assign(new Error(b?.error || `HTTP ${res.status}`), { status: res.status }); }
  return res.json();
}
