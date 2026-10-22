// @ts-check

import { h, render } from 'preact';
import { useState } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { showApiError } from '../toast.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/** @param {{ dbId: number, status: string, sourceName: string, onMigrate: (() => void) | null }} props */
function ImportLinkBanner({ dbId, status, sourceName, onMigrate }) {
  const [state, setState] = useState(status);

  const retry = async () => {
    setState('pending');
    try {
      await api.relinkManga(dbId);
    } catch (e) {
      showApiError(e);
      setState('unlinked');
    }
  };

  const pending = state === 'pending';
  return html`
    <div
      class="flex flex-wrap items-center gap-3 rounded-md border border-warn/30 bg-warn/10 px-4 py-2 text-sm"
      role="status"
    >
      <span class="flex-1 min-w-[16rem] text-text">
        ${pending
          ? t('manga.import_link.pending', { source: sourceName })
          : t('manga.import_link.unlinked', { source: sourceName })}
      </span>
      ${!pending && html`
        <button type="button" class="btn-secondary btn-sm shrink-0" onClick=${retry}>
          ${t('manga.import_link.retry')}
        </button>
      `}
      ${!pending && onMigrate && html`
        <button type="button" class="btn-secondary btn-sm shrink-0" onClick=${onMigrate}>
          ${t('manga.import_link.find_manually')}
        </button>
      `}
    </div>
  `;
}

/**
 * @param {HTMLElement} parent
 * @param {number} dbId
 * @param {string} status
 * @param {string} sourceName
 * @param {{ onMigrate?: (() => void) | null }} [opts]
 * @returns {{ destroy: () => void }}
 */
export function mountImportLinkBanner(parent, dbId, status, sourceName, opts = {}) {
  const mount = document.createElement('div');
  parent.appendChild(mount);
  render(
    html`<${ImportLinkBanner}
      dbId=${dbId}
      status=${status}
      sourceName=${sourceName}
      onMigrate=${opts.onMigrate ?? null}
    />`,
    mount,
  );
  return {
    destroy: () => {
      render(null, mount);
      mount.remove();
    },
  };
}
