// @ts-check

import { h } from 'preact';
import { useState, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { showToast, showApiError } from '../../components/toast.js';
import { SettingsGroup, SettingsRow, ToggleRow, SelectRow } from './_shared.js';
import { useSettingsForm } from './form-bus.js';
import { useBusy } from '../../hooks/use-busy.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/** @param {{ settings: any }} props */
export function EmailSection({ settings }) {
  let cfg = {};
  try {
    cfg = JSON.parse(settings?.email_provider_config ?? '{}');
  } catch {
    cfg = {};
  }

  const initial = {
    email_enabled: settings?.email_enabled ?? false,
    email_from_address: settings?.email_from_address ?? '',
    app_url: settings?.app_url ?? '',
    password_reset_enabled: settings?.password_reset_enabled ?? true,
    email_verification_required: settings?.email_verification_required ?? false,
    host: cfg.host ?? '',
    port: Number(cfg.port ?? 587),
    tls_mode: cfg.tls_mode ?? 'starttls',
    username: cfg.username ?? '',
    password: cfg.password ?? '',
  };

  const [form, setForm] = useState(initial);
  const [saved, setSaved] = useState(initial);
  const set = (/** @type {string} */ k, /** @type {any} */ v) => setForm((f) => ({ ...f, [k]: v }));

  const save = useCallback(async () => {
    const providerConfig = JSON.stringify({
      host: form.host.trim(),
      port: Number(form.port) || 587,
      tls_mode: form.tls_mode,
      username: form.username,
      password: form.password,
    });
    await api.updateSettings({
      Email: {
        email_enabled: form.email_enabled,
        email_provider: 'smtp',
        email_provider_config: providerConfig,
        email_from_address: form.email_from_address.trim(),
        app_url: form.app_url.trim(),
        password_reset_enabled: form.password_reset_enabled,
        email_verification_required: form.email_verification_required,
      },
    });
    setSaved(form);
    showToast(t('common.saved'), { type: 'success' });
  }, [form]);

  useSettingsForm({ current: form, saved, save, reset: () => setForm(saved) });

  const [testTo, setTestTo] = useState('');
  const { busy: testing, run: runTest } = useBusy();
  const sendTest = () =>
    runTest(async () => {
      const to = testTo.trim();
      if (!to) {
        showToast(t('settings.email.test.no_recipient'), { type: 'error' });
        return;
      }
      try {
        await api.sendTestEmail(to);
        showToast(t('settings.email.test.success'), { type: 'success' });
      } catch (e) {
        showApiError(e);
      }
    });

  const input = (/** @type {string} */ type, /** @type {string} */ key, /** @type {string} */ placeholder, /** @type {string} */ cls, /** @type {string} */ [autocomplete] = []) => html`
    <input
      type=${type}
      class=${cls}
      placeholder=${placeholder}
      autocomplete=${autocomplete}
      value=${form[key]}
      onInput=${(/** @type {Event} */ e) => set(key, /** @type {HTMLInputElement} */ (e.target).value)}
    />
  `;

  return html`
    <${SettingsGroup} label=${t('settings.email.general.group')}>
      <${ToggleRow}
        label=${t('settings.email.general.enable.label')}
        description=${t('settings.email.general.enable.desc')}
        checked=${form.email_enabled}
        onChange=${(v) => set('email_enabled', v)}
      />
      <${SettingsRow}
        label=${t('settings.email.general.from.label')}
        description=${t('settings.email.general.from.desc')}
      >
        ${input('email', 'email_from_address', 'noreply@example.com', 'input w-56 text-sm')}
      <//>
      <${SettingsRow}
        label=${t('settings.email.general.app_url.label')}
        description=${t('settings.email.general.app_url.desc')}
      >
        ${input('url', 'app_url', 'https://kani.example.com', 'input w-56 text-sm')}
      <//>
    <//>

    <${SettingsGroup} label=${t('settings.email.features.group')}>
      <${ToggleRow}
        label=${t('settings.email.features.reset.label')}
        description=${t('settings.email.features.reset.desc')}
        checked=${form.password_reset_enabled}
        onChange=${(v) => set('password_reset_enabled', v)}
      />
      <${ToggleRow}
        label=${t('settings.email.features.verify.label')}
        description=${t('settings.email.features.verify.desc')}
        checked=${form.email_verification_required}
        onChange=${(v) => set('email_verification_required', v)}
      />
    <//>

    <p class="text-xs text-text-muted px-1">${t('settings.email.smtp.disclaimer')}</p>
    <${SettingsGroup} label=${t('settings.email.smtp.group')}>
      <${SettingsRow}
        label=${t('settings.email.smtp.host.label')}
        description=${t('settings.email.smtp.host.desc')}
      >
        ${input('text', 'host', 'smtp.example.com', 'input w-56 text-sm')}
      <//>
      <${SettingsRow} label=${t('settings.email.smtp.port.label')}>
        <input
          type="number"
          class="input w-24 text-sm"
          aria-label=${t('settings.email.smtp.port.label')}
          min="1"
          max="65535"
          value=${String(form.port)}
          onInput=${(e) => set('port', Number(e.target.value))}
        />
      <//>
      <${SelectRow}
        label=${t('settings.email.smtp.tls.label')}
        value=${form.tls_mode}
        onChange=${(v) => set('tls_mode', v)}
        options=${[
          { value: 'starttls', label: 'STARTTLS' },
          { value: 'tls', label: 'TLS' },
          { value: 'none', label: 'None' },
        ]}
      />
      <${SettingsRow} label=${t('settings.email.smtp.username.label')}>
        ${input('text', 'username', 'username (optional)', 'input w-56 text-sm', ['off'])}
      <//>
      <${SettingsRow} label=${t('settings.email.smtp.password.label')}>
        ${input('password', 'password', 'password (optional)', 'input w-56 text-sm', ['new-password'])}
      <//>
    <//>

    <${SettingsGroup}>
      <${SettingsRow}
        label=${t('settings.email.test.label')}
        description=${t('settings.email.test.desc')}
      >
        <div class="flex items-center gap-2">
          <input
            type="email"
            class="input w-40 text-sm"
            placeholder="recipient@example.com"
            value=${testTo}
            onInput=${(e) => setTestTo(e.target.value)}
          />
          <button type="button" class="btn-ghost btn-sm" disabled=${testing} onClick=${sendTest}>
            ${testing ? t('common.sending') : t('settings.email.test.btn')}
          </button>
        </div>
      <//>
    <//>
  `;
}
