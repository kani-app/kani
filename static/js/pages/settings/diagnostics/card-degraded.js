// @ts-check
import { h } from 'preact';
import htm from 'htm';
import { DiagCard, useDiagnostics, registerDiagnosticsCard } from '../diagnostics-cards.js';
import { t } from '../../../i18n.js';

const html = htm.bind(h);

/** @param {{ severity: string }} props */
function SeverityTag({ severity }) {
  const isError = severity === 'error';
  return html`
    <span
      class=${`text-[0.6875rem] font-semibold uppercase tracking-wide shrink-0 ${
        isError ? 'text-danger' : 'text-warn'
      }`}
    >
      ${isError ? t('diag.degraded.severity.error') : t('diag.degraded.severity.warn')}
    </span>
  `;
}

function DegradedCard({ refreshToken, span }) {
  const { data, error } = useDiagnostics({ refreshToken });
  const list = data?.degradations ?? [];

  return html`
    <${DiagCard}
      titleKey="diag.degraded.title"
      span=${span}
      loading=${!data && !error}
      error=${error}
    >
      ${data &&
      (list.length === 0
        ? html`<p class="text-sm text-text-muted">${t('diag.degraded.none')}</p>`
        : html`
            <div class="flex flex-col gap-4">
              ${list.map(
                (d) => html`
                  <div key=${d.id} class="flex flex-col gap-1">
                    <div class="flex items-baseline gap-2">
                      <span class="text-sm font-semibold text-text">${d.title}</span>
                      <${SeverityTag} severity=${d.severity} />
                    </div>
                    <p class="text-sm text-text-muted">${d.detail}</p>
                    <p class="text-xs text-text-faint">${t('diag.degraded.remedy')} ${d.remedy}</p>
                  </div>
                `,
              )}
            </div>
          `)}
    <//>
  `;
}

registerDiagnosticsCard({
  id: 'degraded',
  titleKey: 'diag.degraded.title',
  order: 5,
  span: 2,
  Component: DegradedCard,
});
