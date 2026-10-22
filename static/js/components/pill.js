// @ts-check

import { h } from 'preact';
import htm from 'htm';
const html = htm.bind(h);

const BASE = 'inline-flex items-center px-2.5 py-1 text-xs font-medium rounded-md border max-w-44 select-none';
const STATIC = BASE + ' bg-surface-2 border-border text-text';
const DISMISSABLE = BASE + ' bg-surface-2 border-border text-text cursor-pointer hover:bg-danger/10 hover:border-danger/40 hover:text-danger transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent';

/**
 * @param {{
 *   label: string,
 *   onDismiss?: () => void,
 * }} props
 */
export function Pill({ label, onDismiss }) {
  if (onDismiss) {
    return html`
      <button
        type="button"
        class=${DISMISSABLE}
        title=${'Remove: ' + label}
        aria-label=${'Remove ' + label}
        onMouseDown=${(/** @type {MouseEvent} */ e) => e.preventDefault()}
        onClick=${(/** @type {MouseEvent} */ e) => { e.stopPropagation(); onDismiss(); }}
      >
        <span class="truncate">${label}</span>
      </button>
    `;
  }
  return html`
    <span class=${STATIC}>
      <span class="truncate">${label}</span>
    </span>
  `;
}
