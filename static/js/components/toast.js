// @ts-check
// Toast notification — self-dismissing message overlay.

import { t } from '../i18n.js';

/** @type {HTMLElement | null} */
let _container = null;

function _getContainer() {
  if (!_container) {
    _container = document.createElement('div');
    _container.id = 'toast-container';
    _container.className = 'fixed bottom-6 left-1/2 -translate-x-1/2 flex flex-col items-center gap-2 z-toast pointer-events-none';
    _container.setAttribute('aria-live', 'polite');
    _container.setAttribute('aria-atomic', 'false');
    document.body.appendChild(_container);
  }
  return _container;
}

/**
 * Show a toast notification.
 * @param {string} message
 * @param {{ type?: 'info' | 'success' | 'warn' | 'error', duration?: number, action?: { label: string, href?: string, onClick?: () => void } }} [opts]
 */
export function showToast(message, { type = 'info', duration = 3000, action = null } = {}) {
  const container = _getContainer();

  const typeClasses = {
    info:    'bg-surface-3 text-text border border-border',
    success: 'bg-success text-white', // audit-ignore: contrast text on semantic fill (no on-success token)
    warn:    'bg-warn text-black', // audit-ignore: contrast text on semantic fill (no on-warn token)
    error:   'bg-danger text-white', // audit-ignore: contrast text on semantic fill (no on-danger token)
  };

  const toast = document.createElement('div');
  toast.className = [
    'pointer-events-auto px-5 py-3 rounded-md text-sm shadow-md',
    'max-w-sm text-center transition-opacity duration-300',
    typeClasses[type] ?? typeClasses.info,
  ].join(' ');
  toast.setAttribute('role', 'status');
  toast.setAttribute('aria-atomic', 'true');
  toast.textContent = message;

  container.appendChild(toast);

  let timer = setTimeout(() => {
    toast.style.opacity = '0';
    setTimeout(() => toast.remove(), 300);
  }, duration);

  const _dismiss = () => { clearTimeout(timer); toast.style.opacity = '0'; setTimeout(() => toast.remove(), 300); };

  if (action) {
    const el = action.onClick
      ? document.createElement('button')
      : document.createElement('a');
    if (action.onClick) {
      /** @type {HTMLButtonElement} */ (el).type = 'button';
      el.addEventListener('click', (e) => { e.stopPropagation(); _dismiss(); action.onClick(); });
    } else {
      /** @type {HTMLAnchorElement} */ (el).href = action.href ?? '#';
    }
    el.textContent = ' · ' + action.label;
    el.className = 'underline font-semibold';
    toast.appendChild(el);
  }

  toast.addEventListener('click', _dismiss);
}

/**
 * Show an error toast from an API error, preferring the hint over the raw message.
 * @param {any} err
 */
export function showApiError(err) {
  const msg = err?.hint ?? err?.message ?? t('common.error_occurred');
  const traceId = err?.traceId;
  showToast(traceId ? `${msg} — ${t('error.trace_id', { id: traceId })}` : msg, { type: 'error' });
}
