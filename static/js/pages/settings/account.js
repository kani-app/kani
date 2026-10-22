// @ts-check

import { h } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { navigate } from '../../router.js';
import { SettingsGroup, SettingsRow } from './_shared.js';
import { Modal, showConfirm } from '../../components/modal.js';
import { showToast } from '../../components/toast.js';
import { useBusy } from '../../hooks/use-busy.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

function ChangePasswordModal({ open, onClose }) {
  const [cur, setCur] = useState('');
  const [next, setNext] = useState('');
  const [conf, setConf] = useState('');
  const [error, setError] = useState('');
  const { busy, run } = useBusy();

  const save = () =>
    run(async () => {
      if (!cur || !next) {
        setError(t('settings.account.password.modal.error.empty'));
        return;
      }
      if (next !== conf) {
        setError(t('auth.reset.error.mismatch'));
        return;
      }
      setError('');
      try {
        await api.changePassword(cur, next);
        onClose();
        showToast(t('settings.account.password.modal.success'));
      } catch (/** @type {any} */ e) {
        setError(e?.message ?? t('settings.account.password.modal.error.failed'));
      }
    });

  const field = (id, key, autocomplete, value, setter, autoFocus) => html`
    <div class="flex flex-col gap-1.5">
      <label class="text-sm font-medium text-text" for=${id}>${t(key)}</label>
      <input
        type="password"
        id=${id}
        class="input"
        autocomplete=${autocomplete}
        autoFocus=${autoFocus}
        value=${value}
        onInput=${(/** @type {any} */ e) => setter(e.target.value)}
        onKeyDown=${(/** @type {KeyboardEvent} */ e) => e.key === 'Enter' && save()}
      />
    </div>
  `;

  return html`
    <${Modal}
      open=${open}
      title=${t('settings.account.password.modal.title')}
      onClose=${onClose}
      footer=${html`
        <button type="button" class="btn-ghost btn-sm" onClick=${onClose}>${t('common.cancel')}</button>
        <button type="button" class="btn-primary btn-sm" onClick=${save} disabled=${busy}>
          ${t('settings.account.password.modal.title')}
        </button>
      `}
    >
      <div class="flex flex-col gap-4 px-1">
        ${field('modal-cur-pw', 'settings.account.password.modal.current', 'current-password', cur, setCur, true)}
        ${field('modal-new-pw', 'settings.account.password.modal.new', 'new-password', next, setNext, false)}
        ${field('modal-conf-pw', 'settings.account.password.modal.confirm', 'new-password', conf, setConf, false)}
        ${error && html`<p class="text-sm text-danger">${error}</p>`}
      </div>
    <//>
  `;
}

function EmailVerifyRow() {
  const [user, setUser] = useState(/** @type {any} */ (undefined));
  const [sent, setSent] = useState(false);
  const { busy, run } = useBusy();

  useEffect(() => {
    api
      .getCurrentUser()
      .then(setUser)
      .catch(() => setUser(null));
  }, []);

  const resend = () =>
    run(async () => {
      try {
        await api.resendVerification();
        setSent(true);
        setTimeout(() => setSent(false), 3000);
      } catch (/** @type {any} */ e) {
        showToast(e?.message ?? t('settings.account.email.send_failed'), { type: 'error' });
      }
    });

  let status;
  let showResend = false;
  if (user === undefined) {
    status = html`<span class="text-xs text-text-muted">${t('common.loading')}</span>`;
  } else if (user === null) {
    status = html`<span class="text-xs text-text-muted">${t('settings.account.email.load_failed')}</span>`;
  } else if (user.email_verified_at) {
    status = html`<span class="text-xs text-success"
      >${t('settings.account.email.verified_on', {
        date: new Date(user.email_verified_at).toLocaleDateString(),
      })}</span
    >`;
  } else {
    status = html`<span class="text-xs text-text-muted">${t('settings.account.email.not_verified')}</span>`;
    showResend = true;
  }

  return html`
    <${SettingsRow}
      label=${t('settings.account.email.verify.label')}
      description=${t('settings.account.email.verify.desc')}
    >
      <div class="flex items-center gap-2">
        ${status}
        ${showResend &&
        html`<button type="button" class="btn-ghost btn-sm" disabled=${busy} onClick=${resend}>
          ${sent ? t('settings.account.email.sent') : busy ? t('common.sending') : t('settings.account.email.resend')}
        </button>`}
      </div>
    <//>
  `;
}

export function AccountSection() {
  const [pwOpen, setPwOpen] = useState(false);
  const { busy: loggingOut, run: runLogout } = useBusy();

  const logoutAll = () =>
    runLogout(async () => {
      if (
        !(await showConfirm(t('settings.account.sessions.confirm.message'), {
          title: t('settings.account.sessions.signout_all'),
          danger: true,
        }))
      )
        return;
      try {
        await api.logoutEverywhere();
        navigate('/login');
      } catch (/** @type {any} */ e) {
        showToast(e?.message ?? t('settings.account.sessions.signout_failed'), { type: 'error' });
      }
    });

  return html`
    <${SettingsGroup} label=${t('settings.account.password.group')}>
      <${SettingsRow}
        label=${t('settings.account.password.label')}
        description=${t('settings.account.password.desc')}
      >
        <button type="button" class="btn-ghost btn-sm" onClick=${() => setPwOpen(true)}>
          ${t('settings.account.password.btn')}
        </button>
      <//>
    <//>

    <${SettingsGroup} label=${t('settings.account.email.group')}>
      <${EmailVerifyRow} />
    <//>

    <${SettingsGroup} label=${t('settings.account.sessions.group')}>
      <${SettingsRow}
        label=${t('settings.account.sessions.active.label')}
        description=${t('settings.account.sessions.active.desc')}
      >
        <a
          href="/settings?section=security"
          class="btn-ghost btn-sm"
          onClick=${(e) => {
            e.preventDefault();
            navigate('/settings?section=security');
          }}
          >${t('settings.account.sessions.manage')}</a
        >
      <//>
      <${SettingsRow}
        label=${t('settings.account.sessions.signout_all.label')}
        description=${t('settings.account.sessions.signout_all.desc')}
      >
        <button type="button" class="btn-danger btn-sm" disabled=${loggingOut} onClick=${logoutAll}>
          ${t('settings.account.sessions.signout_all')}
        </button>
      <//>
    <//>

    <${ChangePasswordModal} open=${pwOpen} onClose=${() => setPwOpen(false)} />
  `;
}
