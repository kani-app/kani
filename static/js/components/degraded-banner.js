// @ts-check
import { h, render } from 'preact';
import { useState } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { hasPermission } from '../session.js';
import { navigate } from '../router.js';
import { t } from '../i18n.js';

const html = htm.bind(h);

const DISMISS_KEY = 'kani-degraded-dismissed';

/** @param {{ count: number, onDismiss: () => void }} props */
function DegradedBanner({ count, onDismiss }) {
  const [hidden, setHidden] = useState(false);
  if (hidden) return null;

  const dismiss = () => {
    try {
      sessionStorage.setItem(DISMISS_KEY, '1');
    } catch {
      /* storage unavailable — dismissal is best-effort */
    }
    setHidden(true);
    onDismiss();
  };

  // Stacked below sm. As one row the message wrapped to about two words a
  // line and the dismiss button laid out past the right edge of a 320px
  // screen, with no horizontal scroll to reach it.
  return html`
    <div
      class="flex flex-col gap-2 px-4 py-2 border-b border-danger/30 bg-danger/10 text-sm sm:flex-row sm:items-center sm:gap-3"
      role="status"
    >
      <p class="min-w-0">
        <span class="text-danger font-semibold">${t('degraded_banner.label')}</span>${' '}
        <span class="text-text">${t('degraded_banner.message', { count })}</span>
      </p>
      <div class="flex items-center gap-3 shrink-0 sm:ml-auto">
        <button
          type="button"
          class="touch-min-h inline-flex items-center text-accent underline"
          onClick=${() => {
            dismiss();
            navigate('/settings?section=diagnostics');
          }}
        >
          ${t('degraded_banner.action')}
        </button>
        <button
          type="button"
          class="btn-ghost btn-sm"
          aria-label=${t('degraded_banner.dismiss')}
          onClick=${dismiss}
        >
          ${t('degraded_banner.dismiss')}
        </button>
      </div>
    </div>
  `;
}

/**
 * Mounts the banner when a subsystem has failed outright.
 *
 * Only `error` degradations raise it — a `warn` (the module cache being slower,
 * say) belongs in Diagnostics but does not deserve a banner on every page, and a
 * banner that appears for routine things stops being read.
 * @param {HTMLElement} appEl
 */
export async function maybeShowDegradedBanner(appEl) {
  try {
    if (document.getElementById('degraded-banner')) return;
    // Diagnostics is admin-only, so nobody else can act on this or follow the link.
    if (!hasPermission('server:manage')) return;

    try {
      if (sessionStorage.getItem(DISMISS_KEY) === '1') return;
    } catch {
      /* storage unavailable */
    }

    const diag = await api.getDiagnostics();
    const errors = (diag?.degradations ?? []).filter((d) => d.severity === 'error');
    if (errors.length === 0) return;

    const mount = document.createElement('div');
    mount.id = 'degraded-banner';

    const header = appEl.querySelector('header');
    if (header?.nextSibling) {
      appEl.insertBefore(mount, header.nextSibling);
    } else {
      appEl.appendChild(mount);
    }

    render(
      html`<${DegradedBanner} count=${errors.length} onDismiss=${() => mount.remove()} />`,
      mount,
    );
  } catch {
    /* diagnostics unavailable — never block the app for this */
  }
}
