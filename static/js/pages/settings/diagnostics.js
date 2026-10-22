// @ts-check

import { h } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import htm from 'htm';
import { getDiagnosticsCards } from './diagnostics-cards.js';
import { hasPermission } from '../../session.js';
import { useBusy } from '../../hooks/use-busy.js';
import { showApiError } from '../../components/toast.js';
import * as api from '../../api.js';
import { t } from '../../i18n.js';

import './diagnostics/card-degraded.js';
import './diagnostics/card-system.js';
import './diagnostics/card-database.js';
import './diagnostics/card-jobs.js';
import './diagnostics/card-storage.js';
import './diagnostics/card-errors.js';
import './diagnostics/card-browser.js';
import './diagnostics/card-circuits.js';
import './diagnostics/card-bandwidth.js';

const html = htm.bind(h);

const REFRESH_INTERVAL_MS = 30_000;

export function DiagnosticsSection() {
  const [refreshToken, setRefreshToken] = useState(0);
  const { busy, run } = useBusy();

  useEffect(() => {
    const id = setInterval(() => setRefreshToken(n => n + 1), REFRESH_INTERVAL_MS);
    return () => clearInterval(id);
  }, []);

  const cards = getDiagnosticsCards().filter(c => !c.perm || hasPermission(c.perm));

  const downloadBundle = async () => {
    try {
      await run(() => api.downloadSupportBundle());
    } catch (e) {
      showApiError(e);
    }
  };

  return html`
    <div class="flex flex-col gap-4">
      <div class="flex justify-end">
        <button
          type="button"
          class="btn-secondary btn-sm"
          onClick=${() => setRefreshToken(n => n + 1)}
        >
          ${t('diag.refresh')}
        </button>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
        ${cards.map(
          card =>
            html`<${card.Component}
              key=${card.id}
              refreshToken=${refreshToken}
              span=${card.span}
            />`
        )}
      </div>

      <div class="flex flex-col gap-2 pt-2">
        <p class="text-sm text-text-muted">${t('diag.bundle.desc')}</p>
        <div>
          <button type="button" class="btn-secondary" disabled=${busy} onClick=${downloadBundle}>
            ${busy ? t('diag.bundle.preparing') : t('diag.bundle.download')}
          </button>
        </div>
      </div>
    </div>
  `;
}
