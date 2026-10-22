// @ts-check
import { h } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../../api.js';
import { DiagCard, registerDiagnosticsCard } from '../diagnostics-cards.js';
import { StatusDot } from '../../../components/status-dot.js';
import { EmptyState } from '../../../components/empty-state.js';
import { showConfirm } from '../../../components/modal.js';
import { showApiError, showToast } from '../../../components/toast.js';
import { useBusy } from '../../../hooks/use-busy.js';
import { t } from '../../../i18n.js';

const html = htm.bind(h);

function CircuitsCard({ refreshToken }) {
  const [state, setState] = useState({ rows: null, error: null });
  const [nonce, setNonce] = useState(0);
  const { busy, run } = useBusy();

  useEffect(() => {
    let cancelled = false;
    api
      .getSourceCircuits()
      .then(rows => {
        if (!cancelled) setState({ rows: Array.isArray(rows) ? rows : [], error: null });
      })
      .catch(e => {
        if (!cancelled) setState({ rows: null, error: e });
      });
    return () => {
      cancelled = true;
    };
  }, [refreshToken, nonce]);

  const reset = async host => {
    const ok = await showConfirm(t('diag.circuits.reset_confirm', { host }), {
      title: t('diag.circuits.reset_title'),
      confirmLabel: t('diag.circuits.reset'),
    });
    if (!ok) return;
    try {
      await run(() => api.resetSourceCircuit(host));
      showToast(t('diag.circuits.reset_done', { host }), { type: 'success' });
      setNonce(n => n + 1);
    } catch (e) {
      showApiError(e);
    }
  };

  const rows = state.rows;
  return html`
    <${DiagCard}
      titleKey="diag.circuits.title"
      span=${2}
      loading=${!rows && !state.error}
      error=${state.error}
      onRetry=${() => setNonce(n => n + 1)}
    >
      ${rows && rows.length === 0
        ? html`<${EmptyState} title=${t('diag.circuits.empty')} />`
        : rows &&
          html`<div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="text-left text-text-muted">
                  <th class="py-1 font-medium">${t('diag.circuits.host')}</th>
                  <th class="py-1 font-medium">${t('diag.circuits.state')}</th>
                  <th class="py-1 font-medium">${t('diag.circuits.failures')}</th>
                  <th class="py-1"></th>
                </tr>
              </thead>
              <tbody>
                ${rows.map(
                  r => html`
                    <tr key=${r.host} class="border-t border-border-subtle">
                      <td class="py-2 font-mono text-xs">${r.host}</td>
                      <td class="py-2">
                        <${StatusDot}
                          state=${r.is_open ? 'open' : 'closed'}
                          label=${r.is_open ? t('diag.circuits.open') : t('diag.circuits.closed')}
                        />
                      </td>
                      <td class="py-2">${r.consecutive_failures ?? 0}</td>
                      <td class="py-2 text-right">
                        <button
                          type="button"
                          class="btn-secondary btn-sm"
                          disabled=${busy}
                          onClick=${() => reset(r.host)}
                        >
                          ${t('diag.circuits.reset')}
                        </button>
                      </td>
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
  id: 'circuits',
  titleKey: 'diag.circuits.title',
  order: 60,
  span: 2,
  Component: CircuitsCard,
});
