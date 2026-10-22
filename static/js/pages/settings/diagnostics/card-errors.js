// @ts-check
import { h } from 'preact';
import htm from 'htm';
import { DiagCard, useDiagnostics, registerDiagnosticsCard } from '../diagnostics-cards.js';
import { StatRow } from '../../../components/stat-row.js';
import { t } from '../../../i18n.js';

const html = htm.bind(h);

function ErrorsCard({ refreshToken }) {
  const { data, error } = useDiagnostics({ refreshToken });
  return html`
    <${DiagCard} titleKey="diag.errors.title" loading=${!data && !error} error=${error}>
      ${data &&
      html`
        <${StatRow} label=${t('diag.errors.recent')} value=${data.recent_error_count} />
        <a class="text-sm text-accent hover:underline" href="/admin/logs">
          ${t('diag.errors.view_logs')}
        </a>
      `}
    <//>
  `;
}

registerDiagnosticsCard({
  id: 'errors',
  titleKey: 'diag.errors.title',
  order: 45,
  Component: ErrorsCard,
});
