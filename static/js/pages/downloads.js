// @ts-check

import { h, render } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import { getState, subscribe } from '../cache.js';
import { cancelDownload, getDownloadHistory, retryChapterDownload } from '../api.js';
import { navigate } from '../router.js';
import { iconX, iconCheck, iconWarning, iconDownload } from '../icons.js';
import { formatChapterTitle, formatRelativeTime, getLocalInt, setLocal } from '../utils.js';
import { Icon } from '../components/icon.js';
import { EmptyState } from '../components/empty-state.js';
import { setPageHeader, clearPageHeader } from '../components/app-header.js';
import { showApiError } from '../components/toast.js';
import { useBulkSelect } from '../hooks/use-bulk-select.js';
import { t } from '../i18n.js';
const html = htm.bind(h);

/** @typedef {import('../cache.js').ChapterProgress} ChapterProgress */

const HISTORY_SIZES = [5, 10, 25, 50, 100];
const HISTORY_KEY = 'kani_download_history_size';

/** @param {{ entry: ChapterProgress, selected: boolean, onToggle: () => void }} props */
function ActiveRow({ entry, selected, onToggle }) {
  const pct = entry.totalPages > 0
    ? Math.round((entry.completedPages / entry.totalPages) * 100)
    : 0;

  async function handleCancel() {
    try { await cancelDownload(entry.id); } catch { }
  }

  return html`
    <div class=${'flex flex-col gap-2 px-4 py-3 border-b border-border-subtle last:border-b-0' + (selected ? ' bg-accent-dim' : '')}>
      <div class="flex items-center gap-3">
        <input
          type="checkbox"
          class="shrink-0 accent-accent cursor-pointer"
          checked=${selected}
          onChange=${onToggle}
          aria-label=${t('downloads.row.select', { name: entry.name || t('downloads.item') })}
        />
        <div class="shrink-0 w-7 h-7 flex items-center justify-center rounded-full text-accent">
          <svg class="w-4 h-4 dl-ring-spin" viewBox="0 0 32 32" aria-hidden="true">
            <circle cx="16" cy="16" r="12" fill="none" stroke="currentColor" stroke-width="2.5" opacity="0.25" />
            <circle cx="16" cy="16" r="12" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"
              stroke-dasharray="56.5 18.9" />
          </svg>
        </div>
        <div class="flex-1 min-w-0">
          ${entry.mangaTitle && html`
            <p class="text-xs text-text-muted truncate">${entry.mangaTitle}</p>
          `}
          <p class="text-sm text-text truncate" title=${entry.name}>${entry.name}</p>
          ${entry.totalPages > 0 && html`
            <p class="text-xs text-text-muted mt-0.5">${entry.completedPages} / ${entry.totalPages} pages</p>
          `}
        </div>
        <button class="btn-ghost btn-sm shrink-0 text-danger" onClick=${handleCancel} aria-label=${t('common.cancel')}>${t('common.cancel')}</button>
      </div>
      <div class="h-1 rounded-full bg-surface-2 overflow-hidden ml-16">
        <div class="h-full rounded-full bg-accent transition-[width] duration-300" style=${{ width: pct + '%' }}></div>
      </div>
    </div>
  `;
}

/** @param {{ entry: ChapterProgress }} props */
function HistoryRow({ entry }) {
  const iconEl =
    entry.status === 'completed' || entry.status === 'completed_hidden'
      ? html`<${Icon} svg=${iconCheck} />`
      : entry.status === 'failed'
        ? html`<${Icon} svg=${iconWarning} />`
        : html`<${Icon} svg=${iconX} />`;

  const iconColor =
    entry.status === 'completed' || entry.status === 'completed_hidden' ? 'text-success' :
    entry.status === 'failed' ? 'text-danger' : 'text-text-muted';
  
  const date = entry.downloadedAt ? new Date(entry.downloadedAt) : null;
  const relTime = entry.downloadedAt ? formatRelativeTime(date) : null;
  const absTime = entry.downloadedAt ? date.toLocaleString() : null;

  return html`
    <div class="flex items-center gap-3 px-4 py-3 border-b border-border-subtle last:border-b-0">
      <div class=${'shrink-0 w-7 h-7 flex items-center justify-center icon-sm ' + iconColor}>
        ${iconEl}
      </div>
      <div class="flex-1 min-w-0">
        ${entry.mangaId > 0 && html`
          <a
            href=${'/manga/' + entry.mangaId}
            class="text-xs text-text-muted truncate block hover:text-accent transition-colors"
            onClick=${(/** @type {MouseEvent} */ e) => { e.preventDefault(); navigate('/manga/' + entry.mangaId); }}
          >${entry.mangaTitle || 'Manga'}</a>
        `}
        <a
          href=${'/reader/' + entry.id}
          class="text-sm text-text truncate block hover:text-accent transition-colors"
          onClick=${(/** @type {MouseEvent} */ e) => { e.preventDefault(); navigate('/reader/' + entry.id); }}
        >${entry.name || formatChapterTitle({ chapter_number: entry.number ?? null })}</a>
        ${relTime && html`
          <span class="text-xs text-text-muted mt-0.5 block" title=${absTime}>${relTime}</span>
        `}
      </div>
    </div>
  `;
}

function DownloadsPage() {
  const [activeTab, setActiveTab] = useState(/** @type {'active'|'history'} */ ('active'));
  const [historySize, setHistorySize] = useState(() => getLocalInt(HISTORY_KEY, 10));
  const [active, setActive] = useState(/** @type {ChapterProgress[]} */ ([]));
  const [history, setHistory] = useState(/** @type {ChapterProgress[]} */ ([]));

  const activeIds = active.map(e => e.id);
  const failedIds = history.filter(e => e.status === 'failed').map(e => e.id);
  const bulkActive = useBulkSelect(activeIds);
  const bulkFailed = useBulkSelect(failedIds);

  async function cancelSelected() {
    const ids = [...bulkActive.selected];
    bulkActive.clear();
    await Promise.all(ids.map(id => cancelDownload(id).catch(() => {})));
  }

  const retrySelected = useCallback(async () => {
    const ids = [...bulkFailed.selected];
    bulkFailed.clear();
    const results = await Promise.allSettled(ids.map(id => retryChapterDownload(id)));
    const failed = results.filter(r => r.status === 'rejected');
    if (failed.length > 0) showApiError(/** @type {any} */ (failed[0]).reason);
  }, [bulkFailed]);

  useEffect(() => {
    function syncActive() {
      /** @type {Map<number, ChapterProgress>} */
      const map = getState('chaptersProgress');
      const entries = [...map.values()].filter(e => e.status === 'in_progress');
      entries.sort((a, b) => b.id - a.id);
      setActive(entries);
    }
    syncActive();
    return subscribe('chaptersProgress', syncActive);
  }, []);

  // Seed history from API (re-fetches on size change), then merge new SSE completions
  useEffect(() => {
    /** @type {Set<number>} */
    const seenIds = new Set();

    getDownloadHistory(historySize).then(items => {
      if (!Array.isArray(items)) return;
      const mapped = items.map(item => ({
        id: item.id,
        name: item.name,
        mangaId: item.mangaId,
        mangaTitle: item.mangaTitle,
        totalPages: 0,
        completedPages: 0,
        downloadedAt: item.downloadedAt ?? null,
        status: /** @type {ChapterProgress['status']} */ ('completed'),
      }));
      for (const e of mapped) seenIds.add(e.id);
      setHistory(mapped);
    }).catch(() => {});

    // Append new completions from SSE-driven chaptersProgress state
    return subscribe('chaptersProgress', () => {
      /** @type {Map<number, ChapterProgress>} */
      const map = getState('chaptersProgress');
      const newEntries = /** @type {ChapterProgress[]} */ ([]);
      for (const e of map.values()) {
        if ((e.status === 'completed' || e.status === 'completed_hidden' || e.status === 'failed' || e.status === 'cancelled') && !seenIds.has(e.id)) {
          seenIds.add(e.id);
          newEntries.push(e);
        }
      }
      if (newEntries.length > 0) {
        newEntries.sort((a, b) => b.id - a.id);
        setHistory(prev => [...newEntries, ...prev]);
      }
    });
  }, [historySize]);

  function changeHistorySize(/** @type {number} */ n) {
    setHistorySize(n);
    setLocal(HISTORY_KEY, String(n));
  }

  /** @param {'active'|'history'} id */
  function TabBtn({ id, label, count }) {
    const isActive = activeTab === id;
    return html`
      <button
        type="button"
        role="tab"
        aria-selected=${isActive}
        class=${'tab-btn px-4 py-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded-t-md'
          + (isActive ? ' text-accent border-b-2 border-accent' : ' text-text-muted hover:text-text')}
        onClick=${() => setActiveTab(id)}
      >
        ${label}${count > 0 ? html` <span class="ml-1 text-xs opacity-70">${count}</span>` : ''}
      </button>
    `;
  }

  return html`
    <div class="max-w-page mx-auto w-full px-4 md:px-6 py-6 flex flex-col gap-6 page-body-host page-col">

      <!-- Tab bar with inline "Show last" control when on History tab -->
      <div class="flex items-center gap-1 border-b border-border -mb-3 min-h-9" role="tablist">
        <${TabBtn} id="active" label=${t('downloads.tab.active')} count=${active.length} />
        <${TabBtn} id="history" label=${t('downloads.tab.history')} count=${0} />
        ${activeTab === 'history' && html`
          <div class="ml-auto flex items-center gap-2 pb-1">
            <span class="text-xs text-text-muted">${t('downloads.history.show_last')}</span>
            <select
              class="input input-sm w-20"
              aria-label=${t('downloads.history.size.label')}
              value=${historySize}
              onChange=${(/** @type {any} */ e) => changeHistorySize(Number(e.target.value))}
            >
              ${HISTORY_SIZES.map(n => html`<option key=${n} value=${n}>${n}</option>`)}
            </select>
          </div>
        `}
      </div>

      <!-- Active tab -->
      ${activeTab === 'active' && html`
        <div class="flex flex-col gap-2 page-fill page-col">
          ${active.length > 0 && html`
            <div class="flex items-center gap-3 px-4 py-2 bg-surface-2 rounded-lg border border-border">
              <input
                type="checkbox"
                class="accent-accent cursor-pointer"
                checked=${bulkActive.headerState === 'checked'}
                ref=${(el) => { if (el) el.indeterminate = bulkActive.headerState === 'indeterminate'; }}
                onChange=${bulkActive.toggleAll}
                aria-label=${t('downloads.bulk.select_all')}
              />
              <span class="text-sm text-text-muted flex-1">
                ${bulkActive.count > 0 ? t('chapter.bulk.selected', { count: bulkActive.count }) : t('downloads.active.queue', { count: active.length, s: active.length !== 1 ? 's' : '' })}
              </span>
              ${bulkActive.count > 0 && html`
                <button class="btn-danger btn-sm" onClick=${cancelSelected}>
                  ${t('downloads.bulk.cancel', { count: bulkActive.count })}
                </button>
              `}
            </div>
          `}
          <div class="bg-surface border border-border rounded-xl overflow-x-hidden page-body--fit">
            ${active.length === 0
              ? html`<${EmptyState} icon=${iconDownload} title=${t('downloads.empty.title')} subtitle=${t('downloads.empty.desc')} />`
              : active.map(e => html`<${ActiveRow}
                  key=${e.id}
                  entry=${e}
                  selected=${bulkActive.isSelected(e.id)}
                  onToggle=${() => bulkActive.toggle(e.id)}
                />`)
            }
          </div>
        </div>
      `}

      <!-- History tab -->
      ${activeTab === 'history' && html`
        <div class="flex flex-col gap-3 page-fill page-col">
          ${failedIds.length > 0 && html`
            <div class="flex items-center gap-3 px-4 py-2 bg-surface-2 rounded-lg border border-border">
              <input
                type="checkbox"
                class="accent-accent cursor-pointer"
                checked=${bulkFailed.headerState === 'checked'}
                ref=${(el) => { if (el) el.indeterminate = bulkFailed.headerState === 'indeterminate'; }}
                onChange=${bulkFailed.toggleAll}
                aria-label=${t('downloads.bulk.select_all_failed')}
              />
              <span class="text-sm text-text-muted flex-1">
                ${bulkFailed.count > 0 ? t('chapter.bulk.selected', { count: bulkFailed.count }) : t('downloads.failed.queue', { count: failedIds.length })}
              </span>
              ${bulkFailed.count > 0 && html`
                <button class="btn-primary btn-sm" onClick=${retrySelected}>
                  ${t('downloads.bulk.retry', { count: bulkFailed.count })}
                </button>
              `}
            </div>
          `}
          <div class="bg-surface border border-border rounded-xl overflow-x-hidden page-body--fit">
            ${history.length === 0
              ? html`<${EmptyState} icon=${iconCheck} title=${t('downloads.history.empty.title')} subtitle=${t('downloads.history.empty.desc')} />`
              : history.map(e => html`<${HistoryRow} key=${e.id} entry=${e} />`)
            }
          </div>
        </div>
      `}
    </div>
  `;
}

/** @param {HTMLElement} container */
export async function init(container) {
  document.title = 'Downloads - Kani';
  setPageHeader({ crumbs: [{ label: t('downloads.crumb') }] });
  container.classList.add('page-fixed');
  render(html`<${DownloadsPage} />`, container);
}

/** @param {HTMLElement} container */
export function destroy(container) {
  clearPageHeader();
  render(null, container);
}
