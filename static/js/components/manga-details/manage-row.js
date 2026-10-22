
import { h } from 'preact';
import htm from 'htm';

const html = htm.bind(h);

/**
 * @param {{ label: any, desc?: any, children?: any }} props
 */
export function ManageRow({ label, desc, children }) {
  return html`
    <div class="py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0">
      <div class="flex items-center justify-between gap-4">
        <div class="min-w-0">
          <p class="text-sm font-medium text-text">${label}</p>
          ${desc && html`<p class="text-xs text-text-muted mt-0.5">${desc}</p>`}
        </div>
        ${children}
      </div>
    </div>
  `;
}
