// @ts-check
// Cover tile size — one preference behind every cover surface: the library and
// source grids fit whole tiles to the space they have, and the search rails
// size their items from the same token.

import { getLocal, setLocal } from './utils.js';

/** @typedef {'sm'|'md'|'lg'} TileSize */

const KEY = 'kani_tile_size';

/** @type {TileSize} */
const DEFAULT = 'md';

/** @type {TileSize[]} */
export const TILE_SIZES = ['sm', 'md', 'lg'];

/** @returns {TileSize} */
export function getTileSize() {
  const stored = getLocal(KEY);
  return TILE_SIZES.includes(/** @type {TileSize} */ (stored)) ? /** @type {TileSize} */ (stored) : DEFAULT;
}

/**
 * The default carries no attribute, so `--tile-min-w` keeps the value declared
 * in `@theme` rather than being restated in a second place.
 *
 * @param {TileSize} size
 */
function applyTileSize(size) {
  const h = document.documentElement;
  if (size === DEFAULT) h.removeAttribute('data-tile-size');
  else h.setAttribute('data-tile-size', size);
}

/** @param {TileSize} size */
export function setTileSize(size) {
  setLocal(KEY, size);
  applyTileSize(size);
}

export function initTileSize() {
  applyTileSize(getTileSize());
}

/** @returns {number} The current tile width in pixels, 0 when unmeasurable. */
export function tileWidth() {
  if (typeof getComputedStyle !== 'function') return 0;
  const v = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--tile-min-w'));
  return Number.isFinite(v) && v > 0 ? v : 0;
}
