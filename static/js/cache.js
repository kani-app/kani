// @ts-check
// Server-cache state atoms: SSE-populated, cross-tab broadcast where noted.
// Signal-backed via the shared store factory; `sigFor` is exported for
// reactive readers.

import { broadcastStateChange } from './sync.js';
import { createSignalStore } from './signal-store.js';

/**
 * @typedef {{ id: number, name: string, mangaId: number, mangaTitle: string,
 *             totalPages: number, completedPages: number,
 *             number?: number | null, downloadedAt?: string | null,
 *             jobId?: string | null,
 *             status: 'in_progress'|'completed'|'completed_hidden'|'failed'|'cancelled'|'deleted' }} ChapterProgress
 * @typedef {{ mangaId: number, mangaName: string, count: number, chapterNames: string[] }} ScanNotification
 * @typedef {{ type: 'idle' }
 *          | { type: 'running', completed: number, total: number }
 *          | { type: 'done',    total: number,     failed: number }} RefreshState
 */

const _BROADCAST_KEYS = new Set(['chaptersProgress', 'libraryInvalidation', 'sourcesInvalidation']);

const _store = createSignalStore({
  /** @type {Map<number, ChapterProgress>} */
  chaptersProgress: new Map(),

  /** @type {ScanNotification[]} */
  scanNotifications: [],
  /** @type {Set<number>} */
  mutedManga: new Set(),
  upgradesPending: 0,

  /** @type {RefreshState} */
  refreshState: { type: 'idle' },

  libraryInvalidation: 0,

  sourcesInvalidation: 0,

  /** @type {{ total: number, failed: number, newChapters: number, perManga: Map<number, number> } | null} */
  scanResult: null,

  /** @type {Set<number>} */
  scanningMangaIds: new Set(),
});

export const { sigFor, getState, updateState, subscribe } = _store;

/**
 * @param {string} key
 * @param {any} value
 * @param {{ broadcast?: boolean }} [opts]
 */
export function setState(key, value, { broadcast = true } = {}) {
  _store.setState(key, value);
  if (broadcast && _BROADCAST_KEYS.has(key)) broadcastStateChange(key, value);
}
