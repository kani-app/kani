// @ts-check

import { h, render } from 'preact';
import { useState } from 'preact/hooks';
import htm from 'htm';
import { requestPasswordReset } from '../api.js';
import { AuthCard, AuthError, AuthField, AuthSuccess } from '../components/auth-card.js';
import { useBusy } from '../hooks/use-busy.js';
import { t } from '../i18n.js';
const html = htm.bind(h);

function ForgotPasswordPage() {
  const [email, setEmail] = useState('');
  const [error, setError] = useState('');
  const [sent, setSent] = useState(false);
  const { busy, run } = useBusy();

  const submit = (/** @type {Event} */ e) => {
    e.preventDefault();
    run(async () => {
      setError('');
      const trimmed = email.trim();
      if (!trimmed) {
        setError(t('auth.forgot.error.empty_email'));
        return;
      }
      try {
        await requestPasswordReset(trimmed);
        setSent(true);
      } catch {
        setError(t('auth.error.network'));
      }
    });
  };

  return html`
    <${AuthCard} title=${t('auth.forgot.title')} subtitle=${t('auth.forgot.subtitle')}>
      <${AuthError} message=${error} id="fp-error" />
      ${sent
        ? html`<${AuthSuccess}>${t('auth.forgot.success')}</${AuthSuccess}>`
        : html`
          <form class="flex flex-col gap-4" novalidate onSubmit=${submit}>
            <${AuthField}
              id="fp-email"
              label=${t('auth.forgot.email')}
              type="email"
              value=${email}
              onInput=${setEmail}
              autocomplete="email"
              required=${true}
              autofocus=${true}
            />
            <button type="submit" class="btn-primary w-full h-11 mt-2" disabled=${busy}>
              ${busy ? t('auth.forgot.submitting') : t('auth.forgot.submit')}
            </button>
          </form>
        `}
      <p class="text-center text-sm text-text-muted">
        <a href="/login" class="touch-min-h inline-flex items-center text-text-muted underline hover:text-text">${t('auth.forgot.back')}</a>
      </p>
    </${AuthCard}>
  `;
}

/** @param {HTMLElement} container */
export function init(container) {
  document.title = t('auth.forgot.page_title');
  render(html`<${ForgotPasswordPage} />`, container);
}

/** @param {HTMLElement} container */
export function destroy(container) {
  render(null, container);
  container.innerHTML = '';
}
