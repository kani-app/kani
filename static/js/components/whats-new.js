// @ts-check
// What's-new dialog — renders the server-rendered, sanitised changelog HTML.

import { h } from 'preact';
import htm from 'htm';
import { Modal, mountIntoModalRoot } from './modal.js';
import { t } from '../i18n.js';

const html = htm.bind(h);

/**
 * @param {{ version: string, bodyHtml: string, onClose: () => void }} props
 */
function WhatsNewModal({ version, bodyHtml, onClose }) {
  return html`
    <${Modal}
      open=${true}
      title=${t('changelog.title')}
      onClose=${onClose}
      footer=${html`
        <button type="button" class="btn-primary btn-sm" onClick=${onClose}>${t('common.ok')}</button>
      `}
    >
      <p class="text-xs text-text-muted mb-3">${t('changelog.version', { version })}</p>
      <div class="prose-kani max-h-96 overflow-y-auto" dangerouslySetInnerHTML=${{ __html: bodyHtml }} />
    </${Modal}>
  `;
}

/**
 * The HTML must already be sanitised server-side (`render_description`), which is
 * why this takes markup rather than markdown — the client never parses it.
 * @param {string} version
 * @param {string} bodyHtml
 * @returns {Promise<void>}
 */
export function showWhatsNew(version, bodyHtml) {
  return new Promise((resolve) => {
    let cleanup = () => {};
    cleanup = mountIntoModalRoot(html`
      <${WhatsNewModal}
        version=${version}
        bodyHtml=${bodyHtml}
        onClose=${() => { cleanup(); resolve(); }}
      />
    `);
  });
}
