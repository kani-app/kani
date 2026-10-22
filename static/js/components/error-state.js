// @ts-check
import { h, render } from 'preact';
import htm from 'htm';
import { t } from '../i18n.js';

const html = htm.bind(h);

export function ErrorState({ message, onRetry }) {
  const msg = message ?? t('common.something_wrong');
  return html`
    <div class="flex flex-col items-center gap-4 py-8 text-center">
      <p class="text-sm text-danger">${msg}</p>
      ${onRetry && html`<button type="button" class="btn-ghost" onClick=${onRetry}>${t('common.retry')}</button>`}
    </div>
  `;
}

export function createErrorState({ message, onRetry } = {}) {
  const el = document.createElement('div');
  render(html`<${ErrorState} message=${message} onRetry=${onRetry} />`, el);
  return el;
}
