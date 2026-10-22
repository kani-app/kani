// @ts-check

import { h } from 'preact';
import { memo } from 'preact/compat';
import { useState, useEffect, useRef, useMemo, useCallback } from 'preact/hooks';
import htm from 'htm';
import { hasPermission } from '../session.js';
import { UpgradeCompare } from './upgrade-compare.js';
import { getState, subscribe } from '../cache.js';
import { formatDate, isChapterDownloaded } from '../utils.js';
import { useBusy } from '../hooks/use-busy.js';
import { navigate } from '../router.js';
import { downloadChapter, deleteChapter, cancelDownload, setChapterReadStatus, markChaptersUpTo, retryChapterDownload } from '../api.js';
import * as api from '../api.js';
import { Modal } from './modal.js';
import { iconCheck, iconDownload, iconCloud, iconCloudCheck, iconPencil, iconEllipsisVertical } from '../icons.js';
import { Icon } from './icon.js';
import { ContextMenu } from './menu.js';
import { BulkBar } from './bulk-bar.js';
import { cacheChapter, evictChapter } from '../offline.js';
import { showApiError, showToast } from './toast.js';
import { t } from '../i18n.js';
const html = htm.bind(h);

/** @typedef {import('../cache.js').ChapterProgress} ChapterProgress */

/**
 * Row height for the windowed layout, from the --chapter-row-h token (56px
 * comfortable, denser in compact mode). Read per mount: `.chapter-row` in CSS
 * consumes the same token, so JS offsets and rendered heights stay in lockstep.
 */
export function readChapterRowHeight() {
  return _readRowH();
}

function _readRowH() {
  const v = parseInt(getComputedStyle(document.documentElement).getPropertyValue('--chapter-row-h'), 10);
  return Number.isFinite(v) && v > 0 ? v : 56;
}
const OVERSCAN = 5;

/**
 * @typedef {{
 *   id: number,
 *   title: string,
 *   chapter_number?: number | null,
 *   source_chapter_id?: string | null,
 *   scanlator?: string | null,
 *   date_uploaded?: string | null,
 *   downloaded: boolean,
 *   read?: boolean,
 *   is_orphaned?: boolean,
 * }} VirtualChapter
 */

/**
 * @param {{
 *   chapter: VirtualChapter,
 *   readerHref: string,
 *   inLibrary: boolean,
 *   mangaId?: number | null,
 *   selectMode?: boolean,
 *   selected?: boolean,
 *   isCached?: boolean,
 *   kccAvailable?: boolean,
 *   isKeyboardActive?: boolean,
 *   menuTick?: number,
 *   hasNote?: boolean,
 *   onToggleRead?: (id: number, isRead: boolean) => void,
 *   onMarkUpTo?: (chapterNumber: number, isRead: boolean) => void,
 *   onToggleSelect?: (id: number) => void,
 *   onEnterSelectWithChapter?: (id: number) => void,
 *   onDelete?: (id: number) => void,
 *   onCacheChange?: (id: number, cached: boolean) => void,
 * }} props
 */
function ChapterRowInner({ chapter, readerHref, inLibrary, mangaId, onAssignVolume, selectMode, selected, isCached, kccAvailable, hasNote, showScanlator, isKeyboardActive, menuTick, onToggleRead, onMarkUpTo, onToggleSelect, onEnterSelectWithChapter, onDelete, onCacheChange, onUpgradeClick }) {
  const upgradeCandidate = chapter.upgrade_available?.candidates?.[0] ?? null;
  const upgradeIsReassurance = upgradeCandidate?.kind === 'source_downgraded';

  const [progress, setProgress] = useState(/** @type {ChapterProgress|null} */(null));
  const [isRead, setIsRead] = useState(!!chapter.read);
  const [menuOpen, setMenuOpen] = useState(false);
  const btnRef = useRef(/** @type {HTMLButtonElement|null} */(null));
  const longPressTimer = useRef(/** @type {ReturnType<typeof setTimeout>|null} */ (null));
  // One-shot flag: absorbs the click from pointer-up that fires immediately after the
  // long-press timer calls onEnterSelectWithChapter and the row re-renders in select mode.
  const longPressFiredRef = useRef(false);

  useEffect(() => { setIsRead(!!chapter.read); }, [chapter.read]);

  function _startLongPress(/** @type {PointerEvent} */ e) {
    if (selectMode || !inLibrary || !onEnterSelectWithChapter) return;
    // Only primary pointer (touch or left mouse)
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    longPressTimer.current = setTimeout(() => {
      longPressTimer.current = null;
      longPressFiredRef.current = true;
      // Safety reset: if click never fires (mobile OS cancels after contextmenu), clear flag
      setTimeout(() => { longPressFiredRef.current = false; }, 300);
      onEnterSelectWithChapter(chapter.id);
    }, 400);
  }

  function _cancelLongPress() {
    if (longPressTimer.current != null) {
      clearTimeout(longPressTimer.current);
      longPressTimer.current = null;
    }
  }

  useEffect(() => {
    function sync() {
      /** @type {Map<number, ChapterProgress>} */
      const map = getState('chaptersProgress');
      setProgress(map.get(chapter.id) ?? null);
    }
    sync();
    return subscribe('chaptersProgress', sync);
  }, [chapter.id]);

  // Open context menu when the listbox keyboard handler signals it
  useEffect(() => { if (menuTick) setMenuOpen(true); }, [menuTick]);


  const isActive = progress?.status === 'in_progress';
  const isFailed = progress?.status === 'failed';
  const isCancelled = progress?.status === 'cancelled';
  const downloaded = isChapterDownloaded(chapter, progress);

  const canDownload = hasPermission('chapter:download');
  const canDelete = hasPermission('chapter:delete');
  // Left-edge status stripe (a short centred segment via .chapter-row::before,
  // not a full-height border).
  const rowIndicator = downloaded ? 'downloaded' : (isRead ? 'read' : '');

  let statusIndicator = null;
  if (inLibrary) {
    if (isActive) {
      const pct = progress && progress.totalPages > 0
        ? Math.round((progress.completedPages / progress.totalPages) * 100)
        : 0;
      const spinning = pct === 0;
      const circ = 75.4;
      const ring = html`
        <svg class=${'w-4 h-4 ' + (spinning ? 'dl-ring-spin' : '-rotate-90')} viewBox="0 0 32 32" aria-hidden="true">
          <circle cx="16" cy="16" r="12" fill="none" stroke="currentColor" stroke-width="2.5" opacity="0.25" />
          <circle cx="16" cy="16" r="12" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"
            stroke-dasharray=${spinning ? '56.5 18.9' : String(circ)}
            stroke-dashoffset=${spinning ? undefined : String(circ - circ * pct / 100)}
            style=${{ transition: 'stroke-dashoffset 0.3s ease' }}
          />
        </svg>
      `;
      statusIndicator = html`<span class="text-accent shrink-0" aria-label=${pct > 0 ? t('chapter.status.downloading_pct', { pct }) : t('chapter.status.downloading')}>${ring}</span>`;
    } else if (isFailed || (chapter.download_error && !downloaded)) {
      statusIndicator = html`<span class="text-danger text-xs shrink-0 font-medium" aria-label=${t('chapter.status.failed')}>!</span>`;
    } else if (downloaded && !isCancelled) {
      statusIndicator = null;
    } else if (isRead) {
      statusIndicator = html`<span class="text-text-faint shrink-0 icon-xs" aria-label=${t('chapter.status.read_not_downloaded')}><${Icon} svg=${iconCheck} /></span>`;
    } else {
      statusIndicator = html`<span class="text-text-faint shrink-0 icon-xs" aria-label=${t('chapter.status.not_downloaded')}><${Icon} svg=${iconDownload} /></span>`;
    }
  }

  async function handleToggleRead() {
    const newRead = !isRead;
    setIsRead(newRead);
    try {
      await setChapterReadStatus([chapter.id], newRead);
      if (onToggleRead) onToggleRead(chapter.id, newRead);
    } catch (err) {
      setIsRead(!newRead);
      showApiError(err);
    }
  }

  async function handleMarkUpTo(markRead) {
    if (!mangaId || chapter.chapter_number == null) return;
    try {
      await markChaptersUpTo(mangaId, chapter.chapter_number, markRead);
      if (onMarkUpTo) onMarkUpTo(chapter.chapter_number, markRead);
    } catch (err) {
      showApiError(err);
    }
  }

  async function handleDownload() {
    try { await downloadChapter(chapter.id); } catch (err) { showApiError(err); }
  }

  async function handleDelete() {
    try {
      await deleteChapter(chapter.id);
      onDelete?.(chapter.id);
    } catch (err) { showApiError(err); }
  }

  async function handleCacheToggle() {
    if (isCached) {
      evictChapter(chapter.id);
      onCacheChange?.(chapter.id, false);
    } else {
      const pageCount = Number(/** @type {any} */ (chapter).page_count ?? 0);
      cacheChapter(chapter.id, pageCount);
      onCacheChange?.(chapter.id, true);
    }
  }

  function handleExportDownload(url) {
    const a = document.createElement('a');
    a.href = url;
    a.download = '';
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setMenuOpen(false);
  }

  async function handleCancel() {
    try { await cancelDownload(chapter.id); } catch (err) { showApiError(err); }
  }

  async function handleRetry() {
    try { await retryChapterDownload(chapter.id); } catch (err) { showApiError(err); }
  }

  /** @type {import('./menu.js').MenuItem[]} */
  const menuItems = inLibrary ? [
    { label: t('chapter.menu.select'), action: () => { if (onEnterSelectWithChapter) onEnterSelectWithChapter(chapter.id); } },
    { divider: true },
    { label: isRead ? t('chapter.menu.mark_unread') : t('chapter.menu.mark_read'), action: handleToggleRead },
    ...(chapter.chapter_number != null ? [
      { label: t('chapter.menu.mark_read_up_to'), action: () => handleMarkUpTo(true) },
      { label: t('chapter.menu.mark_unread_from'), action: () => handleMarkUpTo(false) },
    ] : []),
    ...((canDownload || canDelete) ? [{ divider: /** @type {true} */ (true) }] : []),
    ...(isActive && canDownload ? [{ label: t('chapter.menu.cancel_download'), action: handleCancel }] : []),
    ...((isFailed || chapter.download_error) && canDownload ? [{ label: t('chapter.action.retry'), action: handleRetry }] : []),
    ...(!isActive && !isFailed && !downloaded && canDownload ? [{ label: t('chapter.menu.download'), action: handleDownload }] : []),
    ...(!isActive && downloaded && !isCancelled && canDelete ? [{ label: t('chapter.menu.delete_download'), action: handleDelete, danger: true }] : []),
    ...(!isActive && downloaded && !isCancelled && ('caches' in window) ? [
      { divider: /** @type {true} */ (true) },
      ...(isCached
        ? [{ label: t('chapter.menu.remove_offline'), action: handleCacheToggle }]
        : [{ label: t('chapter.menu.save_offline'), action: handleCacheToggle }]),
    ] : []),
    ...(mangaId ? [
      { divider: /** @type {true} */ (true) },
      { label: t('chapter.menu.assign_volume'), action: () => onAssignVolume?.(chapter) },
    ] : []),
    ...(!isActive && downloaded && !isCancelled ? [
      { divider: /** @type {true} */ (true) },
      { label: t('chapter.menu.download_cbz'), action: () => handleExportDownload(`/rest/chapters/${chapter.id}/cbz`) },
      { label: t('chapter.menu.export_epub'), action: () => handleExportDownload(`/rest/chapters/${chapter.id}/export/epub`) },
      { label: t('chapter.menu.export_epub_kindle'), action: () => handleExportDownload(`/rest/chapters/${chapter.id}/export/epub?profile=kindle-pw`) },
      { label: t('chapter.menu.export_kepub_kobo'), action: () => handleExportDownload(`/rest/chapters/${chapter.id}/export/kepub?profile=kobo-libra`) },
      ...(kccAvailable ? [{ label: t('chapter.menu.export_mobi_kindle'), action: () => handleExportDownload(`/rest/chapters/${chapter.id}/export/kcc?format=MOBI&profile=KPW5&manga=true`) }] : []),
    ] : []),
  ] : [];

  const menuBtn = inLibrary ? html`
    <div class="relative">
      <button
        ref=${btnRef}
        class="touch-min inline-flex items-center justify-center w-9 h-9 text-text-muted hover:text-text rounded-md cursor-pointer select-none transition-colors"
        aria-label=${t('chapter.list.more_actions')}
        aria-haspopup="menu"
        aria-expanded=${menuOpen}
        aria-controls=${menuOpen ? `chapter-menu-${chapter.id}` : undefined}
        tabindex="-1"
        onClick=${(e) => { e.preventDefault(); e.stopPropagation(); setMenuOpen(o => !o); }}
      ><${Icon} svg=${iconEllipsisVertical} class="icon-sm" /></button>
      ${menuOpen && html`<${ContextMenu} id=${`chapter-menu-${chapter.id}`} items=${menuItems} trigger=${btnRef} onClose=${() => setMenuOpen(false)} />`}
    </div>
  ` : null;

  if (selectMode) {
    return html`
      <div
        id=${'chapter-opt-' + chapter.id}
        role="option"
        tabindex="-1"
        aria-selected=${!!selected}
        data-indicator=${rowIndicator || undefined}
        class=${'chapter-row flex items-center gap-3 px-3 border-b border-border-subtle cursor-pointer select-none' + (chapter.is_orphaned ? ' opacity-60' : '') + (selected ? ' bg-accent/10' : '') + (isKeyboardActive ? ' ring-2 ring-inset ring-accent/60' : '')}
        onClick=${() => {
          if (longPressFiredRef.current) { longPressFiredRef.current = false; return; }
          onToggleSelect && onToggleSelect(chapter.id);
        }}
      >
        <span class="kani-checkbox pointer-events-none shrink-0" aria-hidden="true">
          <input type="checkbox" checked=${!!selected} tabindex="-1" readOnly />
          <span class="kani-checkbox__box">
            <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="2"
              stroke-linecap="round" stroke-linejoin="round"><path d="m2.5 6.5 2.5 2.5 4.5-5.5"/></svg>
          </span>
        </span>
        <div class="flex-1 min-w-0 flex flex-col gap-0.5">
          <span class=${'text-sm truncate ' + (isRead ? 'text-text-faint' : 'text-text')}>${chapter.title}</span>
          <div class="flex items-center gap-3 text-xs text-text-muted">
            ${showScanlator && chapter.scanlator && html`<span>${chapter.scanlator}</span>`}
            ${chapter.date_uploaded && html`<span>${formatDate(chapter.date_uploaded)}</span>`}
          </div>
        </div>
      </div>
    `;
  }

  const isClickable = inLibrary && !isActive && (downloaded || canDownload);
  let nonClickableClass = '', nonClickableTitle = '';
  if (!isClickable) {
    nonClickableClass = ' cursor-default';
    nonClickableTitle = !inLibrary ? t('chapter.list.add_to_library') : isActive ? t('chapter.list.downloading') : t('chapter.list.download_to_read');
  }

  return html`
    <div
      id=${'chapter-opt-' + chapter.id}
      role="option"
      tabindex="-1"
      aria-selected=${undefined}
      data-indicator=${rowIndicator || undefined}
      class=${'chapter-row flex items-center gap-3 px-3 border-b border-border-subtle' + (chapter.is_orphaned ? ' opacity-60' : '') + (menuOpen ? ' relative' : '') + (isKeyboardActive ? ' ring-2 ring-inset ring-accent/60' : '')}
      style=${menuOpen ? 'z-index: 50' : undefined}
      onPointerDown=${_startLongPress}
      onPointerUp=${_cancelLongPress}
      onPointerCancel=${_cancelLongPress}
      onContextMenu=${_cancelLongPress}
    >
      <div class="flex-1 min-w-0 flex flex-col gap-0.5">
        <div class="flex items-center gap-2">
          ${chapter.is_orphaned && html`
            <span class="inline-flex items-center px-1.5 py-0.5 text-xs font-medium rounded-sm bg-warn/20 text-warn">${t('chapter.badge.orphaned')}</span>
          `}
          ${upgradeCandidate && html`
            <button
              type="button"
              class=${'inline-flex items-center px-1.5 py-0.5 text-xs font-medium rounded-sm ' +
                (upgradeIsReassurance ? 'bg-success/20 text-success' : 'bg-accent/15 text-accent')}
              title=${t(upgradeCandidate.reason_key)}
              onClick=${(/** @type {any} */ e) => {
                e.preventDefault();
                e.stopPropagation();
                onUpgradeClick?.(upgradeCandidate, chapter);
              }}
            >${upgradeIsReassurance ? t('upgrade.badge.downgrade') : t('upgrade.badge')}</button>
          `}
          ${statusIndicator}
          ${isClickable
      ? html`<a class=${'text-sm truncate hover:text-accent transition-colors ' + (!downloaded ? 'text-text-muted' : isRead ? 'text-text-faint' : 'text-text')} href=${readerHref} tabindex="-1" title=${!downloaded ? t('chapter.list.download_to_read') : undefined}>${chapter.title}</a>`
      : html`<span class=${'text-sm truncate' + nonClickableClass + (isRead ? ' text-text-faint' : ' text-text-muted')} title=${nonClickableTitle || undefined}>${chapter.title}</span>`
    }
        </div>
        <div class="flex items-center gap-3 text-xs text-text-muted">
          ${showScanlator && chapter.scanlator && html`<span>${chapter.scanlator}</span>`}
          ${chapter.date_uploaded && html`<span>${formatDate(chapter.date_uploaded)}</span>`}
          ${hasNote && html`<span class="text-accent icon-2xs" title=${t('chapter.badge.has_note')}>
            <${Icon} svg=${iconPencil} label=${t('chapter.badge.has_note')} />
          </span>`}
        </div>
      </div>
      <div class="flex items-center gap-1 shrink-0">
        ${(isFailed || chapter.download_error) && canDownload && html`
          <button class="btn-ghost btn-xs text-accent" onClick=${(e) => { e.preventDefault(); e.stopPropagation(); handleRetry(); }} aria-label=${t('chapter.action.retry')}>${t('common.retry')}</button>
        `}
        ${downloaded && !isActive && !isCancelled && html`
          <span
            class=${'icon-xs ' + (isCached ? 'text-accent' : 'text-text-faint')}
            title=${isCached ? t('chapter.badge.cached') : t('chapter.badge.not_cached')}
            aria-label=${isCached ? t('chapter.badge.cached') : t('chapter.badge.not_cached')}
          >
            <${Icon} svg=${isCached ? iconCloudCheck : iconCloud} />
          </span>
        `}
        ${menuBtn}
      </div>
    </div>
  `;
}

const ChapterRow = memo(ChapterRowInner);

/**
 * Chapter list with optional windowed rendering and infinite-scroll load-more.
 *
 * When `height` is provided, renders only the rows visible inside a fixed-height
 * scroll container. `onLoadMore` is triggered by the scroll handler when the user
 * approaches the bottom.
 *
 * Without `height`, all rows are rendered in normal document flow and an
 * IntersectionObserver sentinel triggers `onLoadMore` when it enters the viewport.
 *
 * @param {{
 *   chapters: VirtualChapter[],
 *   readerHrefFn: (ch: VirtualChapter) => string,
 *   inLibrary: boolean,
 *   mangaId?: number | null,
 *   height?: number,
 *   hasMore?: boolean,
 *   loading?: boolean,
 *   selectMode?: boolean,
 *   selected?: Set<number>,
 *   canDownload?: boolean,
 *   canDelete?: boolean,
 *   onLoadMore?: () => void,
 *   onToggleRead?: (id: number, isRead: boolean) => void,
 *   onMarkUpTo?: (chapterNumber: number, isRead: boolean) => void,
 *   onToggleSelect?: (id: number) => void,
 *   onSelectAll?: () => void,
 *   onFlipSelection?: () => void,
 *   onSelectUndownloaded?: () => void,
 *   onSelectUnread?: () => void,
 *   onSelectOrphaned?: () => void,
 *   orphanCount?: number,
 *   onBulkRead?: (isRead: boolean) => void,
 *   onBulkDownload?: () => void,
 *   onBulkDelete?: () => void,
 *   onExitSelect?: () => void,
 *   onEnterSelectWithChapter?: (id: number) => void,
 *   onDelete?: (id: number) => void,
 *   cachedChapterIds?: Set<number>,
 *   kccAvailable?: boolean,
 *   onCacheChange?: (id: number, cached: boolean) => void,
 * }} props
 */
/**
 * Assigns a chapter to one of the manga's volumes, or clears the assignment.
 *
 * `assignChapterVolume` had no caller: volumes could be created, renamed,
 * listed and deleted, but nothing could put a chapter in one.
 */
function VolumePicker({ chapter, mangaId, onClose, onAssigned }) {
  const [volumes, setVolumes] = useState(/** @type {any[]|null} */ (null));
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!chapter || !mangaId) return;
    setVolumes(null);
    api.listVolumes(mangaId).then((v) => setVolumes(Array.isArray(v) ? v : [])).catch(() => setVolumes([]));
  }, [chapter, mangaId]);

  if (!chapter) return null;

  const assign = async (/** @type {number|null} */ volumeId) => {
    setBusy(true);
    try {
      await api.assignChapterVolume(mangaId, chapter.id, volumeId);
      showToast(t('chapter.volume.assigned'), { type: 'success' });
      onAssigned?.();
      onClose();
    } catch (e) {
      showApiError(e);
    } finally {
      setBusy(false);
    }
  };

  return html`
    <${Modal} open=${true} title=${t('chapter.menu.assign_volume')} onClose=${onClose}>
      <div class="flex flex-col gap-2 px-1">
        <p class="text-xs text-text-muted">${t('chapter.volume.desc')}</p>
        ${volumes === null
          ? html`<p class="text-sm text-text-muted">${t('common.loading')}</p>`
          : html`
              <button
                type="button"
                class="text-left px-1 py-2 border-b border-border-subtle hover:bg-surface-hover text-sm"
                disabled=${busy}
                onClick=${() => assign(null)}
              >
                ${t('chapter.volume.none')}
              </button>
              ${volumes.length === 0
                ? html`<p class="text-sm text-text-muted pt-2">${t('chapter.volume.empty')}</p>`
                : volumes.map(
                    (v) => html`
                      <button
                        type="button"
                        key=${v.id}
                        class="text-left px-1 py-2 border-b border-border-subtle hover:bg-surface-hover text-sm"
                        disabled=${busy}
                        onClick=${() => assign(v.id)}
                      >
                        ${v.name || t('chapter.volume.numbered', { n: v.volume_num })}
                      </button>
                    `,
                  )}
            `}
      </div>
    <//>
  `;
}

export function VirtualChapterList({ chapters, readerHrefFn, inLibrary, mangaId, height, hasMore, loading, selectMode, selected, canDownload, canDelete, allSelectedProp, onLoadMore, onToggleRead, onMarkUpTo, onToggleSelect, onSelectAll, onFlipSelection, onSelectUndownloaded, onSelectUnread, onSelectOrphaned, orphanCount = 0, onBulkRead, onBulkDownload, onBulkDelete, onExitSelect, onEnterSelectWithChapter, onDelete, cachedChapterIds, kccAvailable, onCacheChange, notedChapterIds, onUpgradeApplied }) {
  const [upgrade, setUpgrade] = useState(/** @type {any} */ (null));
  const [volumeFor, setVolumeFor] = useState(/** @type {any} */ (null));
  const openAssignVolume = useCallback((/** @type {any} */ ch) => setVolumeFor(ch), []);
  const openUpgrade = useCallback((/** @type {any} */ candidate, /** @type {any} */ ch) => {
    setUpgrade({ candidate, title: ch.title });
  }, []);

  const [scrollTop, setScrollTop] = useState(0);
  const [activeIndex, setActiveIndex] = useState(0);
  const [focused, setFocused] = useState(false);
  // Separate from `focused`: clicking a row focuses the listbox, and painting a
  // keyboard cursor over the row the user did not click reads as a stuck
  // selection. `aria-activedescendant` still follows real focus.
  const [keyboardCursor, setKeyboardCursor] = useState(false);
  const [menuSignal, setMenuSignal] = useState({ id: /** @type {number|null} */ (null), tick: 0 });
  const [ROW_H] = useState(_readRowH);
  const showScanlator = useMemo(() => {
    const seen = new Set();
    for (const c of chapters) {
      if (c.scanlator) seen.add(c.scanlator);
      if (seen.size > 1) return true;
    }
    return false;
  }, [chapters]);
  const sentinelRef = useRef(/** @type {HTMLDivElement | null} */(null));
  const scrollRef = useRef(/** @type {HTMLDivElement | null} */(null));
  const { busy: bulkBusy, run: runBulk } = useBusy();

  useEffect(() => {
    if (height || !hasMore || !onLoadMore || !sentinelRef.current) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries[0].isIntersecting) {
        observer.disconnect();
        onLoadMore();
      }
    }, { rootMargin: '200px' });
    observer.observe(sentinelRef.current);
    return () => observer.disconnect();
  }, [height, hasMore, onLoadMore, chapters.length]);

  // Clamp activeIndex when the list shrinks (e.g. after a delete or reload)
  useEffect(() => {
    if (chapters.length > 0 && activeIndex >= chapters.length) {
      setActiveIndex(chapters.length - 1);
    }
  }, [chapters.length]);

  // Non-windowed: scroll the active row into view when it changes via keyboard
  useEffect(() => {
    if (height) return;
    document.getElementById('chapter-opt-' + chapters[activeIndex]?.id)
      ?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }, [activeIndex, height]);

  const visibleCount = height ? Math.ceil(height / ROW_H) : chapters.length;

  /**
   * Moves the cursor to the row the pointer went down on, so the arrow keys
   * carry on from what the user just touched rather than from the top.
   *
   * @param {PointerEvent} e
   */
  const handlePointerDown = useCallback((e) => {
    const target = e.target instanceof Element ? e.target.closest('[id^="chapter-opt-"]') : null;
    if (!target) return;
    const id = Number(target.id.slice('chapter-opt-'.length));
    const index = chapters.findIndex(c => c.id === id);
    if (index >= 0) setActiveIndex(index);
  }, [chapters]);

  /** @param {KeyboardEvent} e */
  function handleListKeyDown(e) {
    if (!chapters.length) return;
    const len = chapters.length;
    let newIndex = activeIndex;

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      newIndex = Math.min(activeIndex + 1, len - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      newIndex = Math.max(activeIndex - 1, 0);
    } else if (e.key === 'Home') {
      e.preventDefault();
      newIndex = 0;
    } else if (e.key === 'End') {
      e.preventDefault();
      newIndex = len - 1;
    } else if (e.key === 'PageDown') {
      e.preventDefault();
      newIndex = Math.min(activeIndex + visibleCount, len - 1);
    } else if (e.key === 'PageUp') {
      e.preventDefault();
      newIndex = Math.max(activeIndex - visibleCount, 0);
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      const ch = chapters[activeIndex];
      if (!ch) return;
      if (selectMode && onToggleSelect) {
        onToggleSelect(ch.id);
      } else {
        const href = readerHrefFn(ch);
        if (href) navigate(href);
      }
      return;
    } else if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) {
      e.preventDefault();
      const ch = chapters[activeIndex];
      if (ch) setMenuSignal(s => ({ id: ch.id, tick: s.tick + 1 }));
      return;
    } else {
      return;
    }

    setActiveIndex(newIndex);
    setKeyboardCursor(true);

    if (height && scrollRef.current) {
      const top = newIndex * ROW_H;
      const bot = top + ROW_H;
      const st = scrollRef.current.scrollTop;
      let newSt = st;
      if (top < st) newSt = top;
      else if (bot > st + height) newSt = bot - height;
      if (newSt !== st) {
        scrollRef.current.scrollTop = newSt;
        setScrollTop(newSt);
      }
    }
  }

  const skeletonRow = html`<div class="h-14 mx-3 my-1 skeleton rounded-lg" />`;

  const selectedCount = selected ? selected.size : 0;
  const allSelected = allSelectedProp !== undefined
    ? allSelectedProp
    : (selectedCount === chapters.length && chapters.length > 0);

  const selectedDownloadedCount = selected
    ? chapters.filter(ch => selected.has(ch.id) && ch.downloaded).length
    : 0;
  const selectedUndownloadedCount = selectedCount - selectedDownloadedCount;

  const statParts = [];
  if (selectedDownloadedCount > 0) statParts.push(t('chapter.bulk.stat_downloaded', { count: selectedDownloadedCount }));
  if (selectedUndownloadedCount > 0) statParts.push(t('chapter.bulk.stat_not_downloaded', { count: selectedUndownloadedCount }));

  const bulkHelpers = [
    { label: allSelected ? t('chapter.bulk.deselect_all') : t('chapter.bulk.select_all'), onClick: () => onSelectAll && onSelectAll() },
    ...(onFlipSelection ? [{ label: t('chapter.bulk.flip'), onClick: () => onFlipSelection() }] : []),
    ...(onSelectUndownloaded ? [{ label: t('chapter.bulk.undownloaded'), onClick: () => onSelectUndownloaded() }] : []),
    ...(onSelectUnread ? [{ label: t('chapter.bulk.unread'), onClick: () => onSelectUnread() }] : []),
    // Kept from a previous source. Offered even at zero so the capability is
    // discoverable, but disabled with a reason rather than silently inert.
    ...(onSelectOrphaned
      ? [{
          label: t('chapter.bulk.orphaned'),
          title: orphanCount > 0
            ? t('chapter.bulk.orphaned.title', { count: orphanCount })
            : t('chapter.bulk.orphaned.none'),
          disabled: orphanCount === 0,
          onClick: () => onSelectOrphaned(),
        }]
      : []),
  ];
  const bulkActions = [
    { label: t('chapter.bulk.mark_read'), onClick: () => onBulkRead && runBulk(() => onBulkRead(true)), disabled: selectedCount === 0 },
    { label: t('chapter.bulk.mark_unread'), onClick: () => onBulkRead && runBulk(() => onBulkRead(false)), disabled: selectedCount === 0 },
    ...(canDownload && onBulkDownload
      ? [{ label: t('chapter.bulk.download'), onClick: () => runBulk(() => onBulkDownload()), disabled: selectedUndownloadedCount === 0 }]
      : []),
    ...(canDelete && onBulkDelete
      ? [{ label: t('common.delete'), kind: /** @type {'danger'} */ ('danger'), onClick: () => runBulk(() => onBulkDelete()), disabled: selectedDownloadedCount === 0 }]
      : []),
  ];

  const bulkBar = selectMode ? html`
    <${BulkBar}
      countLabel=${t('chapter.bulk.selected', { count: selectedCount })}
      statLine=${selectedCount > 0 && statParts.length ? statParts.join(', ') : null}
      helpers=${bulkHelpers}
      actions=${bulkActions}
      busy=${bulkBusy}
      onCancel=${() => onExitSelect && onExitSelect()}
    />
  ` : null;

  const startIdx = height ? Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN) : 0;
  const endIdx = height ? Math.min(chapters.length, startIdx + visibleCount + OVERSCAN * 2) : chapters.length;
  // Only emit aria-activedescendant when the active option's DOM node exists in the current slice
  const activeOptId = chapters[activeIndex] ? 'chapter-opt-' + chapters[activeIndex].id : undefined;
  const activeDescendant = (activeOptId && activeIndex >= startIdx && activeIndex < endIdx) ? activeOptId : undefined;

  if (!height) {
    return html`
      <div
        class="flex flex-col"
        role="listbox"
        aria-label=${t('chapter.list.label')}
        tabindex="0"
        aria-activedescendant=${focused ? activeDescendant : undefined}
        onKeyDown=${handleListKeyDown}
        onPointerDown=${handlePointerDown}
        onFocus=${(/** @type {FocusEvent} */ e) => {
          setFocused(true);
          const el = /** @type {HTMLElement} */ (e.currentTarget);
          setKeyboardCursor(el.matches?.(':focus-visible') ?? false);
        }}
        onBlur=${() => { setFocused(false); setKeyboardCursor(false); }}
      >
        <div class="flex flex-col divide-y divide-border-subtle">
          ${chapters.map((ch, i) => html`
            <${ChapterRow}
              key=${ch.id}
              chapter=${ch}
              readerHref=${readerHrefFn(ch)}
              inLibrary=${inLibrary}
              mangaId=${mangaId}
              selectMode=${!!selectMode}
              selected=${selected ? selected.has(ch.id) : false}
              isKeyboardActive=${keyboardCursor && i === activeIndex}
              menuTick=${menuSignal.id === ch.id ? menuSignal.tick : 0}
              showScanlator=${showScanlator}
              onToggleRead=${onToggleRead}
              onMarkUpTo=${onMarkUpTo}
              onToggleSelect=${onToggleSelect}
              onEnterSelectWithChapter=${onEnterSelectWithChapter}
              onDelete=${onDelete}
              isCached=${cachedChapterIds ? cachedChapterIds.has(ch.id) : false}
              kccAvailable=${!!kccAvailable}
              onCacheChange=${onCacheChange}
              onUpgradeClick=${openUpgrade}
            onAssignVolume=${openAssignVolume}
              hasNote=${notedChapterIds ? notedChapterIds.has(ch.id) : false}
            />
          `)}
          ${loading ? skeletonRow : (hasMore && html`<div ref=${sentinelRef} class="h-px" />`)}
        </div>
        ${bulkBar}
      </div>
    `;
  }

  // Windowed rendering for fixed-height scroll containers
  const totalH = chapters.length * ROW_H;
  const visible = chapters.slice(startIdx, endIdx);

  function handleScroll(e) {
    const el = /** @type {HTMLElement} */ (e.currentTarget);
    setScrollTop(el.scrollTop);
    if (hasMore && onLoadMore && !loading &&
      el.scrollTop + el.clientHeight >= totalH - ROW_H * (OVERSCAN + 3)) {
      onLoadMore();
    }
  }

  return html`
    <div
      class="flex flex-col"
      role="listbox"
      aria-label=${t('chapter.list.label')}
      tabindex="0"
      aria-activedescendant=${focused ? activeDescendant : undefined}
      onKeyDown=${handleListKeyDown}
      onPointerDown=${handlePointerDown}
      onFocus=${(/** @type {FocusEvent} */ e) => {
        setFocused(true);
        const el = /** @type {HTMLElement} */ (e.currentTarget);
        setKeyboardCursor(el.matches?.(':focus-visible') ?? false);
      }}
      onBlur=${() => { setFocused(false); setKeyboardCursor(false); }}
    >
      <div
        ref=${scrollRef}
        class="overflow-y-auto"
        style=${{ height: height + 'px', scrollbarWidth: 'none' }}
        onScroll=${handleScroll}
      >
        <div style=${{ height: totalH + 'px', position: 'relative' }}>
          ${visible.map((ch, i) => html`
            <div
              key=${ch.id}
              style=${{ position: 'absolute', top: (startIdx + i) * ROW_H + 'px', width: '100%' }}
            >
              <${ChapterRow}
                chapter=${ch}
                readerHref=${readerHrefFn(ch)}
                inLibrary=${inLibrary}
                mangaId=${mangaId}
                selectMode=${!!selectMode}
                selected=${selected ? selected.has(ch.id) : false}
                isKeyboardActive=${keyboardCursor && startIdx + i === activeIndex}
                menuTick=${menuSignal.id === ch.id ? menuSignal.tick : 0}
              showScanlator=${showScanlator}
                onToggleRead=${onToggleRead}
                onMarkUpTo=${onMarkUpTo}
                onToggleSelect=${onToggleSelect}
                onEnterSelectWithChapter=${onEnterSelectWithChapter}
                onDelete=${onDelete}
                onUpgradeClick=${openUpgrade}
            onAssignVolume=${openAssignVolume}
                hasNote=${notedChapterIds ? notedChapterIds.has(ch.id) : false}
              />
            </div>
          `)}
        </div>
        ${loading && html`<div class="px-3 py-2">${skeletonRow}</div>`}
      </div>
      ${bulkBar}
    </div>
    <${VolumePicker}
      chapter=${volumeFor}
      mangaId=${mangaId}
      onClose=${() => setVolumeFor(null)}
      onAssigned=${() => onUpgradeApplied?.()}
    />
    <${UpgradeCompare}
      open=${!!upgrade}
      candidate=${upgrade?.candidate}
      chapterTitle=${upgrade?.title ?? ''}
      onClose=${() => setUpgrade(null)}
      onChanged=${() => onUpgradeApplied?.()}
    />
  `;
}
