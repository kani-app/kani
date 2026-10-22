// @ts-check

import { h } from 'preact';
import { useState } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { t } from '../i18n.js';
import { showApiError, showToast } from './toast.js';
import { showConfirm } from './modal.js';
import { EmptyState } from './empty-state.js';
import { useBusy } from '../hooks/use-busy.js';
import { formatRelativeTime } from '../utils.js';

const html = htm.bind(h);

/**
 * @param {{ title: string, count: number, tone?: 'warn'|'danger'|'muted',
 *           action?: any, children?: any }} props
 */
function Category({ title, count, tone = 'muted', action, children }) {
  const [open, setOpen] = useState(false);
  if (count === 0) return null;

  const toneClass =
    tone === 'danger' ? 'text-danger' : tone === 'warn' ? 'text-warn' : 'text-text';

  return html`
    <div class="border-t border-border-subtle">
      <div class="flex items-center justify-between gap-2 px-4 py-2.5">
        <button
          type="button"
          class="flex items-center gap-2 text-sm text-left"
          onClick=${() => setOpen((o) => !o)}
          aria-expanded=${open}
        >
          <span class="text-text-muted w-3 inline-block">${open ? '▾' : '▸'}</span>
          <span class="${toneClass}">${title}</span>
          <span class="text-xs text-text-muted tabular-nums">${count}</span>
        </button>
        ${action}
      </div>
      ${open
        ? html`<div class="px-4 pb-3 pl-9 flex flex-col gap-1 max-h-64 overflow-y-auto">
            ${children}
          </div>`
        : null}
    </div>
  `;
}

/** @param {string} p */
function basename(p) {
  const parts = String(p).split(/[/\\]/);
  return parts[parts.length - 1] || p;
}

/**
 * @param {{ last: any, onChanged: () => Promise<void> | void }} props
 */
export function ScrubReport({ last, onChanged }) {
  const { busy, run } = useBusy();

  if (last === undefined) return null;
  if (last === null) {
    return html`<p class="px-4 py-3 text-sm text-text-muted">${t('storage.scrub.never')}</p>`;
  }

  const r = last.report;
  const orphans = r.orphaned_files ?? [];
  const issues =
    r.corrupt.length +
    r.missing_files.length +
    r.path_drift.length +
    orphans.length +
    r.cover_mismatches.length +
    r.exact_duplicates.length;

  const removeOrphans = () =>
    run(async () => {
      try {
        // Preview first: the count the server would actually act on may differ
        // from the list on screen if the disk moved under us.
        const preview = await api.deleteOrphans(orphans, true);
        const ok = await showConfirm(
          t('storage.scrub.orphans.confirm', { n: preview.removed_count }),
          {
            title: t('storage.scrub.orphans.delete'),
            confirmLabel: t('common.delete'),
            danger: true,
          },
        );
        if (!ok) return;
        const res = await api.deleteOrphans(orphans, false);
        showToast(t('storage.scrub.orphans.removed', { n: res.removed_count }), {
          type: 'success',
        });
        await onChanged();
      } catch (e) {
        showApiError(e);
      }
    });

  const line = (/** @type {any} */ text, /** @type {string} */ key) =>
    html`<code key=${key} class="text-xs font-mono text-text-muted truncate" title=${text}>
      ${text}
    </code>`;

  return html`
    <div class="flex flex-col">
      <div class="px-4 py-2 text-xs text-text-muted border-t border-border-subtle">
        ${t('storage.scrub.last', {
          depth: t(`storage.scrub.depth.${last.depth}`),
          when: formatRelativeTime(new Date(last.created_at * 1000)),
        })}
        ${' · '}
        ${t('storage.scrub.verified', { ok: r.ok, checked: r.checked })}
        ${r.unhashed > 0 ? ` · ${t('storage.scrub.unhashed_n', { n: r.unhashed })}` : ''}
      </div>

      ${issues === 0
        ? html`<div class="p-2">
            <${EmptyState}
              title=${t('storage.scrub.clean')}
              subtitle=${t('storage.scrub.clean.subtitle')}
            />
          </div>`
        : html`
            <${Category}
              title=${t('storage.scrub.corrupt')}
              count=${r.corrupt.length}
              tone="danger"
            >
              ${r.corrupt.map((/** @type {any} */ c) =>
                line(`#${c[0]} — ${c[1]}`, `c${c[0]}`),
              )}
            <//>

            <${Category} title=${t('storage.scrub.missing')} count=${r.missing_files.length} tone="danger">
              ${r.missing_files.map((/** @type {number} */ id) => line(`#${id}`, `m${id}`))}
            <//>

            <${Category} title=${t('storage.scrub.drift')} count=${r.path_drift.length} tone="warn">
              ${r.path_drift.map((/** @type {any} */ d) =>
                line(`#${d[0]} → ${basename(d[1])}`, `d${d[0]}`),
              )}
            <//>

            <${Category}
              title=${t('storage.scrub.orphaned')}
              count=${orphans.length}
              tone="warn"
              action=${html`<button
                type="button"
                class="btn-ghost btn-sm text-danger"
                disabled=${busy}
                onClick=${removeOrphans}
              >
                ${t('storage.scrub.orphans.delete')}
              </button>`}
            >
              ${orphans.map((/** @type {string} */ p) => line(p, p))}
            <//>

            <${Category}
              title=${t('storage.integrity.cover_mismatches')}
              count=${r.cover_mismatches.length}
              tone="warn"
            >
              ${r.cover_mismatches.map((/** @type {string} */ p) => line(p, p))}
            <//>

            <${Category} title=${t('storage.scrub.duplicates')} count=${r.exact_duplicates.length}>
              ${r.exact_duplicates.map((/** @type {number[]} */ g) =>
                line(g.map((id) => `#${id}`).join(', '), g.join('-')),
              )}
            <//>
          `}
    </div>
  `;
}
