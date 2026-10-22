// @ts-check

import { getLocal, setLocal } from './utils.js';

/** @typedef {'sm'|'md'|'lg'} CoverQuality */

const KEY = 'kani_cover_quality';

/** @type {CoverQuality} */
const DEFAULT = 'lg';

/** @type {CoverQuality[]} */
const COVER_QUALITIES = ['sm', 'md', 'lg'];

const LOCAL_COVER = /^\/rest\/manga\/\d+\/cover(?:\?|$)/;

/** @returns {CoverQuality} */
export function getCoverQuality() {
  const stored = getLocal(KEY);
  return COVER_QUALITIES.includes(/** @type {CoverQuality} */ (stored))
    ? /** @type {CoverQuality} */ (stored)
    : DEFAULT;
}

/** @param {string} quality */
export function setCoverQuality(quality) {
  if (COVER_QUALITIES.includes(/** @type {CoverQuality} */ (quality))) setLocal(KEY, quality);
}

/**
 * @param {string | null | undefined} url
 * @param {CoverQuality} [quality]
 * @returns {string | null | undefined}
 */
export function applyCoverQuality(url, quality = getCoverQuality()) {
  if (!url || !LOCAL_COVER.test(url)) return url;
  const [path, query = ''] = url.split('?');
  const params = new URLSearchParams(query);
  params.set('size', quality);
  return `${path}?${params}`;
}
