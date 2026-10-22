// @ts-check
import { h } from 'preact';
import htm from 'htm';
import { DiagCard, useDiagnostics, registerDiagnosticsCard } from '../diagnostics-cards.js';
import { StatRow } from '../../../components/stat-row.js';
import { formatBytes } from '../../../utils.js';
import { t } from '../../../i18n.js';

const html = htm.bind(h);

function StorageCard({ refreshToken }) {
  const { data, error } = useDiagnostics({ refreshToken });
  return html`
    <${DiagCard} titleKey="diag.storage.title" loading=${!data && !error} error=${error}>
      ${data &&
      html`
        <${StatRow}
          label=${t('diag.storage.free_data')}
          value=${formatBytes(data.disk_free_data_bytes)}
        />
        <${StatRow}
          label=${t('diag.storage.free_library')}
          value=${formatBytes(data.disk_free_library_bytes)}
        />
      `}
    <//>
  `;
}

registerDiagnosticsCard({
  id: 'storage',
  titleKey: 'diag.storage.title',
  order: 40,
  Component: StorageCard,
});
