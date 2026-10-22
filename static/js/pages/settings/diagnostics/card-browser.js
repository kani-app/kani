// @ts-check
import { h } from 'preact';
import htm from 'htm';
import { DiagCard, useDiagnostics, registerDiagnosticsCard } from '../diagnostics-cards.js';
import { StatRow } from '../../../components/stat-row.js';
import { t } from '../../../i18n.js';

const html = htm.bind(h);

function BrowserCard({ refreshToken }) {
  const { data, error } = useDiagnostics({ refreshToken });
  const b = data?.browser;
  return html`
    <${DiagCard} titleKey="diag.browser.title" loading=${!data && !error} error=${error}>
      ${b &&
      html`
        <${StatRow}
          label=${t('diag.browser.solver')}
          value=${t(`diag.browser.solver_${b.solver}`)}
        />
        <${StatRow} label=${t('diag.browser.calls')} value=${b.calls_total} />
        <${StatRow} label=${t('diag.browser.restarts')} value=${b.restarts} />
        <${StatRow} label=${t('diag.browser.solver_attempts')} value=${b.solver_attempts} />
        <${StatRow} label=${t('diag.browser.solver_successes')} value=${b.solver_successes} />
        <${StatRow} label=${t('diag.browser.solver_failures')} value=${b.solver_failures} />
        <${StatRow} label=${t('diag.browser.graceful_shutdowns')} value=${b.graceful_shutdowns} />
        <${StatRow} label=${t('diag.browser.forced_terminations')} value=${b.forced_terminations} />
        <${StatRow} label=${t('diag.browser.max_instances')} value=${b.max_instances} />
      `}
    <//>
  `;
}

registerDiagnosticsCard({
  id: 'browser',
  titleKey: 'diag.browser.title',
  order: 50,
  Component: BrowserCard,
});
