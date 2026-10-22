// @ts-check
import { h } from 'preact';
import htm from 'htm';
import { DiagCard, useDiagnostics, registerDiagnosticsCard } from '../diagnostics-cards.js';
import { StatRow } from '../../../components/stat-row.js';
import { formatDuration } from '../../../utils.js';
import { t } from '../../../i18n.js';

const html = htm.bind(h);

function SystemCard({ refreshToken }) {
  const { data, error } = useDiagnostics({ refreshToken });
  return html`
    <${DiagCard} titleKey="diag.system.title" loading=${!data && !error} error=${error}>
      ${data &&
      html`
        <${StatRow} label=${t('diag.system.version')} value=${data.version} />
        <${StatRow} label=${t('diag.system.git_sha')} value=${data.git_sha || '—'} />
        <${StatRow} label=${t('diag.system.uptime')} value=${formatDuration(data.uptime_secs)} />
      `}
    <//>
  `;
}

registerDiagnosticsCard({
  id: 'system',
  titleKey: 'diag.system.title',
  order: 10,
  Component: SystemCard,
});
