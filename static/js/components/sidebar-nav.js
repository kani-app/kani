// @ts-check

import { h } from 'preact';
import htm from 'htm';
const html = htm.bind(h);

/**
 * Sidebar nav container with optional header title.
 *
 * @param {{
 *   title?: string,
 *   children: any,
 * }} props
 */
export function SidebarNav({ title, children, class: className = '' }) {
  return html`
    <div class=${'flex-1 overflow-y-auto flex flex-col gap-0.5 py-1 ' + className}>
      ${title && html`<p class="px-3 py-1 text-xs font-semibold uppercase tracking-wider text-text-muted mb-1">${title}</p>`}
      ${children}
    </div>
  `;
}
