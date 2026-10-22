// @ts-check
import { h } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../../api.js';
import { DiagCard, registerDiagnosticsCard } from '../diagnostics-cards.js';
import { EmptyState } from '../../../components/empty-state.js';
import { formatBytes } from '../../../utils.js';
import { t } from '../../../i18n.js';

const html = htm.bind(h);

function BandwidthCard({ refreshToken }) {
  const [state, setState] = useState({ rows: null, error: null });

  useEffect(() => {
    let cancelled = false;
    api
      .getProxyStats()
      .then(rows => {
        if (!cancelled) setState({ rows: Array.isArray(rows) ? rows : [], error: null });
      })
      .catch(e => {
        if (!cancelled) setState({ rows: null, error: e });
      });
    return () => {
      cancelled = true;
    };
  }, [refreshToken]);

  const rows = state.rows;
  return html`
    <${DiagCard}
      titleKey="diag.bandwidth.title"
      span=${2}
      loading=${!rows && !state.error}
      error=${state.error}
    >
      ${rows && rows.length === 0
        ? html`<${EmptyState} title=${t('diag.bandwidth.empty')} compact=${true} />`
        : rows &&
          html`<div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="text-left text-text-muted">
                  <th class="py-1 font-medium">${t('diag.bandwidth.host')}</th>
                  <th class="py-1 font-medium text-right">${t('diag.bandwidth.served')}</th>
                </tr>
              </thead>
              <tbody>
                ${rows.map(
                  r => html`
                    <tr key=${r.host} class="border-t border-border-subtle">
                      <td class="py-2 font-mono text-xs">${r.host}</td>
                      <td class="py-2 text-right">${formatBytes(r.bytes)}</td>
                    </tr>
                  `
                )}
              </tbody>
            </table>
          </div>`}
    <//>
  `;
}

registerDiagnosticsCard({
  id: 'bandwidth',
  titleKey: 'diag.bandwidth.title',
  order: 70,
  span: 2,
  Component: BandwidthCard,
});
