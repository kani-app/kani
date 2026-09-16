// @ts-check
// Login page — username/password form, submits as JSON to /rest/auth/login.

import { h, render } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import { navigate } from '../router.js';
import htm from 'htm';
import { getPasswordResetEnabled, getRegistrationEnabled } from '../api.js';
import { AuthCard, AuthError, AuthField } from '../components/auth-card.js';
import { useBusy } from '../hooks/use-busy.js';
import { t } from '../i18n.js';
const html = htm.bind(h);

function LoginPage() {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [canRegister, setCanRegister] = useState(false);
  const [canReset, setCanReset] = useState(false);
  const { busy, run } = useBusy();

  useEffect(() => {
    // Every signed-out entry point lands here, including the very first visit to
    // a brand-new server. There is no account to sign in with yet, so forward to
    // the screen that creates one.
    fetch('/rest/auth/setup-state', { credentials: 'include' })
      .then(r => r.json())
      .then(d => { if (d?.needs_setup && d?.allowed_from_here) navigate('/setup'); })
      .catch(() => {});
    getRegistrationEnabled().then(d => setCanRegister(!!d?.enabled)).catch(() => {});
    getPasswordResetEnabled().then(d => setCanReset(!!d?.enabled)).catch(() => {});
  }, []);

  const submit = (/** @type {Event} */ e) => {
    e.preventDefault();
    run(async () => {
      setError('');
      const ctrl = new AbortController();
      const timer = setTimeout(() => ctrl.abort(new DOMException('Request timed out', 'TimeoutError')), 15_000);
      try {
        const res = await fetch('/rest/auth/login', {
          method: 'POST',
          credentials: 'include',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ username, password }),
          signal: ctrl.signal,
        });
        if (res.ok) {
          window.location.href = '/';
          return;
        }
        const data = await res.json().catch(() => ({}));
        setError(res.status === 401
          ? t('auth.login.error.invalid')
          : (data.error ?? t('auth.login.error.unknown')));
      } catch (/** @type {any} */ err) {
        setError(err?.name === 'TimeoutError'
          ? t('auth.error.server_slow')
          : t('auth.error.network'));
      } finally {
        clearTimeout(timer);
      }
    });
  };

  return html`
    <${AuthCard} title="Kani" subtitle=${t('auth.login.subtitle')}>
      <${AuthError} message=${error} id="login-error" />
      <form class="flex flex-col gap-4" novalidate onSubmit=${submit}>
        <${AuthField}
          id="login-username"
          label=${t('auth.login.username')}
          value=${username}
          onInput=${setUsername}
          autocomplete="username"
          required=${true}
          autofocus=${true}
          describedBy="login-error"
        />
        <${AuthField}
          id="login-password"
          label=${t('auth.login.password')}
          type="password"
          value=${password}
          onInput=${setPassword}
          autocomplete="current-password"
          required=${true}
          describedBy="login-error"
        />
        <button type="submit" class="btn-primary w-full h-11 mt-2" disabled=${busy}>
          ${busy ? t('auth.login.submitting') : t('auth.login.submit')}
        </button>
      </form>
      ${canReset && html`
        <p class="text-center text-sm text-text-muted">
          <a href="/forgot-password" class="touch-min-h inline-flex items-center text-text-muted underline hover:text-text">${t('auth.login.forgot_password')}</a>
        </p>
      `}
      ${canRegister && html`
        <p class="text-center text-sm text-text-muted">
          ${t('auth.login.no_account')} <a href="/register" class="text-text-muted underline hover:text-text">${t('auth.login.create')}</a>
        </p>
      `}
    </${AuthCard}>
  `;
}

/** @param {HTMLElement} container */
export function init(container) {
  document.title = t('auth.login.page_title');
  render(html`<${LoginPage} />`, container);
}

/** @param {HTMLElement} container */
export function destroy(container) {
  render(null, container);
  container.innerHTML = '';
}
