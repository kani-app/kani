// @ts-check
// Tile size picker. Sibling of PageSizeSelect, which stays numeric for the
// chapter list; this one carries the named sizes the grids and rails share.

import { h } from 'preact';
import htm from 'htm';
import { t } from '../i18n.js';
import { TILE_SIZES } from '../tile-size.js';

const html = htm.bind(h);

/**
 * @param {{
 *   value: import('../tile-size.js').TileSize,
 *   onChange: (size: import('../tile-size.js').TileSize) => void,
 *   class?: string,
 * }} props
 */
export function TileSizeSelect({ value, onChange, class: className = '' }) {
  return html`
    <select
      class="input ${className}"
      aria-label=${t('common.tile_size')}
      value=${value}
      onChange=${(/** @type {Event} */ e) =>
        onChange(/** @type {any} */ (/** @type {HTMLSelectElement} */ (e.target).value))}
    >
      ${TILE_SIZES.map(size => html`<option value=${size}>${t(`common.tile_size.${size}`)}</option>`)}
    </select>
  `;
}
