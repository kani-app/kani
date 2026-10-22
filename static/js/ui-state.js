// @ts-check
// UI-local state atoms: not cross-tab, not server-cache. Signal-backed via the
// shared store factory; `sigFor` is exported for reactive readers.

import { createSignalStore } from './signal-store.js';

const _store = createSignalStore({
  /** @type {Set<number>} */
  inFlightChapters: new Set(),

  /** @type {Map<number, boolean>} */
  mangaNotifyPrefs: new Map(),

  /**
   * Incremented per source when a source preference is changed.
   * @type {Map<number, number>}
   */
  sourcePreferenceVersion: new Map(),
});

export const { sigFor, getState, setState, updateState, subscribe } = _store;
