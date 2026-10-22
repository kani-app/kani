// @ts-check

import { h } from 'preact';
import htm from 'htm';
const html = htm.bind(h);

/**
 * @param {{
 *   icon?: any,
 *   label: string,
 *   value: string | number,
 * }} props
 */
export function StatRow({ icon, label, value }) {
  return html`
    <div class="flex items-center gap-2 text-sm">
      ${icon ? html`<span class="icon-sm text-text-muted shrink-0" aria-hidden="true">${html([icon])}</span>` : null}
      <span class="text-text-muted flex-1">${label}</span>
      <span class="font-medium text-text">${value}</span>
    </div>
  `;
}
