// @ts-check
import { h } from 'preact';
import htm from 'htm';
import { DiagCard, useDiagnostics, registerDiagnosticsCard } from '../diagnostics-cards.js';
import { StatRow } from '../../../components/stat-row.js';
import { formatBytes } from '../../../utils.js';
import { t } from '../../../i18n.js';

const html = htm.bind(h);

function DatabaseCard({ refreshToken }) {
  const { data, error } = useDiagnostics({ refreshToken });
  return html`
    <${DiagCard} titleKey="diag.database.title" loading=${!data && !error} error=${error}>
      ${data &&
      html`
        <${StatRow} label=${t('diag.database.size')} value=${formatBytes(data.db_size_bytes)} />
        <${StatRow} label=${t('diag.database.wal')} value=${formatBytes(data.db_wal_size_bytes)} />
      `}
    <//>
  `;
}

registerDiagnosticsCard({
  id: 'database',
  titleKey: 'diag.database.title',
  order: 20,
  Component: DatabaseCard,
});
