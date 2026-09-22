// @ts-check
// Email verification page — automatically verifies the token from the URL.

import { h, render } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import { verifyEmail, resendVerification } from '../api.js';
import { AuthCard } from '../components/auth-card.js';
import { useBusy } from '../hooks/use-busy.js';
import { t } from '../i18n.js';
const html = htm.bind(h);

function VerifyEmailPage() {
  const token = new URLSearchParams(location.search).get('token') ?? '';
  const [status, setStatus] = useState(token ? t('auth.verify.verifying') : t('auth.verify.error.invalid'));
  const [showResend, setShowResend] = useState(!token);
  const [resendMsg, setResendMsg] = useState('');
  const { busy, run } = useBusy();

  useEffect(() => {
    if (!token) return;
    verifyEmail(token)
      .then(() => setStatus(t('auth.verify.success')))
      .catch((/** @type {any} */ err) => {
        setStatus(err?.message ?? t('auth.verify.error.failed'));
        setShowResend(true);
      });
  }, [token]);

  const resend = () => run(async () => {
    try {
      await resendVerification();
      setResendMsg(t('auth.verify.resend.success'));
    } catch {
      setResendMsg(t('auth.verify.resend.failed'));
    }
  });

  return html`
    <${AuthCard} title=${t('auth.verify.title')} subtitle=${status} center=${true}>
      ${showResend && html`
        <div class="flex flex-col gap-3">
          <button type="button" class="btn-primary w-full h-11" disabled=${busy} onClick=${resend}>
            ${busy ? t('auth.verify.resend.sending') : t('auth.verify.resend')}
          </button>
          ${resendMsg && html`<p class="text-xs text-text-muted" role="status">${resendMsg}</p>`}
        </div>
      `}
      <a href="/" class="touch-min-h inline-flex items-center text-sm text-text-muted underline hover:text-text">${t('auth.verify.go_library')}</a>
    </${AuthCard}>
  `;
}

/** @param {HTMLElement} container */
export function init(container) {
  document.title = t('auth.verify.page_title');
  render(html`<${VerifyEmailPage} />`, container);
}

/** @param {HTMLElement} container */
export function destroy(container) {
  render(null, container);
  container.innerHTML = '';
}
