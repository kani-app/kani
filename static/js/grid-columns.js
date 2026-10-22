// @ts-check

/**
 * Batch size where fitting has no answer — a page that scrolls, or a grid that
 * cannot be measured yet.
 */
export const FALLBACK_PAGE_SIZE = 24;

/**
 * Ceiling on one page. A tall window at the smallest tile can fit a lot, and a
 * browser-backed source pays one page load per 28 of them.
 */
export const MAX_PAGE_SIZE = 120;

/**
 * @param {string} value A computed `grid-template-columns` value.
 * @returns {number} Track count, or 0 when the value names no explicit tracks.
 */
export function countTracks(value) {
  if (!value || value === 'none') return 0;
  let depth = 0;
  let tracks = 0;
  let inTrack = false;
  for (const ch of value) {
    if (ch === '(') depth += 1;
    else if (ch === ')') depth -= 1;
    if (depth === 0 && /\s/.test(ch)) {
      inTrack = false;
      continue;
    }
    if (!inTrack) {
      inTrack = true;
      tracks += 1;
    }
  }
  return tracks;
}

/**
 * Track count for a computed `grid-template-columns`, or 0 when the value names
 * no resolved tracks. An unlaid element reports its specified `repeat(...)`
 * rather than the used track list, which is unknown rather than one column.
 *
 * @param {string} value
 * @returns {number}
 */
export function resolvedColumns(value) {
  if (!value || value === 'none' || value.startsWith('repeat(')) return 0;
  return countTracks(value);
}

/**
 * @param {HTMLElement | null} gridEl
 * @returns {number} Columns currently laid out, or 0 when not measurable.
 */
function columnCount(gridEl) {
  if (!gridEl) return 0;
  return resolvedColumns(getComputedStyle(gridEl).gridTemplateColumns);
}

/**
 * Rows of `tileH` that fit in `boundsH`, given `gap` between them. At least one
 * row: a viewport too short for a whole tile still has to show something.
 *
 * @param {number} boundsH
 * @param {number} tileH
 * @param {number} gap
 * @returns {number} Rows, or 0 when the inputs are not measurable.
 */
export function rowsThatFit(boundsH, tileH, gap) {
  if (!(boundsH > 0) || !(tileH > 0)) return 0;
  return Math.max(1, Math.floor((boundsH + gap) / (tileH + gap)));
}

/**
 * Whether the fit-to-viewport shell is in force. The breakpoints live in
 * `app.css`, which publishes `--grid-fit` inside the same media query — the
 * alternative is restating them here and letting the two drift.
 *
 * @returns {boolean}
 */
export function gridFitEnabled() {
  if (typeof getComputedStyle !== 'function') return false;
  return getComputedStyle(document.documentElement).getPropertyValue('--grid-fit').trim() === '1';
}

/**
 * @param {HTMLElement} gridEl
 * @returns {number} Row gap in pixels, 0 when not measurable.
 */
function rowGapOf(gridEl) {
  const gap = parseFloat(getComputedStyle(gridEl).rowGap);
  return Number.isFinite(gap) ? gap : 0;
}

/**
 * One tile's height, derived from the column width and the cover's fixed 2:3
 * ratio rather than measured.
 *
 * Measuring a card is unreliable in both directions: after a tile-size change
 * the cards on screen are still the old size, and `content-visibility: auto`
 * lets the browser report `contain-intrinsic-size` instead of the real height.
 * Either produces a capacity that changes again on the next measurement.
 *
 * Only a caption beneath the card is measured, because nothing predicts how
 * many lines it wraps to.
 *
 * @param {HTMLElement | null} gridEl
 * @returns {number} Pixels, or 0 when not measurable.
 */
function tileHeight(gridEl) {
  if (!gridEl) return 0;
  const columns = columnCount(gridEl);
  if (columns === 0) return 0;

  const gap = rowGapOf(gridEl);
  const columnWidth = (gridEl.getBoundingClientRect().width - gap * (columns - 1)) / columns;
  if (!(columnWidth > 0)) return 0;

  const cell = /** @type {HTMLElement | null} */ (gridEl.firstElementChild);
  const card = cell?.querySelector('.manga-card');
  if (!cell || !card || card === cell) return columnWidth * 1.5;

  const caption = cell.getBoundingClientRect().height - card.getBoundingClientRect().height;
  return columnWidth * 1.5 + Math.max(0, caption);
}

/**
 * How many tiles fit in `boundsEl` at the grid's current column width.
 *
 * @param {HTMLElement | null} gridEl
 * @param {HTMLElement | null} boundsEl The element the grid may not overflow.
 * @returns {number} Tiles, or 0 when not measurable — as with `columnCount`,
 *   0 means unknown rather than none.
 */
export function gridCapacity(gridEl, boundsEl) {
  if (!gridEl || !boundsEl) return 0;
  const columns = columnCount(gridEl);
  const tileH = tileHeight(gridEl);
  if (columns === 0 || tileH === 0) return 0;

  // The grid rarely starts at the top of its bounds — a shelf or a heading can
  // sit above it — so measure from where the grid actually begins.
  const top = gridEl.getBoundingClientRect().top - boundsEl.getBoundingClientRect().top;
  const available = boundsEl.clientHeight - Math.max(0, top);
  const rows = rowsThatFit(available, tileH, rowGapOf(gridEl));
  return rows === 0 ? 0 : columns * rows;
}

/**
 * Watches `observedEl` and reports when the number of tiles that fit changes.
 *
 * Coalesced to a frame: capacity moves with every pixel of height, and each
 * change costs a request the source cache has no entry for.
 *
 * @param {HTMLElement | null} observedEl
 * @param {() => HTMLElement | null} getGridEl
 * @param {() => HTMLElement | null} getBoundsEl
 * @param {(capacity: number) => void} onChange Called only when it changes.
 * @param {number} [current] What the page already holds. Pass it when the first
 *   page was fetched before the grid could be measured, so the first real
 *   measurement is reported rather than adopted silently.
 * @returns {() => void} Unsubscribe.
 */
export function observeCapacity(observedEl, getGridEl, getBoundsEl, onChange, current = 0) {
  if (!observedEl || typeof ResizeObserver !== 'function') return () => {};
  let last = current || gridCapacity(getGridEl(), getBoundsEl());
  let frame = 0;

  const measure = () => {
    frame = 0;
    const next = gridCapacity(getGridEl(), getBoundsEl());
    if (next === 0) return;
    if (last === 0) { last = next; return; }
    if (next !== last) {
      last = next;
      onChange(next);
    }
  };

  const observer = new ResizeObserver(() => {
    if (frame) cancelAnimationFrame(frame);
    frame = requestAnimationFrame(measure);
  });
  observer.observe(observedEl);
  return () => {
    if (frame) cancelAnimationFrame(frame);
    observer.disconnect();
  };
}
