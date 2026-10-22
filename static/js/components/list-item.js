// @ts-check

import { h } from 'preact';
import htm from 'htm';
const html = htm.bind(h);

/**
 * @param {{
 *   avatar?: string,
 *   icon?: any,
 *   title: string,
 *   subtitle?: string,
 *   right?: any,
 *   active?: boolean,
 *   onClick?: () => void,
 * }} props
 */
export function ListItem({ avatar, icon, title, subtitle, right, active, onClick }) {
  return html`
    <div
      class=${`li-row${active ? ' active' : ''}`}
      role="button"
      tabindex="0"
      onClick=${onClick}
      onKeyDown=${(/** @type {KeyboardEvent} */ e) => { if (e.key === 'Enter' || e.key === ' ') onClick?.(); }}
    >
      ${avatar != null
        ? html`<span class="avatar" aria-hidden="true">${avatar}</span>`
        : icon
        ? html`<span class="icon-sm shrink-0" aria-hidden="true">${html([icon])}</span>`
        : null
      }
      <span class="flex flex-col min-w-0 flex-1">
        <span class="li-title truncate">${title}</span>
        ${subtitle ? html`<span class="li-sub truncate">${subtitle}</span>` : null}
      </span>
      ${right ? html`<span class="shrink-0">${right}</span>` : null}
    </div>
  `;
}
