// @ts-check

import { h, render } from 'preact';
import { useState } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { showApiError } from '../toast.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/** @param {{ dbId: number, count: number, onDismiss: () => void }} props */
function SuppressedBanner({ dbId, count, onDismiss }) {
  const [busy, setBusy] = useState(false);

  const dismiss = async () => {
    setBusy(true);
    try {
      await api.dismissSuppressedChapters(dbId);
      onDismiss();
    } catch (e) {
      showApiError(e);
      setBusy(false);
    }
  };

  return html`
    <div
      class="flex items-center gap-3 rounded-md border border-warn/30 bg-warn/10 px-4 py-2 text-sm"
      role="status"
    >
      <span class="flex-1 text-text">${t('manga.suppressed.message', { count })}</span>
      <button
        type="button"
        class="btn-ghost btn-sm shrink-0"
        disabled=${busy}
        onClick=${dismiss}
      >
        ${t('manga.suppressed.dismiss')}
      </button>
    </div>
  `;
}

/**
 * @param {HTMLElement} parent
 * @param {number} dbId
 * @param {number} count
 * @returns {{ destroy: () => void }}
 */
export function mountSuppressedBanner(parent, dbId, count) {
  const mount = document.createElement('div');
  parent.appendChild(mount);
  const destroy = () => {
    render(null, mount);
    mount.remove();
  };
  render(
    html`<${SuppressedBanner} dbId=${dbId} count=${count} onDismiss=${destroy} />`,
    mount
  );
  return { destroy };
}
