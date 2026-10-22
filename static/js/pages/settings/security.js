// @ts-check
// Settings — Security tab: 2FA/TOTP, session inventory, security status.

import { h, render } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { SettingsGroup, SettingsRow } from './_shared.js';
import { SessionList } from '../../components/session-list.js';
import { ErrorState } from '../../components/error-state.js';
import { TotpWizard } from '../../components/totp-wizard.js';
import { showConfirm, showAlert } from '../../components/modal.js';
import { showApiError, showToast } from '../../components/toast.js';
import { useBusy } from '../../hooks/use-busy.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

function _showSetupTotpModal(onDone) {
  const root = document.getElementById('modal-root');
  if (!root) return;
  const cleanup = () => render(null, root);
  render(
    html`<${TotpWizard}
      onComplete=${() => {
        cleanup();
        onDone(true);
      }}
      onCancel=${() => cleanup()}
    />`,
    root,
  );
}

async function _showDisableTotpModal(onDone) {
  const code = prompt(t('settings.security.totp.disable_prompt'));
  if (!code) return;
  try {
    await api.disableTotp(code);
    onDone(false);
  } catch (/** @type {any} */ e) {
    alert(t('settings.security.totp.disable_error', { msg: e?.message ?? '' }));
  }
}

/**
 * Issues a fresh set of backup codes.
 *
 * Re-authenticates first: `/auth/totp/step-up` accepts either a TOTP code or an
 * existing backup code, and without it a stolen session could silently mint
 * itself a new set. Both endpoints existed and had no caller, which meant a user
 * who lost their codes had no recovery path at all.
 */
function BackupCodesRow() {
  const { busy, run } = useBusy();
  const [enabled, setEnabled] = useState(false);

  useEffect(() => {
    api.getFeatures().then((f) => setEnabled(!!f?.totp_enabled)).catch(() => {});
  }, []);

  const regenerate = () =>
    run(async () => {
      const ok = await showConfirm(t('settings.security.backup_codes.confirm'), {
        title: t('settings.security.backup_codes.regenerate'),
        confirmLabel: t('settings.security.backup_codes.regenerate'),
      });
      if (!ok) return;

      const code = prompt(t('settings.security.backup_codes.verify_prompt'));
      if (!code) return;

      try {
        await api.stepUpTotp(code.trim());
      } catch (e) {
        showApiError(e);
        return;
      }

      try {
        const res = await api.regenerateBackupCodes();
        const codes = res?.backup_codes ?? [];
        await showAlert(
          t('settings.security.backup_codes.new', { codes: codes.join('\n') }),
          { title: t('settings.security.backup_codes.regenerate') },
        );
        showToast(t('settings.security.backup_codes.done'), { type: 'success' });
      } catch (e) {
        showApiError(e);
      }
    });

  // Only meaningful once 2FA is on.
  if (!enabled) return null;

  return html`
    <${SettingsRow}
      label=${t('settings.security.backup_codes')}
      description=${t('settings.security.backup_codes.desc')}
    >
      <button type="button" class="btn-secondary btn-sm" disabled=${busy} onClick=${regenerate}>
        ${t('settings.security.backup_codes.regenerate')}
      </button>
    <//>
  `;
}

function TotpStatusRow() {
  const [enabled, setEnabled] = useState(/** @type {boolean|null} */ (null));
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api
      .getFeatures()
      .then((f) => {
        setEnabled(!!f?.totp_enabled);
        setLoading(false);
      })
      .catch(() => {
        setEnabled(false);
        setLoading(false);
      });
  }, []);

  if (loading) {
    return html`<div class="px-3 py-3 text-sm text-text-muted">${t('common.loading')}</div>`;
  }

  if (enabled) {
    return html`
      <div class="flex items-center justify-between gap-3 px-3 py-3">
        <div>
          <div class="text-sm font-medium text-text">${t('settings.security.totp.title')}</div>
          <div class="text-xs text-success mt-0.5">${t('settings.security.totp.enabled')}</div>
        </div>
        <button type="button" class="btn-danger btn-sm" onClick=${() => _showDisableTotpModal(setEnabled)}>
          ${t('settings.security.totp.disable')}
        </button>
      </div>
    `;
  }

  return html`
    <div class="flex items-center justify-between gap-3 px-3 py-3">
      <div>
        <div class="text-sm font-medium text-text">${t('settings.security.totp.title')}</div>
        <div class="text-xs text-text-muted mt-0.5">${t('settings.security.totp.desc')}</div>
      </div>
      <button type="button" class="btn-primary btn-sm" onClick=${() => _showSetupTotpModal(setEnabled)}>
        ${t('settings.security.totp.setup')}
      </button>
    </div>
  `;
}

function SecurityStatusGroup() {
  const [state, setState] = useState(
    /** @type {{ status: string, features: any }} */ ({ status: 'loading', features: null }),
  );

  const load = useCallback(async () => {
    setState((s) => ({ ...s, status: 'loading' }));
    try {
      const features = await api.getFeatures();
      setState({ status: 'ready', features });
    } catch {
      setState({ status: 'error', features: null });
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const f = state.features;
  return html`
    <${SettingsGroup} label=${t('settings.security.status.group')}>
      ${state.status === 'loading'
        ? html`<div class="px-3 py-3 text-sm text-text-muted">${t('common.loading')}</div>`
        : state.status === 'error'
        ? html`<${ErrorState} message=${t('settings.security.status.load_failed')} onRetry=${load} />`
        : html`
            ${f?.public_instance &&
            html`<div class="px-3 py-2 rounded bg-accent/10 text-accent text-sm font-medium">
              ${t('settings.security.status.public_instance')}
            </div>`}
            <${SettingsRow}
              label=${t('settings.security.status.https.label')}
              description=${f?.public_instance
                ? t('settings.security.status.https.desc_public')
                : t('settings.security.status.https.desc')}
            />
            <${SettingsRow}
              label=${t('settings.security.status.headers.label')}
              description=${t('settings.security.status.headers.desc')}
            />
          `}
    <//>
  `;
}

export function SecuritySection() {
  return html`
    <${SettingsGroup} label=${t('settings.security.totp.group')}>
      <${TotpStatusRow} />
      <${BackupCodesRow} />
    <//>
    <${SettingsGroup} label=${t('settings.security.sessions.group')}>
      <${SessionList} />
    <//>
    <${SecurityStatusGroup} />
  `;
}
