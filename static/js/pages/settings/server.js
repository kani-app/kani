// @ts-check
// Settings — Server section (restart, stop, admin controls).

import { h } from 'preact';
import { useState } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { showConfirm } from '../../components/modal.js';
import { showToast, showApiError } from '../../components/toast.js';
import { SettingsGroup, SettingsRow } from './_shared.js';
import { useBusy } from '../../hooks/use-busy.js';
import { iconSpinner } from '../../icons.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/** @param {{ label: string, description: string, btnLabel: string, cls: string, onRun: () => Promise<void> }} props */
function ActionRow({ label, description, btnLabel, cls, onRun }) {
  const { busy, run } = useBusy();
  return html`
    <${SettingsRow} label=${label} description=${description}>
      <button type="button" class=${cls + ' btn-sm'} disabled=${busy} onClick=${() => run(onRun)}>
        ${btnLabel}
      </button>
    <//>
  `;
}

function _showRestartOverlay() {
  const overlay = document.createElement('div');
  overlay.id = 'restart-overlay';
  overlay.className = [
    'fixed inset-0 z-top flex flex-col items-center justify-center gap-4',
    'bg-bg/90 backdrop-blur-sm',
  ].join(' ');
  overlay.innerHTML = `
    <div class="icon-2xl text-accent">${iconSpinner}</div>
    <p class="text-lg font-semibold text-text">${t('settings.server.restart_overlay.title')}</p>
    <p class="text-sm text-text-muted">${t('settings.server.restart_overlay.desc')}</p>
  `;
  document.body.appendChild(overlay);

  const poll = setInterval(async () => {
    try {
      const res = await fetch('/health');
      if (res.ok) {
        clearInterval(poll);
        window.location.reload();
      }
    } catch {
    }
  }, 2000);

  window.addEventListener(
    'kani:server-restart',
    () => {
      clearInterval(poll);
      window.location.reload();
    },
    { once: true },
  );
}

export function ServerSection() {
  const [dangerBusy, setDangerBusy] = useState(false);

  const doRestart = async () => {
    if (
      !(await showConfirm(t('settings.server.danger.restart.confirm'), {
        title: t('settings.server.danger.restart.label'),
        confirmLabel: t('settings.server.danger.restart.btn'),
        danger: true,
      }))
    )
      return;
    setDangerBusy(true);
    try {
      await api.serverRestart();
      _showRestartOverlay();
    } catch (/** @type {any} */ e) {
      showToast(e?.hint ?? e?.message ?? t('settings.server.danger.restart.failed'), { type: 'error' });
      setDangerBusy(false);
    }
  };

  const doStop = async () => {
    if (
      !(await showConfirm(t('settings.server.danger.stop.confirm'), {
        title: t('settings.server.danger.stop.label'),
        confirmLabel: t('settings.server.danger.stop.btn'),
        danger: true,
      }))
    )
      return;
    setDangerBusy(true);
    try {
      await api.serverStop();
      showToast(t('settings.server.danger.stop.stopping'), { type: 'info', duration: 8000 });
    } catch (/** @type {any} */ e) {
      showToast(e?.hint ?? e?.message ?? t('settings.server.danger.stop.failed'), { type: 'error' });
      setDangerBusy(false);
    }
  };

  return html`
    <${SettingsGroup} label=${t('settings.server.admin.group')}>
      <${ActionRow}
        label=${t('settings.server.admin.cancel_downloads.label')}
        description=${t('settings.server.admin.cancel_downloads.desc')}
        btnLabel=${t('settings.server.admin.cancel_downloads.btn')}
        cls="btn-ghost"
        onRun=${async () => {
          if (
            !(await showConfirm(t('settings.server.admin.cancel_downloads.confirm'), {
              title: t('settings.server.admin.cancel_downloads.label'),
              confirmLabel: t('settings.server.admin.cancel_downloads.btn'),
              danger: true,
            }))
          )
            return;
          try {
            await api.cancelAllGlobalDownloads();
            showToast(t('settings.server.admin.cancel_downloads.done'), { type: 'info' });
          } catch (e) {
            showApiError(e);
          }
        }}
      />
      <${ActionRow}
        label=${t('settings.server.admin.stop_scan.label')}
        description=${t('settings.server.admin.stop_scan.desc')}
        btnLabel=${t('settings.server.admin.stop_scan.btn')}
        cls="btn-ghost"
        onRun=${async () => {
          if (
            !(await showConfirm(t('settings.server.admin.stop_scan.confirm'), {
              title: t('settings.server.admin.stop_scan.btn'),
              confirmLabel: t('settings.server.admin.stop_scan.btn'),
              danger: true,
            }))
          )
            return;
          try {
            await api.stopScan();
            showToast(t('settings.server.admin.stop_scan.done'), { type: 'info' });
          } catch (e) {
            showApiError(e);
          }
        }}
      />
      <${ActionRow}
        label=${t('settings.server.admin.clear_cache.label')}
        description=${t('settings.server.admin.clear_cache.desc')}
        btnLabel=${t('settings.server.admin.clear_cache.btn')}
        cls="btn-ghost"
        onRun=${async () => {
          try {
            await api.clearCache();
            showToast(t('settings.server.admin.clear_cache.done'), { type: 'success' });
          } catch (e) {
            showApiError(e);
          }
        }}
      />
    <//>

    <${SettingsGroup} label=${t('settings.server.danger.group')} cardClass="border border-danger/20">
      <${SettingsRow}
        label=${t('settings.server.danger.restart.label')}
        description=${t('settings.server.danger.restart.desc')}
      >
        <button type="button" class="btn-primary btn-sm" disabled=${dangerBusy} onClick=${doRestart}>
          ${t('settings.server.danger.restart.btn')}
        </button>
      <//>
      <${SettingsRow}
        label=${t('settings.server.danger.stop.label')}
        description=${t('settings.server.danger.stop.desc')}
      >
        <button type="button" class="btn-danger btn-sm" disabled=${dangerBusy} onClick=${doStop}>
          ${t('settings.server.danger.stop.btn')}
        </button>
      <//>
    <//>
  `;
}
