// @ts-check
// Mobile navigation for permitted destinations beyond the five permanent tab slots.

import { h } from 'preact';
import htm from 'htm';
import { Modal, mountIntoModalRoot } from './modal.js';
import { navigate } from '../router.js';
import { hasPermission } from '../session.js';
import { Icon } from './icon.js';
import { t } from '../i18n.js';

const html = htm.bind(h);

/**
 * @param {{ items: Array<{ href: string, label: string, icon: string }>, onClose: () => void }} props
 */
function MoreSheet({ items, onClose }) {
  const path = location.pathname;
  return html`
    <${Modal} open=${true} sheet=${true} title=${t('nav.more')} onClose=${onClose}>
      <nav class="flex flex-col -my-2" aria-label=${t('nav.more')}>
        ${items.map(item => {
          const active = item.href === '/' ? path === '/' : path.startsWith(item.href);
          return html`
            <a
              key=${item.href}
              href=${item.href}
              aria-current=${active ? 'page' : undefined}
              class=${'flex items-center gap-3 py-3 text-sm hover:text-accent focus-visible:outline-none focus-visible:text-accent border-b border-border-subtle last:border-0 '
                + (active ? 'text-accent font-medium' : 'text-text')}
              onClick=${(/** @type {MouseEvent} */ e) => { e.preventDefault(); onClose(); navigate(item.href); }}
            >
              <span class=${'icon-md shrink-0 ' + (active ? 'text-accent' : 'text-text-muted')} aria-hidden="true"><${Icon} svg=${item.icon} /></span>
              <span>${item.label}</span>
            </a>
          `;
        })}
      </nav>
    </${Modal}>
  `;
}

/**
 * @param {Array<{ href: string, label: string, icon: string, perm?: string }>} defs
 */
export function showMoreSheet(defs) {
  const items = defs.filter(d => !d.perm || hasPermission(d.perm));
  let cleanup = () => {};
  cleanup = mountIntoModalRoot(html`
    <${MoreSheet} items=${items} onClose=${() => cleanup()} />
  `);
}
