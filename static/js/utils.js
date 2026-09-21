// @ts-check

import { t } from './i18n.js';
import { iconCheck } from './icons.js';

/**
 * Returns a debounced version of `fn` that delays invocation by `ms`.
 * The returned function has a `.cancel()` method to clear a pending call.
 * @param {Function} fn
 * @param {number} ms
 * @returns {Function & { cancel: () => void }}
 */
export function debounce(fn, ms) {
  /** @type {number | undefined} */
  let timer = undefined;
  /** @param {any[]} args */
  const debounced = (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => { timer = undefined; fn(...args); }, ms);
  };
  debounced.cancel = () => { clearTimeout(timer); timer = undefined; };
  return debounced;
}

/**
 * @param {string} key
 * @returns {string}
 */
export function getLocal(key) {
  try { return localStorage.getItem(key) ?? ''; } catch { return ''; }
}

/**
 * @param {string} key
 * @param {string} value
 */
export function setLocal(key, value) {
  try { localStorage.setItem(key, value); } catch { }
}

/**
 * @param {string} key
 * @param {number} fallback
 * @returns {number}
 */
export function getLocalInt(key, fallback) {
  const v = parseInt(getLocal(key), 10);
  return Number.isFinite(v) ? v : fallback;
}

/**
 * @param {string} value
 * @returns {any | null}
 */
export function getJsonSafe(value) {
  try { return JSON.parse(value); } catch { return null; }
}

/**
 * @param {string} key
 * @returns {any | null}
 */
export function getLocalJson(key) {
  return getJsonSafe(getLocal(key));
}

/**
 * @param {string} key
 * @param {any} obj
 */
export function setLocalJson(key, obj) {
  setLocal(key, JSON.stringify(obj));
}

/** @type {Record<string, string>} */
const HTML_ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };

/**
 * Escapes a string for safe insertion as text content in HTML.
 * @param {string} str
 * @returns {string}
 */
/**
 * The modifier key label for this platform: Macs use ⌘, everything else Ctrl.
 * Hard-coding ⌘ mislabels the shortcut for the majority of users.
 */
export function modKeyLabel() {
  const p = typeof navigator !== 'undefined'
    ? (navigator.userAgentData?.platform || navigator.platform || '')
    : '';
  return /mac|iphone|ipad|ipod/i.test(p) ? '\u2318' : 'Ctrl';
}

/** The mod-key label combined with a key, e.g. "⌘K" or "Ctrl+K". */
export function modKeyCombo(key) {
  const mod = modKeyLabel();
  return mod === '\u2318' ? `${mod}${key}` : `${mod}+${key}`;
}

export function escapeHtml(str) {
  return String(str ?? '').replace(/[&<>"']/g, c => HTML_ESCAPES[c]);
}

/**
 * Formats a date as "Jan 01, 2025".
 * Accepts an ISO date string, or a Unix timestamp in seconds (number).
 * @param {string | number | null | undefined} val
 * @returns {string}
 */
export function formatDate(val) {
  if (val == null || val === '') return '';
  try {
    const d = typeof val === 'number' ? new Date(val * 1000) : new Date(val);
    if (isNaN(d.getTime())) return '';
    return d.toLocaleDateString('en-US', {
      month: 'short', day: '2-digit', year: 'numeric',
    });
  } catch {
    return String(val);
  }
}

const _CONFIRM_SKIP_PREFIX = 'kani-confirm-skip-';

export function resetAllConfirmDialogs() {
  for (let i = localStorage.length - 1; i >= 0; i--) {
    const k = localStorage.key(i);
    if (k?.startsWith(_CONFIRM_SKIP_PREFIX)) localStorage.removeItem(k);
  }
}

/**
 * Disables the given control(s) for the duration of an async operation and
 * restores their prior state in a `finally` block. Accepts a single element or
 * an iterable (NodeList/array). Returns the awaited result of `fn`; errors
 * propagate after state is restored.
 * @template T
 * @param {Element | Iterable<Element>} target
 * @param {() => Promise<T>} fn
 * @returns {Promise<T>}
 */
export async function withBusy(target, fn) {
  const els = /** @type {HTMLElement[]} */ (
    target instanceof Element ? [target] : [...target]
  );
  const prev = els.map(el => /** @type {any} */ (el).disabled === true);
  for (const el of els) {
    /** @type {any} */ (el).disabled = true;
    el.setAttribute('aria-busy', 'true');
  }
  try {
    return await fn();
  } finally {
    els.forEach((el, i) => {
      /** @type {any} */ (el).disabled = prev[i];
      el.removeAttribute('aria-busy');
    });
  }
}

/**
 * Extracts `has_next_page` from an API result, falling back to a length comparison
 * when the field is absent. Pass `pageSize = 0` (or omit both) when no fallback is needed.
 * @param {any} result
 * @param {number} [itemsLength]
 * @param {number} [pageSize]
 * @returns {boolean}
 */
export function hasNextPage(result, itemsLength = 0, pageSize = 0) {
  if (result?.has_next_page != null) return Boolean(result.has_next_page);
  if (result?.has_next      != null) return Boolean(result.has_next);
  return pageSize > 0 && itemsLength > pageSize;
}

/**
 * Returns true if a chapter is downloaded, considering both the stored status and
 * any in-flight progress event (status === 'completed').
 * @param {{ download_status?: number | null, downloaded?: boolean } | null} chapter
 * @param {{ status?: string } | null} [progress]
 * @returns {boolean}
 */
export function isChapterDownloaded(chapter, progress) {
  if (progress?.status === 'completed') return true;
  return !!(chapter?.download_status >= 2 || chapter?.downloaded);
}

/**
 * Formats a chapter title as "Vol. X. Ch. Y. - Title" (volume omitted if absent).
 * Accepts fields from either local chapters (number/title/volume) or recent updates
 * (chapter_number/chapter_name).
 * @param {{ volume?: number | null, number?: number | null, chapter_number?: number | null, title?: string | null, chapter_name?: string | null }} ch
 * @returns {string}
 */
export function formatChapterTitle(ch) {
  const num = ch.number ?? ch.chapter_number ?? '?';
  let s = '';
  if (ch.volume != null) s += `Vol. ${ch.volume} - `;
  s += `Ch. ${num}`;
  const name = ch.title ?? ch.chapter_name ?? null;
  if (name) s += `: ${name}`;
  return s || `Ch. ${num}`;
}

/**
 * Formats a date string or Date as a human-readable relative time (e.g. "2 hours ago").
 * Returns null if the input is null/undefined/unparseable.
 * @param {string | Date | null | undefined} dateInput
 * @returns {string | null}
 */
export function formatRelativeTime(dateInput) {
  if (dateInput == null) return null;
  const date = typeof dateInput === 'string' ? new Date(dateInput) : dateInput;
  if (isNaN(date.getTime())) return null;
  const diffMs = Date.now() - date.getTime();
  const diffSec = Math.floor(diffMs / 1000);
  if (diffSec < 60) return 'just now';
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return diffMin === 1 ? '1 minute ago' : `${diffMin} minutes ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return diffHr === 1 ? '1 hour ago' : `${diffHr} hours ago`;
  const diffDay = Math.floor(diffHr / 24);
  if (diffDay < 30) return diffDay === 1 ? '1 day ago' : `${diffDay} days ago`;
  const diffMonth = Math.floor(diffDay / 30);
  if (diffMonth < 12) return diffMonth === 1 ? '1 month ago' : `${diffMonth} months ago`;
  const diffYear = Math.floor(diffMonth / 12);
  return diffYear === 1 ? '1 year ago' : `${diffYear} years ago`;
}

/**
 * Mounts a skeleton only after a delay, cancelling if real content arrives first.
 * Prevents skeleton flicker on fast connections.
 *
 * @param {() => void} mountFn   — called after `delayMs` if not cancelled
 * @param {number} [delayMs=150]
 * @returns {() => void}         — cancel function; call when real content is ready
 */
/** Compact relative time: "just now", "5m ago", "3h ago", or a locale date string. */
export function fmtCompactDate(dateStr) {
  try {
    const d = new Date(dateStr + 'Z');
    const diff = Date.now() - d.getTime();
    if (diff < 60_000) return 'just now';
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
    return d.toLocaleDateString();
  } catch { return dateStr; }
}

export function deferredSkeleton(mountFn, delayMs = 150) {
  const t = setTimeout(mountFn, delayMs);
  return () => clearTimeout(t);
}

/**
 * Returns an accessible label for a consecutive-error count badge so AT
 * users don't rely on color alone to infer severity.
 * @param {number} count
 * @returns {string}
 */
export function errorCountAriaLabel(count) {
  if (count >= 3) return `${count} errors — unhealthy`;
  return `${count} ${count === 1 ? 'error' : 'errors'}`;
}

/**
 * Attaches a pull-to-refresh handler to `el`.
 *
 * Opt-in per page, and deliberately so. The gesture says only that something
 * reloaded; on a page whose subject is not obvious from the page itself, a
 * spinner reads as fetching from the sources, which this does not do. `subject`
 * names what was refreshed for the announcement, and is the hook a page that
 * really does reach the sources would extend.
 *
 * @param {HTMLElement} el the element that actually scrolls
 * @param {() => Promise<void> | void} onRefresh
 * @param {{ subject: string, threshold?: number, minSpin?: number }} opts
 * @returns {() => void} cleanup
 */
export function addPullToRefresh(el, onRefresh, { subject, threshold = 60, minSpin = 400 } = {}) {
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  let startY = 0;
  let pulling = false;
  let busy = false;

  /** @type {HTMLElement | null} */
  let indicator = null;

  // Overlaid rather than inserted into the flow: a block appearing above the
  // scroller shifts the whole list down mid-gesture. Sized to its contents, so
  // it covers a strip of one cover rather than a band across every one.
  function _ensureIndicator() {
    if (indicator) return indicator;
    indicator = document.createElement('div');
    indicator.className = 'pull-refresh-indicator';
    indicator.setAttribute('aria-hidden', 'true');
    document.body.appendChild(indicator);
    return indicator;
  }

  function _removeIndicator() {
    indicator?.remove();
    indicator = null;
  }

  /** @param {string} text @param {'spin' | 'done' | null} glyph */
  function _setState(text, glyph) {
    const ind = _ensureIndicator();
    ind.dataset.state = glyph ?? '';
    ind.innerHTML = glyph === 'spin'
      ? `<span class="pull-refresh-spinner"></span><span>${escapeHtml(text)}</span>`
      : glyph === 'done'
        ? `<span class="icon-sm">${iconCheck}</span><span>${escapeHtml(text)}</span>`
        : `<span>${escapeHtml(text)}</span>`;
    return ind;
  }

  /**
   * `el` must be the element that actually scrolls. Given one that does not,
   * `scrollTop` is pinned at 0, so the at-the-top test always passes and every
   * upward flick anywhere in the list reads as a pull. Testing the overflow
   * makes a mis-wired caller inert, and unlike comparing heights it still works
   * on a list shorter than the viewport.
   */
  const scrolls = () => /auto|scroll/.test(getComputedStyle(el).overflowY);
  const atTop = () => scrolls() && el.scrollTop <= 0;

  function onTouchStart(/** @type {TouchEvent} */ e) {
    if (busy || !atTop()) return;
    if (e.touches.length !== 1) return;
    startY = e.touches[0].clientY;
    pulling = false;
  }

  function onTouchMove(/** @type {TouchEvent} */ e) {
    if (busy || !atTop()) return;
    const dy = e.touches[0].clientY - startY;
    if (dy <= 0) return;
    pulling = dy >= threshold;
    // Shown from the first pixel, not only once armed: the gesture was
    // undiscoverable while its only label appeared at the threshold.
    const ind = _setState(pulling ? t('pull_refresh.release') : t('pull_refresh.pull'), null);
    ind.style.opacity = reduced ? '1' : String(Math.min(1, 0.35 + (dy / threshold) * 0.65));
  }

  async function onTouchEnd() {
    if (busy) return;
    if (!pulling) { _removeIndicator(); return; }
    pulling = false;
    busy = true;

    const ind = _setState(t('pull_refresh.refreshing'), 'spin');
    ind.style.opacity = '1';
    const started = Date.now();
    try {
      await onRefresh();
    } catch {
      // Swallowed deliberately: this runs from a passive listener, where a
      // rejection would escape and strand the indicator on screen. The page's
      // own fetch reports its failure.
    } finally {
      // A refresh that returns in 80ms would otherwise flash and vanish, which
      // reads as the gesture not having fired at all.
      const spun = Date.now() - started;
      if (spun < minSpin) await new Promise((r) => setTimeout(r, minSpin - spun));

      _setState(t('pull_refresh.done'), 'done').style.opacity = '1';
      announce(t('pull_refresh.done.announce', { subject }));
      setTimeout(() => {
        if (indicator) indicator.style.opacity = '0';
        setTimeout(_removeIndicator, reduced ? 0 : 200);
        busy = false;
      }, 700);
    }
  }

  el.addEventListener('touchstart', onTouchStart, { passive: true });
  el.addEventListener('touchmove', onTouchMove, { passive: true });
  el.addEventListener('touchend', onTouchEnd, { passive: true });
  return () => {
    el.removeEventListener('touchstart', onTouchStart);
    el.removeEventListener('touchmove', onTouchMove);
    el.removeEventListener('touchend', onTouchEnd);
    _removeIndicator();
  };
}


/** @type {HTMLElement | null} */
let _liveRegion = null;

/**
 * Writes `message` to a singleton `aria-live="polite"` region so screen readers
 * announce it without moving focus. Call for results that have no visible toast.
 * @param {string} message
 */
export function announce(message) {
  if (!_liveRegion) {
    _liveRegion = document.createElement('div');
    _liveRegion.setAttribute('aria-live', 'polite');
    _liveRegion.setAttribute('aria-atomic', 'true');
    _liveRegion.className = 'sr-only';
    document.body.appendChild(_liveRegion);
  }
  _liveRegion.textContent = '';
  requestAnimationFrame(() => { if (_liveRegion) _liveRegion.textContent = message; });
}


/** @param {number} bytes */
export function formatBytes(bytes) {
  if (bytes == null) return '—';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

/** @param {number} secs */
export function formatDuration(secs) {
  if (secs == null) return '—';
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${Math.floor(secs)}s`;
}
