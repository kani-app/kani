// @ts-check
import { h, render } from 'preact';
import htm from 'htm';
import { Icon } from '../icon.js';
import { iconSpinner } from '../../icons.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

function Spinner() {
  return html`<span class="icon-2xl text-accent"><${Icon} svg=${iconSpinner} /></span>`;
}

/** @param {{ progressText: string, onCancel: () => void }} props */
function Loading({ progressText, onCancel }) {
  return html`
    <div class="flex flex-col items-center justify-center gap-4 min-h-full text-center px-6">
      <${Spinner} />
      <p class="text-sm text-text">${t('reader.dl.loading')}</p>
      <p class="text-xs text-text-muted">${progressText}</p>
      <button class="btn-ghost btn-sm" onClick=${onCancel}>${t('common.cancel')}</button>
    </div>`;
}

/** @param {{ status: string, onRetry: () => void, onBack: () => void }} props */
function Failed({ status, onRetry, onBack }) {
  return html`
    <div class="flex flex-col items-center justify-center gap-4 min-h-full text-center px-6">
      <p class="text-sm text-danger">${t('reader.dl.status', { status })}</p>
      <button class="btn-ghost btn-sm" onClick=${onRetry}>${t('common.retry')}</button>
      <button class="btn-ghost btn-sm" onClick=${onBack}>${t('common.cancel')}</button>
    </div>`;
}

/** @param {{ totalPages: number, completedPages: number } | null | undefined} p */
function progressText(p) {
  return p && p.totalPages > 0
    ? t('reader.dl.progress', { completed: p.completedPages, total: p.totalPages })
    : '';
}

/**
 * @param {HTMLElement} container
 * @param {HTMLElement} readerRoot
 */
export function createDownloadOverlay(container, readerRoot) {
  /** @param {import('preact').VNode} vnode */
  function _show(vnode) {
    container.style.backgroundColor = readerRoot.style.backgroundColor || '';
    container.classList.remove('hidden');
    render(vnode, container);
  }

  /** @param {{ progress: any, onCancel: () => void }} opts */
  function showLoading({ progress, onCancel }) {
    _show(h(Loading, { progressText: progressText(progress), onCancel }));
  }

  /** @param {{ status: string, onRetry: () => void, onBack: () => void }} opts */
  function showError({ status, onRetry, onBack }) {
    _show(h(Failed, { status, onRetry, onBack }));
  }

  function hide() {
    render(null, container);
    container.classList.add('hidden');
  }

  return { showLoading, showError, hide };
}
