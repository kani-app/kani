// @ts-check

import * as api from '../api.js';
import { hasPermission } from '../session.js';
import { t } from '../i18n.js';
import { renderPagination } from '../components/pagination.js';
import { getParam, replaceState as urlReplaceState } from '../url-params.js';
import { scrollPageTop } from '../router.js';
import { getMangaCoverUrl } from '../api.js';
import { applyCoverQuality } from '../cover-quality.js';
import { formatChapterTitle, hasNextPage, escapeHtml, deferredSkeleton } from '../utils.js';
import { skeletonUpdateList } from '../components/skeletons.js';
import { startLoading, finishLoading } from '../components/page-loading-bar.js';
import { createErrorState } from '../components/error-state.js';
import { createEmptyState } from '../components/empty-state.js';
import { iconBookOpen, iconDownload, iconCheck, iconSpinner } from '../icons.js';
import { getState as getCache, subscribe } from '../cache.js';
import { getState as getUiState, updateState as updateUiState } from '../ui-state.js';
import { setPageHeader, clearPageHeader } from '../components/app-header.js';


/**
 * Returns the download control HTML for a chapter row.
 * Exactly one of: a read link (green tick), a download button, or empty string.
 * @param {{ id: number, title: string, is_downloaded: boolean }} ch
 * @param {boolean} canDownload
 */
function _renderDownloadControl(ch, canDownload) {
  if (ch.is_downloaded) {
    return `<a href="/reader/${ch.id}" class="icon-xs text-success shrink-0 dl-btn" aria-label="${t('updates.action.read', { title: escapeHtml(ch.title) })}" title="${t('updates.action.read_title')}">${iconCheck}</a>`;
  }
  if (canDownload) {
    return `<button class="dl-btn shrink-0" aria-label="${t('updates.action.download', { title: escapeHtml(ch.title) })}" data-chapter-id="${ch.id}" data-chapter-title="${escapeHtml(ch.title)}" title="${t('updates.action.download_title')}">${iconDownload}</button>`;
  }
  return '';
}

/** Formats a chapter number for the group subtitle (e.g. 167.5 stays 167.5). */
function _fmtNum(n) {
  return String(n);
}


let _page = 1;
/** @type {AbortController | null} */
let _abort = null;
/** @type {(() => void) | null} */
let _destroyPagination = null;
/** @type {(() => void) | null} */
let _unsubProgress = null;
/** @type {HTMLElement | null} */
let _listEl = null;


/** @param {HTMLElement} container */
export async function init(container) {
  document.title = 'Recent Updates - Kani';
  _page = parseInt(getParam('page') ?? '1', 10) || 1;
  _listEl = null;
  _unsubProgress?.();
  _unsubProgress = null;
  setPageHeader({ crumbs: [{ label: t('updates.crumb') }] });

  container.classList.add('page-fixed');
  container.innerHTML = `
    <div class="max-w-page mx-auto w-full overflow-x-hidden px-4 md:px-6 py-4 md:py-6 flex flex-col gap-6 page-body-host page-col">
      <div class="js-list page-body flex flex-col gap-6 min-w-0" aria-live="polite" aria-busy="true"></div>
      <div class="js-pagination"></div>
    </div>
  `;

  _listEl = /** @type {HTMLElement} */ (container.querySelector('.js-list'));
  const paginEl = /** @type {HTMLElement} */ (container.querySelector('.js-pagination'));

  _unsubProgress = subscribe('chaptersProgress', _onProgressUpdate);

  await _fetch(_listEl, paginEl);

}


function _updateUrl() {
  urlReplaceState(_page > 1 ? { page: _page } : {});
}


/** @param {HTMLElement} listEl @param {HTMLElement} paginEl */
async function _fetch(listEl, paginEl) {
  _abort?.abort();
  _abort = new AbortController();

  listEl.setAttribute('aria-busy', 'true');
  startLoading();
  const cancelSkeleton = deferredSkeleton(() => { listEl.innerHTML = skeletonUpdateList(4); });

  _destroyPagination?.();
  _destroyPagination = null;
  paginEl.innerHTML = '';

  let result;
  try {
    result = await api.getRecentUpdates(_page, _abort.signal);
  } catch (e) {
    cancelSkeleton();
    if (e?.name === 'AbortError') return;
    listEl.innerHTML = '';
    listEl.setAttribute('aria-busy', 'false');
    finishLoading();
    listEl.appendChild(createErrorState({
      message: t('updates.error.load_failed'),
      onRetry: () => _fetch(listEl, paginEl),
    }));
    return;
  }

  cancelSkeleton();
  finishLoading();
  listEl.innerHTML = '';
  listEl.setAttribute('aria-busy', 'false');

  const updates = Array.isArray(result?.recent_updates) ? result.recent_updates
    : Array.isArray(result?.updates)                    ? result.updates
    : Array.isArray(result)                             ? result
    : [];

  if (updates.length === 0) {
    listEl.appendChild(createEmptyState({
      icon: iconBookOpen,
      title: t('updates.empty.title'),
      subtitle: t('updates.empty.subtitle'),
    }));
    return;
  }

  /** @type {Map<string, Map<number, { manga_id: number, manga_title: string, cover_url: string | null, chapters: any[] }>>} */
  const byDate = new Map();
  /** Tracks the newest timestamp seen for each dateKey so we can sort groups newest-first. */
  const rawDates = /** @type {Map<string, number>} */ (new Map());
  for (const item of updates) {
    const rawDate = item.discovered_at ?? item.date_uploaded ?? null;
    const dateKey = rawDate ? _relativeDate(rawDate) : 'Unknown date';
    const mid = item.manga_id ?? item.manga?.id;
    const title = item.manga_name ?? item.manga_title ?? item.manga?.title ?? '';

    if (!byDate.has(dateKey)) byDate.set(dateKey, new Map());
    const byManga = /** @type {Map<number, any>} */ (byDate.get(dateKey));
    if (!byManga.has(mid)) byManga.set(mid, { manga_id: mid, manga_title: title, cover_url: item.cover_url ?? null, chapters: [] });
    byManga.get(mid).chapters.push({
      id: item.chapter_id ?? item.id,
      title: formatChapterTitle(item),
      chapter_number: item.chapter_number,
      date_uploaded: rawDate,
      is_downloaded: item.is_downloaded ?? false,
    });
    const ts = rawDate ? new Date(rawDate).getTime() : 0;
    if (!rawDates.has(dateKey) || ts > /** @type {number} */ (rawDates.get(dateKey))) {
      rawDates.set(dateKey, ts);
    }
  }

  const canDownload = hasPermission('chapter:download');

  // Sort date groups newest-first regardless of Map insertion order.
  const sortedGroups = [...byDate.entries()].sort(
    ([a], [b]) => (rawDates.get(b) ?? 0) - (rawDates.get(a) ?? 0),
  );

  for (const [dateLabel, mangaMap] of sortedGroups) {
    const daySection = document.createElement('section');
    daySection.className = 'flex flex-col mt-6 first:mt-0';

    const dateHeader = document.createElement('h2');
    dateHeader.className = 'update-day__label';
    dateHeader.textContent = dateLabel;
    daySection.appendChild(dateHeader);

    const dayItems = document.createElement('div');
    dayItems.className = 'update-day__items';
    daySection.appendChild(dayItems);

    for (const group of mangaMap.values()) {
      const groupEl = document.createElement('div');
      groupEl.className = 'update-group';

      const coverUrl = applyCoverQuality(group.cover_url ?? getMangaCoverUrl(group.manga_id, 'sm'));
      const mangaHref = `/manga/${group.manga_id}`;
      const n = group.chapters.length;
      const nums = group.chapters
        .map(c => c.chapter_number)
        .filter(v => typeof v === 'number' && !isNaN(v))
        .sort((a, b) => a - b);
      const range = nums.length
        ? (nums[0] === nums[nums.length - 1]
            ? t('updates.group.chapter_one', { n: _fmtNum(nums[0]) })
            : t('updates.group.chapter_range', { from: _fmtNum(nums[0]), to: _fmtNum(nums[nums.length - 1]) }))
        : '';
      const subtitle = [t('updates.group.count', { count: n, s: n === 1 ? '' : 's' }), range]
        .filter(Boolean)
        .join(' · ');

      const coverHtml = `
        <a href="${escapeHtml(mangaHref)}" class="w-11 h-16 rounded-md overflow-hidden shrink-0 bg-surface-2 block focus-visible:ring-2 focus-visible:ring-accent focus-visible:outline-none">
          <img src="${escapeHtml(coverUrl)}" alt="${escapeHtml(group.manga_title)}" class="w-full h-full object-cover" loading="lazy" />
        </a>`;
      const titleHtml = `
        <a href="${escapeHtml(mangaHref)}" class="touch-min-h inline-flex items-center text-base font-semibold text-text hover:underline truncate focus-visible:outline-none focus-visible:underline">
          ${escapeHtml(group.manga_title)}
        </a>`;

      if (n === 1) {
        const ch = group.chapters[0];
        groupEl.innerHTML = `
          <div class="flex items-center gap-3 min-w-0">
            ${coverHtml}
            <div class="flex flex-col min-w-0 flex-1">
              ${titleHtml}
              <span class="text-sm text-text-muted truncate mt-0.5">${escapeHtml(ch.title)}</span>
            </div>
            <span class="js-dl-ctrl shrink-0">${_renderDownloadControl(ch, canDownload)}</span>
          </div>
        `;
        dayItems.appendChild(groupEl);
        continue;
      }

      groupEl.innerHTML = `
        <div class="flex items-start gap-3 min-w-0">
          ${coverHtml}
          <div class="flex flex-col min-w-0 flex-1 pt-0.5">
            ${titleHtml}
            <span class="text-xs text-text-muted mt-0.5">${escapeHtml(subtitle)}</span>
          </div>
        </div>
      `;

      const chList = document.createElement('ul');
      chList.className = 'update-group__chapters';
      chList.setAttribute('role', 'list');

      for (const ch of group.chapters) {
        const li = document.createElement('li');
        li.className = 'update-chapter-row flex items-center justify-between gap-2 border-b border-border-subtle last:border-b-0';
        li.setAttribute('role', 'listitem');
        li.dataset.chapterId = String(ch.id);
        li.innerHTML = `
          <span class="text-sm text-text flex-1 truncate min-w-0">${escapeHtml(ch.title)}</span>
          <span class="js-dl-ctrl shrink-0">${_renderDownloadControl(ch, canDownload)}</span>
        `;
        chList.appendChild(li);
      }

      groupEl.appendChild(chList);
      dayItems.appendChild(groupEl);
    }

    listEl.appendChild(daySection);
  }

  if (canDownload) {
    for (const btn of /** @type {NodeListOf<HTMLButtonElement>} */ (listEl.querySelectorAll('button[data-chapter-id]'))) {
      const id = Number(btn.dataset.chapterId);
      btn.addEventListener('click', async () => {
        /** @type {Set<number>} */
        const inFlight = getUiState('inFlightChapters');
        if (inFlight.has(id)) return;
        updateUiState('inFlightChapters', (/** @type {Set<number>} */ s) => { const n = new Set(s); n.add(id); return n; });
        btn.disabled = true;
        btn.innerHTML = iconSpinner;
        try {
          await api.downloadChapter(id);
          // Progress subscription handles button state during and after download
        } catch {
          btn.innerHTML = iconDownload;
          btn.disabled = false;
        } finally {
          updateUiState('inFlightChapters', (/** @type {Set<number>} */ s) => { const n = new Set(s); n.delete(id); return n; });
        }
      });
    }
  }

  _onProgressUpdate(getCache('chaptersProgress'));

  const hasNext = hasNextPage(result);
  if (_page > 1 || hasNext) {
    const { destroy } = renderPagination(paginEl, {
      page: _page,
      hasNext,
      total: result?.total_pages ?? undefined,
      onPageChange: (p) => { _page = p; _updateUrl(); _fetch(listEl, paginEl); scrollPageTop(); },
    });
    _destroyPagination = destroy;
  }
}


/**
 * Called whenever chaptersProgress state changes.
 * Updates download button appearance for any visible chapter rows.
 * @param {Map<number, any>} progress
 */
function _onProgressUpdate(progress) {
  if (!_listEl) return;
  for (const btn of /** @type {NodeListOf<HTMLButtonElement>} */ (_listEl.querySelectorAll('button[data-chapter-id]'))) {
    const id = Number(btn.dataset.chapterId);
    const p = progress?.get(id);
    if (!p) continue;
    const ctrl = /** @type {HTMLElement | null} */ (btn.closest('.js-dl-ctrl'));
    if (p.status === 'completed' || p.status === 'completed_hidden') {
      const title = btn.dataset.chapterTitle ?? '';
      const ch = { id, title, is_downloaded: true };
      if (ctrl) ctrl.innerHTML = _renderDownloadControl(ch, true);
    } else if (p.status === 'in_progress') {
      btn.disabled = true;
      btn.innerHTML = iconSpinner;
    } else if (p.status === 'failed' || p.status === 'cancelled') {
      btn.innerHTML = iconDownload;
      btn.disabled = false;
      updateUiState('inFlightChapters', (/** @type {Set<number>} */ s) => { const n = new Set(s); n.delete(id); return n; });
    }
  }
}


/** @param {string} dateStr */
function _relativeDate(dateStr) {
  try {
    const d = new Date(dateStr);
    const now = new Date();
    const diffDays = Math.floor((now.getTime() - d.getTime()) / 86_400_000);
    if (diffDays === 0) return t('updates.date.today');
    if (diffDays === 1) return t('updates.date.yesterday');
    return d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  } catch {
    return formatDate(dateStr);
  }
}


/** @param {HTMLElement} container */
export function destroy(container) {
  clearPageHeader();
  _abort?.abort();
  _abort = null;
  _destroyPagination?.();
  _destroyPagination = null;
  _unsubProgress?.();
  _unsubProgress = null;
  _listEl = null;
  container.innerHTML = '';
}
