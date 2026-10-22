// @ts-check
// Server-Sent Events client for real-time download/refresh/scan notifications.
// Manages reconnection with exponential backoff and detects server restarts.

import { getState as getSession, setState as setSession } from './session.js';
import { getState, setState, updateState } from './cache.js';
import { getState as getUiState } from './ui-state.js';
import { postToServiceWorker, cacheChapter } from './offline.js';
import { showToast } from './components/toast.js';
import { t } from './i18n.js';
import * as api from './api.js';

const SSE_URL = '/rest/events';
const MAX_DELAY_MS = 30_000;

/** @type {EventSource | null} */
let _source = null;
let _retryCount = 0;
/** @type {ReturnType<typeof setTimeout> | null} */
let _retryTimer = null;
let _everConnected = false;

/** Accumulates per-manga new chapter counts during a scan run. Reset on each 'started' event. */
let _scanNewChapters = /** @type {Map<number, number>} */ (new Map());

/**
 * Subscribe to lifecycle events for a single job id.
 * Auto-unsubscribes on the first terminal event (completed, failed, cancelled).
 * @param {string} jobId
 * @param {{ onProgress?: (e: any) => void, onComplete?: (e: any) => void, onFailed?: (e: any) => void, onCancelled?: (e: any) => void }} callbacks
 * @returns {() => void} cleanup function
 */
export function subscribeJob(jobId, { onProgress, onComplete, onFailed, onCancelled } = {}) {
  function handler(/** @type {CustomEvent} */ e) {
    const data = /** @type {any} */ (e).detail;
    if (data.job_id !== jobId) return;
    const type = data.type;
    if (type === 'job_progress') { onProgress?.(data); return; }
    if (type === 'job_completed') { onComplete?.(data); cleanup(); return; }
    if (type === 'job_failed')    { onFailed?.(data);   cleanup(); return; }
    if (type === 'job_cancelled') { onCancelled?.(data); cleanup(); return; }
  }
  function cleanup() { window.removeEventListener('kani:sse', handler); }
  window.addEventListener('kani:sse', handler);
  return cleanup;
}

/**
 * Opens the SSE connection to /rest/events.
 * Call once at app startup. Returns a `disconnect()` function.
 * @returns {{ disconnect: () => void }}
 */
export function connectSSE() {
  // Load the muted set before events start arriving, so the notification
  // preference holds from the first chapter of the session rather than only
  // for series the user has since opened.
  api.getNotifyPrefs()
    .then((res) => setState('mutedManga', new Set((res?.muted ?? []).map(Number))))
    .catch(() => {});
  _connect();
  return { disconnect: _disconnect };
}

function _connect() {
  if (_source) { _source.close(); _source = null; }

  _source = new EventSource(SSE_URL, { withCredentials: true });

  _source.addEventListener('open', () => {
    _retryCount = 0;
    _everConnected = true;
    window.dispatchEvent(new CustomEvent('kani:sse-connected'));
  });

  _source.addEventListener('message', (event) => {
    let data;
    try {
      data = JSON.parse(event.data);
    } catch (err) {
      console.warn('[SSE] Failed to parse event payload, dropping:', err, event.data);
      return;
    }
    _handleEvent(data);
    window.dispatchEvent(new CustomEvent('kani:sse', { detail: data }));
  });

  // The server sends a named 'close' event when the stream lags too much.
  _source.addEventListener('close', () => {
    _scheduleReconnect();
  });

  _source.addEventListener('error', () => {
    _source?.close();
    _source = null;
    _scheduleReconnect();
  });
}

function _disconnect() {
  if (_retryTimer) { clearTimeout(_retryTimer); _retryTimer = null; }
  if (_source) { _source.close(); _source = null; }
}

function _scheduleReconnect() {
  if (_retryTimer) return;
  if (_everConnected) window.dispatchEvent(new CustomEvent('kani:sse-disconnected'));
  const delay = Math.min(1000 * Math.pow(2, _retryCount), MAX_DELAY_MS);
  _retryCount++;
  _retryTimer = setTimeout(() => {
    _retryTimer = null;
    _connect();
  }, delay);
}

/**
 * Drops a chapter's progress entry. Entries otherwise live for the tab's
 * lifetime, so a later attempt on the same chapter would read the previous
 * attempt's terminal state as its own.
 * @param {number} chapterId
 */
export function clearChapterProgress(chapterId) {
  updateState('chaptersProgress', (map) => {
    if (!map.has(chapterId)) return map;
    const m = new Map(map);
    m.delete(chapterId);
    return m;
  });
}

/** @param {any} data */
/**
 * Applies `update` to a chapter's progress entry, creating one when the
 * `chapter_started` that would have seeded it never arrived — a download that
 * fails while fetching the page list emits only its terminal event.
 *
 * @param {unknown} chapterId
 * @param {string | undefined} chapterName
 * @param {(entry: any) => any} update
 */
function _updateChapterEntry(chapterId, chapterName, update) {
  updateState('chaptersProgress', (map) => {
    const m = new Map(map);
    const id = Number(chapterId);
    const entry = m.get(id) ?? {
      id,
      name: chapterName ?? '',
      mangaId: 0,
      mangaTitle: '',
      totalPages: 0,
      completedPages: 0,
      status: 'in_progress',
      jobId: null,
    };
    m.set(id, update(entry));
    return m;
  });
}

function _handleEvent(data) {
  const type = data.type;

  if (type === 'state_snapshot') {
    const prev = getSession('bootId');
    if (data.boot_id && prev && data.boot_id !== prev) {
      window.dispatchEvent(new CustomEvent('kani:server-restart'));
    }
    if (data.boot_id) setSession('bootId', data.boot_id);

    /** @type {Map<number, import('./cache.js').ChapterProgress>} */
    const map = new Map();
    for (const ch of (data.chapters ?? [])) {
      map.set(Number(ch.chapter_id), {
        id: Number(ch.chapter_id),
        name: ch.chapter_name ?? '',
        mangaId: Number(ch.manga_id ?? 0),
        mangaTitle: ch.manga_title ?? '',
        totalPages: ch.total_pages ?? 0,
        completedPages: ch.completed_pages ?? 0,
        status: _normaliseStatus(ch.status),
      });
    }
    setState('chaptersProgress', map);

    if (data.is_refreshing) {
      setState('refreshState', { type: 'running', completed: 0, total: 0 });
    }
    return;
  }

  if (type === 'chapter_started') {

    updateState('chaptersProgress', (map) => {
      const m = new Map(map);
      m.set(Number(data.chapter_id), {
        id: Number(data.chapter_id),
        name: data.chapter_name,
        mangaId: Number(data.manga_id ?? 0),
        mangaTitle: data.manga_title ?? '',
        totalPages: data.total_pages,
        completedPages: 0,
        status: 'in_progress',
        jobId: data.job_id ?? null,
      });
      return m;
    });
    return;
  }

  if (type === 'page_completed') {
    _updateChapterEntry(data.chapter_id, data.chapter_name, (entry) => {
      // Only advance — out-of-order events must not move the bar backward.
      const next = Number(data.page_index) + 1;
      return next > entry.completedPages ? { ...entry, completedPages: next } : entry;
    });
    return;
  }

  if (type === 'chapter_completed') {
    const pages = Number(data.successful_pages ?? 0);
    _updateChapterEntry(data.chapter_id, data.chapter_name, (entry) => ({
      ...entry,
      status: 'completed',
      totalPages: entry.totalPages || pages,
      completedPages: entry.totalPages || pages,
    }));
    _maybeAutoCache(Number(data.chapter_id), pages);
    return;
  }

  if (type === 'chapter_failed') {
    _updateChapterEntry(data.chapter_id, data.chapter_name, (entry) => ({ ...entry, status: 'failed' }));
    return;
  }

  if (type === 'chapter_cancelled') {
    _updateChapterEntry(data.chapter_id, data.chapter_name, (entry) => ({ ...entry, status: 'cancelled' }));
    return;
  }

  if (type === 'started') {
    setState('refreshState', { type: 'running', completed: 0, total: data.total });
    // Mark every manga that will be scanned as "pending" so covers show a spinner.
    setState('scanningMangaIds', new Set((data.manga_ids ?? []).map(Number)));
    _scanNewChapters = new Map();
    return;
  }

  if (type === 'manga_refreshed') {
    updateState('refreshState', (s) => {
      if (s.type !== 'running') return s;
      return { ...s, completed: data.completed, total: data.total };
    });
    updateState('scanningMangaIds', (s) => { const n = new Set(s); n.delete(Number(data.manga_id)); return n; });
    // Accumulate new chapter counts (non-zero only for scan operations).
    const nc = Number(data.new_chapters ?? 0);
    if (nc > 0) _scanNewChapters.set(Number(data.manga_id), nc);
    return;
  }

  if (type === 'completed') {
    const totalNew = [..._scanNewChapters.values()].reduce((a, b) => a + b, 0);
    setState('scanResult', {
      total: Number(data.total),
      failed: Number(data.failed),
      newChapters: totalNew,
      perManga: new Map(_scanNewChapters),
    });
    _scanNewChapters = new Map();
    setState('refreshState', { type: 'done', total: data.total, failed: data.failed });
    setState('scanningMangaIds', new Set());
    // Increment library invalidation so pages re-fetch
    api.clearSWRCache('library:');
    updateState('libraryInvalidation', (n) => n + 1);
    // Return to idle after 5s
    setTimeout(() => setState('refreshState', { type: 'idle' }), 5000);
    return;
  }

  if (type === 'library_invalidated') {
    // The subscriber refetches through getLibrary, which reads the SWR cache.
    // Leaving it populated serves the pre-invalidation page for the TTL.
    api.clearSWRCache('library:');
    updateState('libraryInvalidation', (n) => n + 1);
    return;
  }

  // Emitted since upgrade detection shipped and handled by nobody, so a scan
  // that found replaceable chapters left no trace until the user happened to
  // open the Upgrades page.
  if (type === 'upgrades_found') {
    updateState('upgradesPending', (n) => (Number(n) || 0) + (Number(data.count) || 0));
    return;
  }

  // The event carries the host and failure count and was broadcast to nothing,
  // so requests to a failing source simply started erroring with no
  // explanation of why or for how long.
  if (type === 'circuit_open') {
    showToast(
      t('sse.circuit_open', { host: data.host ?? '?', count: data.failure_count ?? 0 }),
      { type: 'error' },
    );
    return;
  }

  if (type === 'new_chapters') {
    if (localStorage.getItem('kani_disable_notifications') !== 'true') {
      const incomingNames = /** @type {string[]} */ (data.chapter_names ?? []);
      updateState('scanNotifications', (list) => {
        const existing = list.findIndex((/** @type {{ mangaId: number; }} */ n) => n.mangaId === Number(data.manga_id));
        if (existing >= 0) {
          const copy = [...list];
          copy[existing] = {
            ...copy[existing],
            count: copy[existing].count + data.count,
            chapterNames: [...(copy[existing].chapterNames ?? []), ...incomingNames],
          };
          return copy;
        }
        return [...list, {
          mangaId: Number(data.manga_id),
          mangaName: data.manga_name,
          count: data.count,
          chapterNames: incomingNames,
        }];
      });
    }

    // Browser push notifications
    const mangaId = Number(data.manga_id);
    // The muted set is loaded once per session, so this holds for every series
    // rather than only those whose detail page happened to be opened. The
    // per-page Map still wins when present, so a toggle applies immediately.
    const notifyPrefs = getUiState('mangaNotifyPrefs');
    const muted = getState('mutedManga');
    const notifyAllowed = notifyPrefs instanceof Map && notifyPrefs.has(mangaId)
      ? notifyPrefs.get(mangaId)
      : !(muted instanceof Set && muted.has(mangaId));
    if (
      notifyAllowed &&
      localStorage.getItem('kani_browser_notifications') === 'true' &&
      'Notification' in window &&
      Notification.permission === 'granted'
    ) {
      const count = data.count ?? 1;
      const body = count === 1
        ? (data.chapter_names?.[0] ?? 'New chapter available')
        : `${count} new chapters`;
      try {
        new Notification(data.manga_name ?? 'New chapters', { body, tag: `kani-manga-${mangaId}` });
      } catch { }
    }
  }
}

/**
 * Normalises a status string from the snapshot into our internal status enum.
 * @param {string} raw
 * @returns {'in_progress'|'completed'|'failed'|'cancelled'}
 */
function _normaliseStatus(raw) {
  if (raw === 'in_progress') return 'in_progress';
  if (raw === 'completed')   return 'completed';
  if (raw === 'failed')      return 'failed';
  return 'cancelled';
}

/**
 * Cache a just-downloaded chapter if offline auto-mode is active.
 * @param {number} chapterId
 * @param {number} pageCount
 */
function _maybeAutoCache(chapterId, pageCount) {
  if (localStorage.getItem('kani_offline_mode') !== 'auto') return;
  if (!navigator.serviceWorker?.controller) return;
  if (pageCount <= 0) return;
  cacheChapter(chapterId, pageCount);
}
