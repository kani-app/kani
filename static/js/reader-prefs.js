
import * as api from './api.js';
import { getLocal, setLocal, getLocalJson, setLocalJson } from './utils.js';

/** @typedef {'scroll'|'paged'|'webtoon'|'continuous-paged'} ReadingMode */
/** @typedef {'rtl'|'ltr'} Direction */
/** @typedef {'both'|'width'|'height'} FitMode */
/** @typedef {'black'|'white'|'sepia'} BgColor */
/** @typedef {'multiply'|'screen'|'overlay'|'color'} TintBlend */
/** @typedef {'full'|'pagecount'|'off'} MiniStripMode */

/**
 * @typedef {Object} ReaderPrefs
 * @property {number|null}     mangaId
 * @property {ReadingMode}     mode
 * @property {Direction}       direction
 * @property {FitMode}         fit
 * @property {BgColor}         bg
 * @property {boolean}         bgTintPage  — multiply bg colour onto greyscale pages
 * @property {boolean}         smoothScroll
 * @property {boolean}         doublePage
 * @property {boolean}         autoSpread
 * @property {boolean}         spreadOffset — shift double-page parity so page 0 is paired
 * @property {number}          preloadCount
 * @property {number}          brightness
 * @property {number}          contrast
 * @property {number}          saturation
 * @property {boolean}         grayscale
 * @property {boolean}         invert
 * @property {number}          cropTop      — percentage 0–50
 * @property {number}          cropBottom   — percentage 0–50
 * @property {number}          cropLeft     — percentage 0–50
 * @property {number}          cropRight    — percentage 0–50
 * @property {boolean}         cropV2       — sentinel: true means crop values are percentages
 * @property {boolean}         pageOverlay
 * @property {string}          tapLeft
 * @property {string}          tapCenter
 * @property {string}          tapRight
 * @property {string}          tintColor
 * @property {number}          tintOpacity
 * @property {TintBlend}       tintBlend
 * @property {number}          slideshowInterval
 * @property {number}          inactivityTimeout
 * @property {boolean}         endCardInPaged — show the end-of-chapter card in paged/continuous modes
 * @property {MiniStripMode}   miniStrip       — bottom progress strip: full segments / page-count only / off
 */

/** Hardcoded fallbacks — every key must appear here so callers can depend on the shape. */
const DEFAULTS = /** @type {ReaderPrefs} */ ({
  mangaId:     null,
  mode:        'scroll',
  direction:   'rtl',
  fit:         'both',
  bg:          'black',
  bgTintPage:  false,
  smoothScroll: false,
  doublePage:  false,
  autoSpread:  true,
  spreadOffset: false,
  preloadCount: 2,
  brightness:  100,
  contrast:    100,
  saturation:  100,
  grayscale:   false,
  invert:      false,
  cropTop:     0,
  cropBottom:  0,
  cropLeft:    0,
  cropRight:   0,
  cropV2:      true,
  pageOverlay: false,
  tapLeft:     'prev',
  tapCenter:   'menu',
  tapRight:    'next',
  tintColor:          '#ff9966', // audit-ignore: default tint colour value
  tintOpacity:        0,
  tintBlend:          'multiply',
  slideshowInterval:  5,
  inactivityTimeout:  0,
  endCardInPaged:     false,
  miniStrip:          'full',
});

/** Keys that have a matching global localStorage entry (for backwards-compat). */
const LS_MAP = {
  mode:        'kani_reader_mode',
  direction:   'kani_reader_direction',
  fit:         'kani_reader_fit',
  smoothScroll:'kani_reader_smooth',
  doublePage:  'kani_reader_double',
  autoSpread:  'kani_reader_spread',
};

/** In-memory cache: mangaId → merged prefs object. */
const _cache = new Map();
/** Debounce timers for server writes, keyed by mangaId. */
const _syncTimers = new Map();

/**
 * Read global localStorage defaults on top of hardcoded defaults.
 * @returns {ReaderPrefs}
 */
function _fromLocalStorage() {
  const p = { ...DEFAULTS };
  const mode = getLocal(LS_MAP.mode);
  if (mode === 'paged' || mode === 'scroll' || mode === 'webtoon' || mode === 'continuous-paged') {
    p.mode = mode;
  }
  const dir = getLocal(LS_MAP.direction);
  if (dir === 'ltr' || dir === 'rtl') p.direction = dir;
  const fit = getLocal(LS_MAP.fit);
  if (fit === 'both' || fit === 'width' || fit === 'height') p.fit = /** @type {FitMode} */ (fit);
  if (getLocal(LS_MAP.smoothScroll) === 'true')  p.smoothScroll = true;
  if (getLocal(LS_MAP.doublePage)   === 'true')  p.doublePage   = true;
  if (getLocal(LS_MAP.autoSpread)   === 'false') p.autoSpread   = false;

  const ext = getLocalJson('kani_reader_prefs_global');
  if (ext && typeof ext === 'object') Object.assign(p, ext);

  return p;
}

/**
 * Load per-manga prefs from the server, merge over localStorage defaults, and
 * cache the result. The server blob overrides everything.
 *
 * Call this once per chapter load (after getting the manga_id from the manifest).
 *
 * @param {number|null} mangaId
 * @returns {Promise<ReaderPrefs>}
 */
export async function loadReaderPrefs(mangaId) {
  const base = _fromLocalStorage();
  base.mangaId = mangaId;

  if (!mangaId) {
    _cache.set(null, base);
    return base;
  }

  try {
    const tracking = await api.getMangaTracking(mangaId);
    if (tracking?.reader_prefs) {
      const serverPrefs = JSON.parse(tracking.reader_prefs);
      if (serverPrefs && typeof serverPrefs === 'object' && !Array.isArray(serverPrefs)) {
        Object.assign(base, serverPrefs);
      }
    }
    if (tracking?.reading_direction === 'ltr' || tracking?.reading_direction === 'rtl') {
      base.direction = tracking.reading_direction;
    }
  } catch { }

  base.mangaId = mangaId;

  // Absence of cropV2 identifies pixel-based crop values that cannot be reused as percentages.
  if (!base.cropV2) {
    base.cropTop = 0; base.cropBottom = 0; base.cropLeft = 0; base.cropRight = 0;
    base.cropV2 = true;
  }

  _cache.set(mangaId, base);
  return base;
}

/**
 * Update a single pref key in place, write through to localStorage (global
 * default), and debounce-sync the full per-manga object to the server.
 *
 * @param {ReaderPrefs} prefs  — the live object returned by loadReaderPrefs
 * @param {string} key
 * @param {*} value
 */
export function setReaderPref(prefs, key, value) {
  // @ts-ignore: ReaderPrefs is indexed by a validated dynamic preference key.
  prefs[key] = value;

  // Global keys remain part of the reader-preference compatibility contract.
  if (key in LS_MAP) {
    // @ts-ignore: LS_MAP is indexed only after the membership check.
    setLocal(LS_MAP[key], String(value));
  }

  _persistGlobalExtended(prefs);

  // Sync to server (debounced).
  _scheduleSyncToServer(prefs);
}

/**
 * Write the extended prefs (everything beyond the legacy LS_MAP keys) into
 * a single 'kani_reader_prefs_global' JSON blob so they survive across sessions
 * even before the manga's tracking row is created.
 * @param {ReaderPrefs} prefs
 */
function _persistGlobalExtended(prefs) {
  /** @type {Record<string,*>} */
  const ext = {};
  for (const key of Object.keys(DEFAULTS)) {
    if (key === 'mangaId') continue;
    if (!(key in LS_MAP)) {
      // @ts-ignore: DEFAULTS supplies keys shared with ReaderPrefs.
      ext[key] = prefs[key];
    }
  }
  setLocalJson('kani_reader_prefs_global', ext);
}

/**
 * Debounce a server PUT so rapid slider changes don't spam the API.
 * @param {ReaderPrefs} prefs
 */
function _scheduleSyncToServer(prefs) {
  const mangaId = prefs.mangaId;
  if (!mangaId) return;

  if (_syncTimers.has(mangaId)) clearTimeout(_syncTimers.get(mangaId));

  _syncTimers.set(mangaId, setTimeout(() => {
    _syncTimers.delete(mangaId);

    /** @type {Record<string,*>} */
    const blob = {};
    for (const key of Object.keys(DEFAULTS)) {
      if (key === 'mangaId' || key === 'direction') continue;
      // @ts-ignore: DEFAULTS supplies keys shared with ReaderPrefs.
      blob[key] = prefs[key];
    }

    api.setMangaTracking(mangaId, { reader_prefs: JSON.stringify(blob) }).catch(() => {});

    api.setMangaTracking(mangaId, { reading_direction: prefs.direction }).catch(() => {});
  }, 800));
}

/**
 * Cancel any pending server sync for this manga (call from reader destroy).
 * @param {ReaderPrefs} prefs
 */
export function cancelReaderPrefsSync(prefs) {
  const mangaId = prefs.mangaId;
  if (mangaId && _syncTimers.has(mangaId)) {
    clearTimeout(_syncTimers.get(mangaId));
    _syncTimers.delete(mangaId);
  }
}
