// @ts-check

import { h, render } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import { validateResetToken, confirmPasswordReset } from '../api.js';
import { AuthCard, AuthError, AuthField, AuthSuccess } from '../components/auth-card.js';
import { PasswordStrength } from '../components/password-strength.js';
import { useBusy } from '../hooks/use-busy.js';
import { t } from '../i18n.js';
const html = htm.bind(h);

function ResetPasswordPage() {
  const token = new URLSearchParams(location.search).get('token') ?? '';
  const [phase, setPhase] = useState(/** @type {'verifying'|'form'|'invalid'|'success'} */ (token ? 'verifying' : 'invalid'));
  const [subtitle, setSubtitle] = useState(token ? t('auth.reset.verifying') : t('auth.reset.error.invalid_link'));
  const [newPw, setNewPw] = useState('');
  const [confPw, setConfPw] = useState('');
  const [error, setError] = useState('');
  const { busy, run } = useBusy();

  useEffect(() => {
    if (!token) return;
    validateResetToken(token)
      .then(data => {
        setSubtitle(t('auth.reset.for_email', { email: data.email_hint }));
        setPhase('form');
      })
      .catch(() => {
        setSubtitle(t('auth.reset.error.expired'));
        setPhase('invalid');
      });
  }, [token]);

  const submit = (/** @type {Event} */ e) => {
    e.preventDefault();
    run(async () => {
      setError('');
      if (newPw.length < 8) {
        setError(t('auth.reset.error.too_short'));
        return;
      }
      if (newPw !== confPw) {
        setError(t('auth.reset.error.mismatch'));
        return;
      }
      try {
        await confirmPasswordReset(token, newPw);
        setPhase('success');
      } catch (/** @type {any} */ err) {
        setError(err?.message ?? t('auth.reset.error.failed'));
      }
    });
  };

  return html`
    <${AuthCard} title=${t('auth.reset.title')} subtitle=${subtitle}>
      <${AuthError} message=${error} id="rp-error" />
      ${phase === 'success' && html`
        <${AuthSuccess}>
          ${t('auth.reset.success')} <a href="/login" class="underline">${t('auth.reset.success.signin')}</a>
        </${AuthSuccess}>
      `}
      ${phase === 'form' && html`
        <form class="flex flex-col gap-4" novalidate onSubmit=${submit}>
          <div>
            <${AuthField}
              id="rp-new-pw"
              label=${t('auth.reset.new_password')}
              type="password"
              value=${newPw}
              onInput=${setNewPw}
              autocomplete="new-password"
              required=${true}
              autofocus=${true}
            />
            <${PasswordStrength} password=${newPw} />
          </div>
          <${AuthField}
            id="rp-conf-pw"
            label=${t('auth.reset.confirm_password')}
            type="password"
            value=${confPw}
            onInput=${setConfPw}
            autocomplete="new-password"
            required=${true}
          />
          <button type="submit" class="btn-primary w-full h-11 mt-2" disabled=${busy}>
            ${busy ? t('common.saving') : t('auth.reset.submit')}
          </button>
        </form>
      `}
      ${phase === 'invalid' && html`
        <p class="text-center text-sm">
          <a href="/forgot-password" class="touch-min-h inline-flex items-center text-text-muted underline hover:text-text">${t('auth.reset.request_link')}</a>
        </p>
      `}
      <p class="text-center text-sm text-text-muted">
        <a href="/login" class="touch-min-h inline-flex items-center text-text-muted underline hover:text-text">${t('auth.reset.back')}</a>
      </p>
    </${AuthCard}>
  `;
}

/** @param {HTMLElement} container */
export function init(container) {
  document.title = t('auth.reset.page_title');
  render(html`<${ResetPasswordPage} />`, container);
}

/** @param {HTMLElement} container */
export function destroy(container) {
  render(null, container);
  container.innerHTML = '';
}
