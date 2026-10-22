// @ts-check

import { h } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { SettingsGroup } from './_shared.js';
import { hasPermission } from '../../session.js';
import { useBusy } from '../../hooks/use-busy.js';
import { showToast, showApiError } from '../../components/toast.js';
import { fmtCompactDate, errorCountAriaLabel } from '../../utils.js';
import { EmptyState } from '../../components/empty-state.js';
import { ErrorState } from '../../components/error-state.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

function CountBadge({ errors }) {
  if (errors >= 3) {
    return html`<span
      class="text-xs font-semibold px-1.5 py-0.5 rounded bg-danger/20 text-danger"
      aria-label=${errorCountAriaLabel(errors)}
      >${errors}</span
    >`;
  }
  if (errors > 0) {
    return html`<span
      class="text-xs font-semibold px-1.5 py-0.5 rounded bg-warn/20 text-warn"
      aria-label=${errorCountAriaLabel(errors)}
      >${errors}</span
    >`;
  }
  return html`<span class="text-text-muted">0</span>`;
}

/**
 * Reload control for a source that keeps failing. Viewing health only needs
 * `source:browse`, but reloading needs `source:install` — so a browse-only user
 * must not be shown a button that would 403.
 */
function ReloadButton({ sourceId, onReloaded }) {
  const { busy, withBusy } = useBusy();
  const reload = () =>
    withBusy(async () => {
      try {
        await api.reloadSource(sourceId);
        showToast(t('settings.health.reload.done'), { type: 'success' });
        onReloaded();
      } catch (e) {
        showApiError(e);
      }
    });
  return html`<button type="button" class="btn-secondary btn-sm" disabled=${busy} onClick=${reload}>
    ${busy ? t('settings.health.reloading') : t('settings.health.reload')}
  </button>`;
}

export function SourcesHealthSection() {
  const [state, setState] = useState(
    /** @type {{ status: string, rows: any[] }} */ ({ status: 'loading', rows: [] }),
  );
  const [reloadKey, setReloadKey] = useState(0);
  const canReload = hasPermission('source:install');

  useEffect(() => {
    api
      .getSourcesHealth()
      .then((rows) => setState({ status: 'ready', rows: Array.isArray(rows) ? rows : [] }))
      .catch(() => setState({ status: 'error', rows: [] }));
  }, [reloadKey]);

  return html`
    <${SettingsGroup} label=${t('settings.health.group')}>
      ${state.status === 'loading'
        ? html`<div class="px-4 py-3 text-sm text-text-muted">${t('common.loading')}</div>`
        : state.status === 'error'
        ? html`<${ErrorState} message=${t('settings.health.error')} />`
        : state.rows.length === 0
        ? html`<${EmptyState}
            title=${t('settings.health.empty.title')}
            subtitle=${t('settings.health.empty.desc')}
          />`
        : html`
            <div class="overflow-x-auto">
              <table class="w-full text-sm">
                <thead>
                  <tr class="border-b border-border-subtle">
                    <th class="text-left text-xs font-medium text-text-muted px-4 py-2">
                      ${t('settings.health.col.source')}
                    </th>
                    <th class="text-left text-xs font-medium text-text-muted px-4 py-2">
                      ${t('settings.health.col.last_success')}
                    </th>
                    <th class="text-left text-xs font-medium text-text-muted px-4 py-2">
                      ${t('settings.health.col.last_error')}
                    </th>
                    <th class="text-right text-xs font-medium text-text-muted px-4 py-2">
                      ${t('settings.health.col.errors')}
                    </th>
                    <th class="text-right text-xs font-medium text-text-muted px-4 py-2">
                      ${t('settings.health.col.avg_ms')}
                    </th>
                    <th class="px-4 py-2"><span class="sr-only">${t('settings.health.col.actions')}</span></th>
                  </tr>
                </thead>
                <tbody class="divide-y divide-border-subtle">
                  ${state.rows.map((row) => {
                    const errors = row.consecutive_error_count ?? 0;
                    return html`
                      <tr key=${row.source_name} class=${errors >= 3 ? 'bg-danger/5' : ''}>
                        <td class="px-4 py-2.5 font-medium text-text">${row.source_name}</td>
                        <td class="px-4 py-2.5 text-text-muted">
                          ${row.last_success_at ? fmtCompactDate(row.last_success_at) : '—'}
                        </td>
                        <td class="px-4 py-2.5 text-text-muted">
                          ${row.last_error_at ? fmtCompactDate(row.last_error_at) : '—'}
                        </td>
                        <td class="px-4 py-2.5 text-right tabular-nums">
                          <${CountBadge} errors=${errors} />
                        </td>
                        <td class="px-4 py-2.5 text-right tabular-nums text-text-muted">
                          ${row.avg_response_ms != null ? `${Math.round(row.avg_response_ms)}` : '—'}
                        </td>
                        <td class="px-4 py-2.5 text-right">
                          ${errors >= 3 && canReload
                            ? html`<${ReloadButton}
                                sourceId=${row.source_id}
                                onReloaded=${() => setReloadKey((k) => k + 1)}
                              />`
                            : null}
                        </td>
                      </tr>
                    `;
                  })}
                </tbody>
              </table>
            </div>
          `}
    <//>
  `;
}
