// @ts-check
import { h, render } from 'preact';
import { useState } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { t } from '../i18n.js';

const html = htm.bind(h);

const DISMISS_PREFIX = 'kani-update-dismissed:';

/** @param {{ latest: string, url: string, onDismiss: () => void }} props */
function UpdateBanner({ latest, url, onDismiss }) {
  const [hidden, setHidden] = useState(false);
  if (hidden) return null;

  const dismiss = () => {
    try {
      sessionStorage.setItem(DISMISS_PREFIX + latest, '1');
    } catch {
      /* storage unavailable — dismissal is best-effort */
    }
    setHidden(true);
    onDismiss();
  };

  return html`
    <div
      class="flex items-center gap-3 px-4 py-2 border-b border-warn/30 bg-warn/10 text-sm"
      role="status"
    >
      <span class="text-warn font-semibold shrink-0">${t('update_banner.label')}</span>
      <span class="text-text flex-1">${t('update_banner.message', { version: latest })}</span>
      <a href=${url} target="_blank" rel="noopener noreferrer" class="text-accent underline shrink-0">
        ${t('update_banner.action')}
      </a>
      <button
        type="button"
        class="btn-ghost btn-sm shrink-0"
        aria-label=${t('update_banner.dismiss')}
        onClick=${dismiss}
      >
        ${t('update_banner.dismiss')}
      </button>
    </div>
  `;
}

/**
 * Fetches update state and mounts the banner after the app header.
 * @param {HTMLElement} appEl
 */
export async function maybeShowUpdateBanner(appEl) {
  try {
    if (document.getElementById('update-banner')) return;

    const info = await api.getSystemUpdate();
    if (!info?.update_available || !info?.latest) return;

    let dismissed = false;
    try {
      dismissed = sessionStorage.getItem(DISMISS_PREFIX + info.latest) === '1';
    } catch {
      /* storage unavailable */
    }
    if (dismissed) return;

    const mount = document.createElement('div');
    mount.id = 'update-banner';

    const header = appEl.querySelector('header');
    if (header?.nextSibling) {
      appEl.insertBefore(mount, header.nextSibling);
    } else {
      appEl.appendChild(mount);
    }

    render(
      html`<${UpdateBanner}
        latest=${info.latest}
        url=${info.url ?? 'https://github.com/kani-app/kani/releases/latest'}
        onDismiss=${() => {
          render(null, mount);
          mount.remove();
        }}
      />`,
      mount
    );
  } catch {
  }
}
