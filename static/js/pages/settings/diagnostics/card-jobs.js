// @ts-check
import { h } from 'preact';
import htm from 'htm';
import { DiagCard, useDiagnostics, registerDiagnosticsCard } from '../diagnostics-cards.js';
import { StatRow } from '../../../components/stat-row.js';
import { t } from '../../../i18n.js';

const html = htm.bind(h);

function JobsCard({ refreshToken }) {
  const { data, error } = useDiagnostics({ refreshToken });
  return html`
    <${DiagCard} titleKey="diag.jobs.title" loading=${!data && !error} error=${error}>
      ${data &&
      html`
        <${StatRow} label=${t('diag.jobs.running')} value=${data.jobs_running} />
        <${StatRow} label=${t('diag.jobs.downloads')} value=${data.active_downloads} />
      `}
    <//>
  `;
}

registerDiagnosticsCard({
  id: 'jobs',
  titleKey: 'diag.jobs.title',
  order: 30,
  Component: JobsCard,
});
