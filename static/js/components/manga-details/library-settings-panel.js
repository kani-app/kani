// @ts-check

import { h, render } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { hasPermission } from '../../session.js';
import { t } from '../../i18n.js';
import { ManageRow } from './manage-row.js';
const html = htm.bind(h);

/**
 * @param {HTMLElement} containerEl
 * @param {{ dbId: number, autoScan: boolean }} ctx
 */
export function mountLibrarySettingsPanel(containerEl, ctx) {
  const mount = document.createElement('div');
  containerEl.appendChild(mount);
  render(html`<${LibrarySettingsPanel} dbId=${ctx.dbId} initialAutoScan=${ctx.autoScan} />`, mount);
}

/** @param {{ label: any, desc: any, checked: boolean, onChange: (e: Event) => void }} props */
function ToggleRow({ label, desc, checked, onChange }) {
  return html`
    <${ManageRow} label=${label} desc=${desc}>
      <label class="kani-toggle cursor-pointer shrink-0">
        <input type="checkbox" class="kani-toggle__input" checked=${checked} onChange=${onChange} />
        <span class="kani-toggle__track"></span>
      </label>
    <//>
  `;
}

function LibrarySettingsPanel({ dbId, initialAutoScan }) {
  const [autoScan, setAutoScan] = useState(/** @type {boolean} */ (initialAutoScan));
  const [autoDownload, setAutoDownload] = useState(false);
  const [webhooks, setWebhooks] = useState(true);
  const [preferredOnly, setPreferredOnly] = useState(true);

  useEffect(() => {
    api.getMangaDetails(dbId).then(res => {
      if (res?.auto_download != null) setAutoDownload(res.auto_download);
      if (res?.download_all_preferred_only != null) setPreferredOnly(res.download_all_preferred_only);
    }).catch(() => {});
    api.getMangaWebhookNotify(dbId).then(res => {
      if (res?.enabled != null) setWebhooks(res.enabled);
    }).catch(() => {});
  }, [dbId]);

  async function handleToggle(current, setter, apiFn) {
    setter(!current);
    try { await apiFn(!current); } catch { setter(current); }
  }

  const canManage = hasPermission('library:manage');
  const canDownload = hasPermission('chapter:download');
  if (!canManage && !canDownload) return null;

  return html`
    <div class="bg-surface border border-border rounded-xl px-4 md:px-6 py-1">
      ${canManage && html`<${ToggleRow}
        label=${t('manga.settings.auto_scan')}
        desc=${t('manga.settings.auto_scan.desc')}
        checked=${autoScan}
        onChange=${() => handleToggle(autoScan, setAutoScan, v => api.toggleAutoScan(dbId, v))}
      />`}
      ${canManage && html`<${ToggleRow}
        label=${t('manga.settings.auto_download')}
        desc=${t('manga.settings.auto_download.desc')}
        checked=${autoDownload}
        onChange=${() => handleToggle(autoDownload, setAutoDownload, v => api.toggleAutoDownload(dbId, v))}
      />`}
      ${canManage && html`<${ToggleRow}
        label=${t('manga.settings.webhooks')}
        desc=${t('manga.settings.webhooks.desc')}
        checked=${webhooks}
        onChange=${() => handleToggle(webhooks, setWebhooks, v => api.setMangaWebhookNotify(dbId, v))}
      />`}
      ${canDownload && html`<${ToggleRow}
        label=${t('manga.settings.preferred_only')}
        desc=${t('manga.settings.preferred_only.desc')}
        checked=${preferredOnly}
        onChange=${() => handleToggle(preferredOnly, setPreferredOnly, v => api.toggleDownloadAllPreferred(dbId, v))}
      />`}
    </div>
  `;
}
