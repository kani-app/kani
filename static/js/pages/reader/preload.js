// @ts-check
// Pure preload/pace math for the reader. No DOM, no closure state.

/**
 * Rolling pages-per-minute from a list of page-advance timestamps (ms).
 * Needs at least two samples spanning a positive interval.
 * @param {number[]} paceTimestamps
 * @returns {number | null}
 */
export function pagesPerMinute(paceTimestamps) {
  if (!paceTimestamps || paceTimestamps.length < 2) return null;
  const dtMin = (paceTimestamps[paceTimestamps.length - 1] - paceTimestamps[0]) / 60000;
  return dtMin > 0 ? (paceTimestamps.length - 1) / dtMin : null;
}

/**
 * Minutes to read the remaining pages at the given pace, or null when unknown.
 * The caller formats the number into a user-facing string.
 * @param {number | null} ppm — pages per minute
 * @param {number} remaining — pages left after the current one
 * @returns {number | null}
 */
export function minutesRemaining(ppm, remaining) {
  if (!ppm) return null;
  if (remaining <= 0) return 0;
  return remaining / ppm;
}

/**
 * Smart preload count: how many images can fully load within one average
 * page-read interval, clamped to [1, max]. Falls back to `max` until there's
 * enough pace/fetch data.
 * @param {{ max: number, fetchMsLog: number[], ppm: number | null, minSamples?: number }} opts
 * @returns {number}
 */
export function adaptivePreloadCount({ max, fetchMsLog, ppm, minSamples = 3 }) {
  if (!fetchMsLog || fetchMsLog.length < minSamples || !ppm) return max;
  const avgFetchMs = fetchMsLog.reduce((a, b) => a + b, 0) / fetchMsLog.length;
  if (avgFetchMs <= 0) return max;
  const msPerPage = 60000 / ppm;
  return Math.max(1, Math.min(max, Math.floor(msPerPage / avgFetchMs)));
}

/**
 * Page index at which to begin preloading the next chapter. Paged mode triggers
 * near the last few pages; scroll mode at 80% through.
 * @param {'paged' | 'continuous-paged' | 'scroll' | string} mode
 * @param {number} pagesLength
 * @returns {number}
 */
export function preloadThreshold(mode, pagesLength) {
  return mode === 'paged'
    ? pagesLength - 3
    : Math.floor(pagesLength * 0.8);
}
