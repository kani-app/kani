// @ts-check

import { h, render } from 'preact';
import { useState, useEffect, useCallback, useMemo } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { t } from '../i18n.js';
import { hasPermission } from '../session.js';
import { UpgradeCompare } from '../components/upgrade-compare.js';
import { EmptyState } from '../components/empty-state.js';
import { showConfirm } from '../components/modal.js';
import { showToast } from '../components/toast.js';
import { setPageHeader, clearPageHeader } from '../components/app-header.js';
import { useBusy } from '../hooks/use-busy.js';
import { formatChapterTitle } from '../utils.js';

const html = htm.bind(h);

/** @param {any} u */
function isReassurance(u) {
  return u.candidate?.kind === 'source_downgraded';
}

/** @param {any} u */
function chapterLabel(u) {
  return formatChapterTitle(u);
}

function UpgradeRow({ entry, canManage, onOpen }) {
  const reassurance = isReassurance(entry);
  return html`
    <button
      type="button"
      class="w-full text-left flex items-baseline gap-3 px-1 py-2.5 border-b border-border-subtle hover:bg-surface-hover"
      onClick=${() => onOpen(entry)}
      disabled=${!canManage && !reassurance}
    >
      <span class="flex-1 min-w-0">
        <span class="block text-sm text-text truncate">${entry.manga_title}</span>
        <span class="block text-xs text-text-muted truncate">
          ${chapterLabel(entry)} · ${t(entry.candidate.reason_key)}
        </span>
      </span>
      <span class="text-xs shrink-0 ${reassurance ? 'text-text-muted' : 'text-accent'}">
        ${reassurance ? t('upgrade.badge.downgrade') : t('upgrade.badge')}
      </span>
    </button>
  `;
}

function UpgradesPage() {
  const [entries, setEntries] = useState(/** @type {any[]|null} */ (null));
  const [error, setError] = useState(/** @type {any} */ (null));
  const [open, setOpen] = useState(/** @type {any} */ (null));
  const { busy, run } = useBusy();

  const canManage = hasPermission('library:manage');

  const load = useCallback(async () => {
    try {
      const res = await api.getAllUpgrades();
      setEntries(Array.isArray(res) ? res : []);
      setError(null);
    } catch (e) {
      setError(e);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  // Only preferred-scanlator candidates are safe to apply unattended: they are
  // a ranking decision the user already made. A re-upload is a quality
  // judgement and stays one-at-a-time.
  const bulkable = useMemo(
    () => (entries ?? []).filter((e) => e.candidate.kind === 'preferred_scanlator'),
    [entries],
  );

  const replaceAll = () =>
    run(async () => {
      const ok = await showConfirm(
        t('upgrades.bulk.confirm', { n: bulkable.length }),
        { title: t('upgrades.bulk'), confirmLabel: t('upgrade.replace') },
      );
      if (!ok) return;
      let done = 0;
      for (const entry of bulkable) {
        try {
          await api.applyChapterUpgrade(entry.candidate.held_chapter_id);
          done += 1;
        } catch {
          /* one failure must not abandon the rest */
        }
      }
      showToast(t('upgrades.bulk.done', { n: done }), { type: 'success' });
      await load();
    });

  if (error) {
    return html`
      <${EmptyState}
        title=${t('upgrades.error')}
        subtitle=${String(error?.message ?? error)}
      />
    `;
  }

  if (entries === null) {
    return html`<div class="p-4 text-sm text-text-muted">${t('common.loading')}</div>`;
  }

  if (entries.length === 0) {
    return html`
      <${EmptyState}
        title=${t('upgrades.empty')}
        subtitle=${t('upgrades.empty.subtitle')}
      />
    `;
  }

  return html`
    <div class="max-w-page mx-auto w-full px-4 md:px-6 py-4 md:py-6 flex flex-col gap-3 page-body-host page-col">
      <div class="flex items-baseline justify-between gap-3">
        <p class="text-sm text-text-muted">${t('upgrades.count', { n: entries.length })}</p>
        ${canManage && bulkable.length > 0
          ? html`
              <button
                type="button"
                class="btn-secondary btn-sm"
                disabled=${busy}
                onClick=${replaceAll}
              >
                ${t('upgrades.bulk.action', { n: bulkable.length })}
              </button>
            `
          : null}
      </div>

      <div class="flex flex-col page-body--fit">
        ${entries.map(
          (entry) => html`
            <${UpgradeRow}
              key=${`${entry.candidate.held_chapter_id}-${entry.candidate.candidate_source_chapter_id}`}
              entry=${entry}
              canManage=${canManage}
              onOpen=${setOpen}
            />
          `,
        )}
      </div>

      <${UpgradeCompare}
        open=${open != null}
        candidate=${open?.candidate ?? null}
        chapterTitle=${open ? `${open.manga_title} · ${chapterLabel(open)}` : ''}
        onClose=${() => setOpen(null)}
        onChanged=${load}
      />
    </div>
  `;
}

/** @param {HTMLElement} container */
export async function init(container) {
  setPageHeader({ crumbs: [{ label: t('upgrades.crumb') }] });
  container.classList.add('page-fixed');
  render(html`<${UpgradesPage} />`, container);
}

/** @param {HTMLElement} container */
export function destroy(container) {
  clearPageHeader();
  render(null, container);
  container.innerHTML = '';
}
