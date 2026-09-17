// @ts-check

import { h, render } from 'preact';
import htm from 'htm';
import * as api from '../api.js';
import { hasPermission } from '../session.js';
import { getState, setState, updateState, subscribe } from '../cache.js';
import { navigate, scrollPageTop } from '../router.js';
import { debounce, hasNextPage, deferredSkeleton, addPullToRefresh, withBusy } from '../utils.js';
import { getTileSize, setTileSize } from '../tile-size.js';
import { showConfirm, Modal, mountIntoModalRoot } from '../components/modal.js';
import { TileSizeSelect } from '../components/tile-size-select.js';
import { BulkBar } from '../components/bulk-bar.js';
import { mountSavedSearches } from '../components/library/saved-searches.js';
import { showCategoryAssignModal } from '../components/library/category-assign-modal.js';
import { mountContinueShelf } from '../components/library/continue-shelf.js';
import { getParam, pushState, replaceState } from '../url-params.js';

/** @type {Record<string, number>} */
const STATUS_VALUES = { ongoing: 0, completed: 1, hiatus: 2, cancelled: 3, unknown: 4 };
import { Combobox } from '../components/combobox.js';
import { Select } from '../components/form/select.js';
import { createSearchInput } from '../components/form/search-input.js';
import { mountDisplayMenu } from '../components/library/display-menu.js';
import { renderCategoryTabs } from '../components/tabs.js';
import { openBulkMigrationDialogue } from '../components/bulk-migration-dialogue.js';
import { renderPagination } from '../components/pagination.js';
import { renderMangaGrid, createMangaCard, mangaGridCell, setMangaCardScanning, setMangaCardDownloadProgress, setNewChapterCount } from '../components/manga-card.js';
import { skeletonGridItems } from '../components/skeletons.js';
import { FALLBACK_PAGE_SIZE, MAX_PAGE_SIZE, gridCapacity, gridFitEnabled, observeCapacity } from '../grid-columns.js';
import { startLoading, finishLoading } from '../components/page-loading-bar.js';
import { createErrorState } from '../components/error-state.js';
import { createEmptyState } from '../components/empty-state.js';
import { iconBookOpen, iconChevronDown, iconRefresh, iconCheck} from '../icons.js';
import { showToast, showApiError } from '../components/toast.js';
import { showContextMenu } from '../components/menu.js';
import { setPageHeader, clearPageHeader } from '../components/app-header.js';
import { t } from '../i18n.js';
import { prefersInfiniteScroll } from '../pagination-mode.js';
const html = htm.bind(h);


let _search = '';
let _statusFilter = /** @type {string|null} */ (null);
let _tagFilter    = /** @type {number|null} */ (null);
let _authorFilter = /** @type {number|null} */ (null);
let _artistFilter = /** @type {number|null} */ (null);
let _sourceFilter = /** @type {number|null} */ (null);
let _catFilter        = /** @type {number|null} */ (null);
let _collectionFilter = /** @type {number|null} */ (null);
let _readingStatusFilter = /** @type {number|null} */ (null);
let _hideNoUnread = false;
let _hideCompletedStatus = false;
let _sortOrder = 'up';
let _page = 1;
let _pageSize = 0;
/** @type {import('../tile-size.js').TileSize} */
let _tileSize = 'md';

/**
 * A search matches titles, alternative titles, descriptions and credited
 * people. Only a match the user cannot see on the card needs saying.
 * @param {{ match_field?: string | null, match_text?: string | null }} m
 * @returns {string | null}
 */
function _matchCaption(m) {
  if (!m.match_field || !m.match_text) return null;
  if (m.match_field === 'author') return t('library.match.author', { name: m.match_text });
  if (m.match_field === 'description') return t('library.match.description', { text: m.match_text });
  return null;
}
/** @type {(() => void)|null} */ let _stopColumnWatch = null;

/** @returns {HTMLElement|null} */
function _mangaGridEl() {
  return /** @type {HTMLElement|null} */ (_gridEl?.querySelector('.manga-grid') ?? null);
}

/** @returns {HTMLElement|null} The element the grid may not overflow. */
function _boundsEl() {
  return /** @type {HTMLElement|null} */ (_container?.querySelector('.js-scroll') ?? null);
}

/**
 * The page holds exactly what fits. Where the shell does not pin the page to
 * the viewport the question has no answer, so a fixed batch is used instead.
 */
function _syncPageSize() {
  const next = gridFitEnabled() && !_isInfinite()
    ? gridCapacity(_mangaGridEl(), _boundsEl())
    : 0;
  if (next > 0) _pageSize = Math.min(next, MAX_PAGE_SIZE);
  else if (_pageSize < 1) _pageSize = FALLBACK_PAGE_SIZE;
}

function _isInfinite() {
  return prefersInfiniteScroll('kani_library_pagination');
}

/** @type {AbortController|null} */ let _abort = null;
/** @type {(() => void)|null} */   let _removePullToRefresh = null;
/** @type {{ destroy: () => void }|null} */ let _shelfHandle = null;
/** @type {HTMLElement|null} */ let _savedSearchesEl = null;
/** @type {(() => void)|null} */   let _unsubRefresh = null;
/** @type {(() => void)|null} */   let _unsubInvalidation = null;
/** @type {(() => void)|null} */   let _unsubScanning = null;
/** @type {(() => void)|null} */   let _unsubDownloads = null;
/** @type {(() => void)|null} */   let _unsubScanResult = null;
/** True while a scan triggered from this page is in flight. */
let _scanInProgress = false;
/** @type {(() => void)|null} */   let _destroyPagination = null;
/** @type {(() => void)|null} */   let _destroyTabs = null;
/** @type {Map<number, { id: number, title: string, cover_url: string | null }>} */
const _mangaById = new Map();
/** @type {IntersectionObserver|null} */ let _sentinelObserver = null;

/** @type {HTMLElement|null} */ let _traceEl = null;

/**
 * One line per library response, behind `?debug=pagination`. KANI-44 does not
 * reproduce off-device, so the trace has to run where it does.
 * Remove with KANI-44.
 * @param {string} line
 */
function _tracePagination(line) {
  if (new URLSearchParams(location.search).get('debug') !== 'pagination') return;
  if (!_traceEl) {
    _traceEl = document.createElement('div');
    _traceEl.className = 'debug-panel';
    document.body.appendChild(_traceEl);
  }
  const row = document.createElement('div');
  row.textContent = line;
  _traceEl.appendChild(row);
  _traceEl.scrollTop = _traceEl.scrollHeight;
}
/** @type {HTMLElement|null} */    let _authorContainer = null;
/** @type {HTMLElement|null} */    let _artistContainer = null;
/** @type {HTMLElement|null} */    let _tagsContainer = null;
/** @type {HTMLElement|null} */    let _sourceContainer = null;
/** @type {HTMLElement[]} */       let _sortMountEls = [];
/** @type {HTMLElement|null} */    let _statusMountEl = null;
/** @type {HTMLElement|null} */    let _readingStatusMountEl = null;
/** @type {HTMLElement[]} */       let _displayMountEls = [];
/** @type {HTMLElement|null} */    let _gridEl = null;
/** @type {HTMLElement|null} */    let _paginEl = null;
/** @type {NodeListOf<HTMLInputElement>|null} */ let _searchEls = null;
/** @type {HTMLElement|null} */    let _container = null;
/** @type {(() => void)|null} */   let _mountComboboxesFn = null;
/** @type {(() => void)|null} */   let _renderFilterControlsFn = null;
/** @type {(() => void)|null} */   let _updateFilterCountFn = null;
/** @type {(() => void)|null} */   let _cancelInitSkeleton = null;

let _selectMode = false;
/** @type {Set<number>} */ let _selected = new Set();
/** @type {HTMLElement|null} */ let _bulkBarEl = null;
/** @type {(() => void)|null} */ let _closeCtxMenu = null;
/** @type {((e: KeyboardEvent) => void)|null} */ let _escHandler = null;
// One-shot flag: absorbs the click that fires on pointer-up after a long-press
// entered select mode. Set by the long-press timer; consumed by the first onCardClick.
let _absorbNextCardClick = false;
let _selectModeEnteredAt = 0;
const CONTEXT_MENU_AFTER_LONG_PRESS_GUARD_MS = 500;
/** @type {ReturnType<typeof setTimeout>|null} */ let _lpTimer = null;


function _hasActiveFilters() {
  return !!(_search || _statusFilter || _readingStatusFilter != null ||
    _hideNoUnread || _hideCompletedStatus || _tagFilter != null ||
    _authorFilter != null || _artistFilter != null || _sourceFilter != null);
}

function _clearAllFilters() {
  _search = '';
  _statusFilter = null;
  _readingStatusFilter = null;
  _hideNoUnread = false;
  _hideCompletedStatus = false;
  _tagFilter = null;
  _authorFilter = null;
  _artistFilter = null;
  _sourceFilter = null;
  _catFilter = null;
  _collectionFilter = null;
  _page = 1;
  if (_container) {
    for (const el of _container.querySelectorAll('.js-search')) /** @type {HTMLInputElement} */ (el).value = '';
    _mountComboboxesFn?.();
    _renderFilterControlsFn?.();
    _updateFilterCountFn?.();
  }
  _updateUrl();
  _fetchLibrary();
}

/** @param {HTMLElement} container */
export async function init(container) {
  _container = container;
  document.title = t('library.title');
  _tileSize = getTileSize();

  // Restore filter state from URL params
  _page       = parseInt(getParam('page') ?? '1', 10) || 1;
  _search     = getParam('search') ?? '';
  _statusFilter = getParam('status') || null;
  _tagFilter  = getParam('tag_id')    ? Number(getParam('tag_id'))    : null;
  _authorFilter = getParam('author_id') ? Number(getParam('author_id')) : null;
  _artistFilter = getParam('artist_id') ? Number(getParam('artist_id')) : null;
  _sourceFilter = getParam('source_id') ? Number(getParam('source_id')) : null;
  _catFilter        = getParam('cat_id')        ? Number(getParam('cat_id'))        : null;
  _collectionFilter = getParam('collection_id') ? Number(getParam('collection_id')) : null;
  _readingStatusFilter = getParam('reading_status') ? Number(getParam('reading_status')) : null;
  _hideNoUnread = getParam('hide_no_unread') === '1';
  _hideCompletedStatus = getParam('hide_completed') === '1';
  _sortOrder  = getParam('sort') ?? 'updated_desc';

  let refreshBtn = /** @type {HTMLButtonElement|null} */ (null);

  if (hasPermission('library:refresh')) {
    refreshBtn = document.createElement('button');
    refreshBtn.type = 'button';
    refreshBtn.className = 'btn-secondary btn-sm flex items-center gap-2';
    refreshBtn.innerHTML = `<span class="icon-sm shrink-0">${iconRefresh}</span><span>${t('library.refresh_all')}</span>`;
  }

  let scanAllBtn = /** @type {HTMLButtonElement|null} */ (null);
  if (hasPermission('library:refresh')) {
    scanAllBtn = document.createElement('button');
    scanAllBtn.type = 'button';
    scanAllBtn.className = 'btn-ghost btn-sm';
    scanAllBtn.textContent = t('library.scan_all');
    scanAllBtn.addEventListener('click', async () => {
      if (!scanAllBtn || scanAllBtn.disabled) return;
      scanAllBtn.disabled = true;
      _scanInProgress = true;
      try {
        await api.scanMangaMultiple('all');
        // Card spinners and completion toast are driven by SSE events.
        // Fallback: re-enable button if SSE never delivers 'completed' within 2 minutes.
        setTimeout(() => {
          if (scanAllBtn && scanAllBtn.disabled) {
            scanAllBtn.disabled = false;
            scanAllBtn.textContent = t('library.scan_all');
            _scanInProgress = false;
            setState('scanningMangaIds', new Set());
          }
        }, 120_000);
      } catch (e) {
        showApiError(e);
        scanAllBtn.disabled = false;
        scanAllBtn.textContent = t('library.scan_all');
        _scanInProgress = false;
        setState('scanningMangaIds', new Set());
      }
    });
  }

  const _hdrActions = /** @type {HTMLElement[]} */ ([]);
  if (scanAllBtn) _hdrActions.push(scanAllBtn);
  if (refreshBtn) _hdrActions.push(refreshBtn);
  setPageHeader({ crumbs: [{ label: t('library.nav.title') }], actions: _hdrActions.length ? _hdrActions : null });

  // Desktop and tablet fill the shell exactly: the tabs, filters and pager
  // hold their place and only the grid moves. Mobile scrolls the document.
  container.classList.add('page-fixed');

  container.innerHTML = `
    <div class="max-w-page mx-auto w-full px-3 sm:px-4 md:px-6 py-4 md:py-6 flex flex-col gap-4 page-body-host page-col">

      <!-- Category tabs (horizontal scroll on mobile) -->
      <div class="js-tabs shrink-0 overflow-x-auto [scrollbar-width:none] [-webkit-overflow-scrolling:touch]"></div>

      <!-- Search bar (mobile only — on desktop it lives inline in the controls row) -->
      <div class="js-search-slot-mobile lg:hidden"></div>

      <!-- Controls row: search (desktop) + Filters toggle + sort + page size + display -->
      <div class="flex items-center gap-2">
        <div class="js-search-slot-desktop hidden lg:block lg:flex-1"></div>
        <button
          type="button"
          class="js-filter-toggle input flex items-center justify-between gap-2 text-left w-full lg:w-auto flex-1 lg:flex-none"
          aria-expanded="false"
          aria-controls="library-filters"
        >
          <span class="flex items-center gap-2 text-sm font-medium">
            ${t('library.filters')}
            <span class="js-filter-count hidden items-center justify-center min-w-5 h-5 px-1.5 rounded-full bg-accent text-on-accent text-xs font-medium"></span>
          </span>
          <span class="icon-sm text-text-muted transition-transform js-filter-chevron">${iconChevronDown}</span>
        </button>
        <div class="js-sort-mount hidden lg:block shrink-0"></div>
        <div class="js-tile-size-mount hidden sm:block w-28 shrink-0"></div>
        <div class="js-display-mount shrink-0"></div>
      </div>

      <!-- Parking spot for the filter panel while its sheet is closed. -->
      <div class="js-filters-home hidden"></div>

      <!-- Filter panel — moved into a sheet when open, never shown in flow -->
      <div id="library-filters" class="js-filters hidden flex-col gap-2">
        <!-- Sort + page size. Inline in the controls row on desktop, so these
             mounts stay empty there. -->
        <div class="flex items-center gap-2 lg:hidden">
          <div class="js-sort-mount flex-1"></div>
          <div class="js-tile-size-mount w-28 shrink-0 sm:hidden"></div>
        </div>
        <!-- One column, uniform widths. The ragged mix of intrinsic and
             full-width pills had no edge to follow down either side. -->
        <div class="flex flex-col gap-2">
          <div class="js-status-mount w-full"></div>
          <div class="js-reading-status-mount w-full"></div>
          <div class="js-tags-combobox w-full"></div>
          <div class="js-author-combobox w-full"></div>
          <div class="js-artist-combobox w-full"></div>
          <div class="js-source-combobox w-full"></div>
          <div class="js-saved-searches flex items-center gap-2"></div>
        </div>
      </div>

      <!-- The scrolling region. The shelf scrolls with the grid rather than
           pinning: on a 720 px screen a pinned shelf would leave the grid a
           single row deep. -->
      <div class="js-scroll page-body flex flex-col gap-4 min-w-0">
        <!-- Continue Reading shelf (component owns its internals) -->
        <div class="js-shelf"></div>

        <!-- Grid -->
        <div class="js-grid" aria-live="polite" aria-busy="false"><div class="manga-grid"></div></div>
      </div>

      <!-- Pagination -->
      <div class="js-pagination"></div>
    </div>
  `;

  _gridEl          = /** @type {HTMLElement} */ (container.querySelector('.js-grid'));
  _paginEl         = /** @type {HTMLElement} */ (container.querySelector('.js-pagination'));
  _authorContainer = /** @type {HTMLElement} */ (container.querySelector('.js-author-combobox'));
  _artistContainer = /** @type {HTMLElement} */ (container.querySelector('.js-artist-combobox'));
  _tagsContainer   = /** @type {HTMLElement} */ (container.querySelector('.js-tags-combobox'));
  _sourceContainer = /** @type {HTMLElement} */ (container.querySelector('.js-source-combobox'));

  const tabsEl           = /** @type {HTMLElement} */ (container.querySelector('.js-tabs'));

  // Search fields (one per breakpoint, values kept in sync). Built via the
  // shared factory; the inputs keep the js-search class so the sync/clear-all
  // wiring below is unchanged.
  for (const slotSel of ['.js-search-slot-mobile', '.js-search-slot-desktop']) {
    const slot = /** @type {HTMLElement} */ (container.querySelector(slotSel));
    if (!slot) continue;
    const { el } = createSearchInput({
      value: _search,
      placeholder: t('library.search.placeholder'),
      ariaLabel: t('library.search.aria'),
      inputClass: 'js-search',
    });
    slot.appendChild(el);
  }
  const searchEls        = /** @type {NodeListOf<HTMLInputElement>} */ (container.querySelectorAll('.js-search'));
  _searchEls = searchEls;
  _sortMountEls         = [...container.querySelectorAll('.js-sort-mount')].map(el => /** @type {HTMLElement} */ (el));
  _statusMountEl        = /** @type {HTMLElement} */ (container.querySelector('.js-status-mount'));
  _readingStatusMountEl = /** @type {HTMLElement} */ (container.querySelector('.js-reading-status-mount'));
  _displayMountEls      = [...container.querySelectorAll('.js-display-mount')].map(el => /** @type {HTMLElement} */ (el));
  const sizeMountEls     = /** @type {NodeListOf<HTMLElement>} */ (container.querySelectorAll('.js-tile-size-mount'));
  const shelfEl          = /** @type {HTMLElement} */ (container.querySelector('.js-shelf'));
  const filterToggle    = /** @type {HTMLButtonElement} */ (container.querySelector('.js-filter-toggle'));
  const filtersEl       = /** @type {HTMLElement} */ (container.querySelector('.js-filters'));
  const filterCountEl   = /** @type {HTMLElement} */ (container.querySelector('.js-filter-count'));
  const filterChevronEl = /** @type {HTMLElement} */ (container.querySelector('.js-filter-chevron'));
  _gridEl.addEventListener('contextmenu', (e) => {
    const card = /** @type {HTMLElement|null} */ (/** @type {HTMLElement} */ (e.target)?.closest('[data-manga-id]'));
    if (!card) return;
    e.preventDefault();
    const id = parseInt(card.dataset.mangaId ?? '', 10);
    if (isNaN(id)) return;
    // A contextmenu emitted by the completed long-press must not toggle the new selection.
    if (_selectMode) {
      if (Date.now() - _selectModeEnteredAt < CONTEXT_MENU_AFTER_LONG_PRESS_GUARD_MS) return;
      _toggleMangaSelected(id, card);
      return;
    }
    const titleEl = card.querySelector('.title span');
    const title = titleEl?.textContent ?? '';
    _showMangaContextMenu({ id, title }, e.clientX, e.clientY, card);
  });

  // Long-press to enter select mode (touch / stylus / long right-hold)
  _gridEl.addEventListener('pointerdown', (e) => {
    if (_selectMode) return;
    const card = /** @type {HTMLElement|null} */ (/** @type {HTMLElement} */ (e.target)?.closest('[data-manga-id]'));
    if (!card) return;
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    _lpTimer = setTimeout(() => {
      _lpTimer = null;
      const id = parseInt(card.dataset.mangaId ?? '', 10);
      if (!isNaN(id)) {
        // Set flag before entering select mode so the click that fires on pointer-up
        // (immediately after this timer) is absorbed by onCardClick.
        _absorbNextCardClick = true;
        // Safety reset in case click never fires (e.g. touch + OS contextmenu cancels it)
        setTimeout(() => { _absorbNextCardClick = false; }, 300);
        _enterSelectMode();
        _toggleMangaSelected(id, card);
      }
    }, 400);
  });
  const _cancelLp = () => { if (_lpTimer != null) { clearTimeout(_lpTimer); _lpTimer = null; } };
  _gridEl.addEventListener('pointerup', _cancelLp);
  _gridEl.addEventListener('pointercancel', _cancelLp);
  _gridEl.addEventListener('pointermove', _cancelLp);

  // Escape leaves select mode (only when no dialog/popover is capturing it).
  _escHandler = (e) => {
    if (e.key === 'Escape' && _selectMode) { _exitSelectMode(); }
  };
  document.addEventListener('keydown', _escHandler);

  function _updateFilterCount() {
    if (!filterCountEl) return;
    const count = [_statusFilter, _readingStatusFilter != null ? true : null, _hideNoUnread || null, _hideCompletedStatus || null, _tagFilter != null ? true : null, _authorFilter != null ? true : null, _artistFilter != null ? true : null, _sourceFilter != null ? true : null].filter(Boolean).length;
    if (count > 0) {
      filterCountEl.textContent = String(count);
      filterCountEl.classList.remove('hidden');
      filterCountEl.classList.add('inline-flex');
    } else {
      filterCountEl.classList.add('hidden');
      filterCountEl.classList.remove('inline-flex');
    }
  }
  _updateFilterCountFn = _updateFilterCount;
  _updateFilterCount();

  // The panel opens as a sheet rather than expanding in flow: it overlays the
  // grid instead of displacing it, so the results stay visible while you filter.
  // Same surface at every breakpoint, per the design system.
  const filtersHomeEl = /** @type {HTMLElement} */ (container.querySelector('.js-filters-home'));

  function _setFiltersOpen(open) {
    if (open) {
      filtersEl.classList.remove('hidden');
      filtersEl.classList.add('flex');
    } else {
      // Re-parent before unmounting, or Preact takes the panel down with it.
      filtersHomeEl?.appendChild(filtersEl);
      filtersEl.classList.add('hidden');
      filtersEl.classList.remove('flex');
    }
    filterChevronEl?.classList.toggle('rotate-180', open);
    filterToggle?.setAttribute('aria-expanded', String(open));

    mountIntoModalRoot(open
      ? html`
        <${Modal}
          open=${true}
          sheet=${true}
          title=${t('library.filters')}
          onClose=${() => _setFiltersOpen(false)}
        >
          <div class="flex flex-col gap-2" ref=${(el) => { if (el) el.appendChild(filtersEl); }}></div>
        <//>
      `
      : null);
  }

  filterToggle?.addEventListener('click', () => {
    _setFiltersOpen(filterToggle.getAttribute('aria-expanded') !== 'true');
  });

  // Show skeleton only if data takes > 150 ms
  _syncPageSize();
  _cancelInitSkeleton = deferredSkeleton(() => {
    const grid = _mangaGridEl();
    if (grid) grid.innerHTML = skeletonGridItems(_pageSize);
  });
  // Infinite mode does not fit: its batch is fixed, and refetching on resize
  // would discard every batch already appended.
  if (!_isInfinite()) {
    _stopColumnWatch = observeCapacity(_gridEl, _mangaGridEl, _boundsEl, () => {
      _syncPageSize();
      _page = 1;
      _fetchLibrary();
    }, _pageSize);
  }

  const [tags, authors, artists, categories, collectionsRaw, sourcesRaw] = await Promise.allSettled([
    api.getTags(), api.getAuthors(), api.getArtists(), api.getCategories(),
    api.listCollections(), api.getSources(),
  ]).then(r => r.map(s => s.status === 'fulfilled' ? s.value : []));

  const catList = Array.isArray(categories) ? categories : [];
  const collectionList = Array.isArray(collectionsRaw) ? collectionsRaw : [];

  const tabItems = [
    { id: null, name: t('library.tab.all') },
    ...catList.map(c => ({ id: c.id, name: c.name })),
    ...collectionList.map(c => ({ id: -(c.id), name: `★ ${c.name}` })),
  ];

  function _activeTabId() {
    if (_collectionFilter != null) return -(_collectionFilter);
    return _catFilter;
  }

  function _onTabSelect(id) {
    if (id === null) {
      _catFilter = null;
      _collectionFilter = null;
    } else if (id < 0) {
      _catFilter = null;
      _collectionFilter = -(id);
    } else {
      _catFilter = id;
      _collectionFilter = null;
    }
    _page = 1;
    _updateUrl();
    tabsHandle.update(_activeTabId());
    _fetchLibrary();
  }

  const tabsHandle = renderCategoryTabs(tabsEl, {
    tabs: tabItems,
    activeId: _activeTabId(),
    onSelect: _onTabSelect,
  });
  _destroyTabs = tabsHandle.destroy;

  // Mount Preact Combobox for tags/author/artist
  const tagOptions    = (Array.isArray(tags)    ? tags    : []).map(t => ({ id: t.id ?? t, name: t.name ?? t }));
  const authorOptions = (Array.isArray(authors) ? authors : []).map(a => ({ id: a.id, name: a.name }));
  const artistOptions = (Array.isArray(artists) ? artists : []).map(a => ({ id: a.id, name: a.name }));
  const sourceOptions = (Array.isArray(sourcesRaw) ? sourcesRaw : []).map(src => ({ id: src.id, name: src.name }));

  function _mountComboboxes() {
    if (_tagsContainer) {
      render(html`<${Combobox}
        options=${tagOptions}
        value=${_tagFilter}
        onChange=${(id) => { _tagFilter = id; _page = 1; _mountComboboxes(); _updateFilterCount(); _updateUrl(); _fetchLibrary(); }}
        placeholder=${t('library.filter.all_tags')}
      />`, _tagsContainer);
    }
    if (_authorContainer) {
      render(html`<${Combobox}
        options=${authorOptions}
        value=${_authorFilter}
        onChange=${(id) => { _authorFilter = id; _page = 1; _mountComboboxes(); _updateFilterCount(); _updateUrl(); _fetchLibrary(); }}
        placeholder=${t('library.filter.author')}
      />`, _authorContainer);
    }
    if (_artistContainer) {
      render(html`<${Combobox}
        options=${artistOptions}
        value=${_artistFilter}
        onChange=${(id) => { _artistFilter = id; _page = 1; _mountComboboxes(); _updateFilterCount(); _updateUrl(); _fetchLibrary(); }}
        placeholder=${t('library.filter.artist')}
      />`, _artistContainer);
    }
    if (_sourceContainer) {
      render(html`<${Combobox}
        options=${sourceOptions}
        value=${_sourceFilter}
        onChange=${(id) => { _sourceFilter = id; _page = 1; _mountComboboxes(); _updateFilterCount(); _updateUrl(); _fetchLibrary(); }}
        placeholder=${t('library.filter.source')}
      />`, _sourceContainer);
    }
  }
  _mountComboboxesFn = _mountComboboxes;
  _mountComboboxes();

  _savedSearchesEl = /** @type {HTMLElement|null} */ (container.querySelector('.js-saved-searches'));
  if (_savedSearchesEl) mountSavedSearches(_savedSearchesEl, { getCurrentFilters: _currentFiltersForSavedSearch, onApply: _applySearchQuery });

  for (const searchEl of searchEls) {
    searchEl.addEventListener('input', debounce(() => {
      _search = searchEl.value.trim();
      for (const other of searchEls) { if (other !== searchEl) other.value = searchEl.value; }
      _page = 1;
      _updateUrl(true);
      _fetchLibrary();
    }, 300));
  }

  const SORT_OPTIONS = [
    ['updated_desc', t('library.sort.updated_desc')],
    ['updated_asc', t('library.sort.updated_asc')],
    ['name_asc', t('library.sort.name_asc')],
    ['name_desc', t('library.sort.name_desc')],
    ['added_desc', t('library.sort.added_desc')],
    ['added_asc', t('library.sort.added_asc')],
    ['score_desc', t('library.sort.score_desc')],
    ['score_asc', t('library.sort.score_asc')],
    ['last_read_desc', t('library.sort.last_read_desc')],
  ].map(([value, label]) => ({ value, label }));

  const STATUS_OPTIONS = ['', 'ongoing', 'completed', 'hiatus', 'cancelled', 'unknown'].map(v => ({
    value: v,
    label: v ? t(`manga.status.${v}`) : t('library.status.all'),
  }));

  const READING_STATUS_OPTIONS = [
    ['', t('library.reading_status.all')],
    ['0', t('library.reading_status.reading')],
    ['1', t('library.reading_status.on_hold')],
    ['2', t('library.reading_status.dropped')],
    ['3', t('library.reading_status.plan_to_read')],
    ['4', t('library.reading_status.completed')],
    ['5', t('library.reading_status.rereading')],
  ].map(([value, label]) => ({ value, label }));

  function _renderFilterControls() {
    for (const el of _sortMountEls) {
      render(html`<${Select}
        options=${SORT_OPTIONS}
        value=${_sortOrder}
        ariaLabel=${t('library.sort.aria')}
        onChange=${(/** @type {string} */ v) => { _sortOrder = v; _page = 1; _renderFilterControls(); _updateUrl(); _fetchLibrary(); }}
      />`, el);
    }
    if (_statusMountEl) {
      render(html`<${Select}
        options=${STATUS_OPTIONS}
        value=${_statusFilter ?? ''}
        ariaLabel=${t('library.status.aria')}
        onChange=${(/** @type {string} */ v) => { _statusFilter = v || null; _page = 1; _renderFilterControls(); _updateFilterCount(); _updateUrl(); _fetchLibrary(); }}
      />`, _statusMountEl);
    }
    if (_readingStatusMountEl) {
      render(html`<${Select}
        options=${READING_STATUS_OPTIONS}
        value=${_readingStatusFilter != null ? String(_readingStatusFilter) : ''}
        ariaLabel=${t('library.reading_status.aria')}
        onChange=${(/** @type {string} */ v) => { _readingStatusFilter = v ? Number(v) : null; _page = 1; _renderFilterControls(); _updateFilterCount(); _updateUrl(); _fetchLibrary(); }}
      />`, _readingStatusMountEl);
    }
    for (const el of _displayMountEls) {
      mountDisplayMenu(el, {
        hideRead: _hideNoUnread,
        hideCompleted: _hideCompletedStatus,
        onChangeHideRead: (/** @type {boolean} */ v) => { _hideNoUnread = v; _page = 1; _renderFilterControls(); _updateFilterCount(); _updateUrl(); _fetchLibrary(); },
        onChangeHideCompleted: (/** @type {boolean} */ v) => { _hideCompletedStatus = v; _page = 1; _renderFilterControls(); _updateFilterCount(); _updateUrl(); _fetchLibrary(); },
      });
    }
  }
  _renderFilterControlsFn = _renderFilterControls;
  _renderFilterControls();

  const _renderTileSize = () => {
    for (const mountEl of sizeMountEls) {
      render(html`<${TileSizeSelect}
        value=${_tileSize}
        onChange=${(/** @type {import('../tile-size.js').TileSize} */ size) => {
          _tileSize = size;
          setTileSize(size);
          // The new track width only exists after the browser has laid the grid
          // out again, so measure on the next frame rather than this one.
          requestAnimationFrame(() => {
            _syncPageSize();
            _page = 1;
            _renderTileSize();
            _fetchLibrary();
          });
        }}
      />`, mountEl);
    }
  };
  _renderTileSize();

  refreshBtn?.addEventListener('click', async () => {
    if (refreshBtn) refreshBtn.disabled = true;
    try {
      await api.startRefreshAll();
    } catch (e) {
      if (refreshBtn) refreshBtn.disabled = false;
      showApiError(e);
    }
  });


  function _applyRefreshState(state) {
    const isRunning = state.type === 'running';
    const pct = isRunning && state.total > 0 ? Math.round((state.completed / state.total) * 100) : 0;

    if (_scanInProgress) {
      if (scanAllBtn) {
        if (isRunning) {
          const label = pct > 0 ? `${pct}%` : t('library.scanning');
          scanAllBtn.innerHTML = `<span class="icon-sm shrink-0 animate-spin">${iconRefresh}</span><span>${label}</span>`;
        }
      }
      if (refreshBtn) {
        refreshBtn.disabled = false;
        refreshBtn.innerHTML = `<span class="icon-sm shrink-0">${iconRefresh}</span><span>${t('library.refresh_all')}</span>`;
      }
    } else {
      if (!refreshBtn) return;
      if (isRunning) {
        refreshBtn.disabled = true;
        const label = pct > 0 ? `${pct}%` : t('library.refreshing');
        refreshBtn.innerHTML = `<span class="icon-sm shrink-0 animate-spin">${iconRefresh}</span><span>${label}</span>`;
      } else {
        refreshBtn.disabled = false;
        refreshBtn.innerHTML = `<span class="icon-sm shrink-0">${iconRefresh}</span><span>${t('library.refresh_all')}</span>`;
      }
    }
  }

  _unsubRefresh = subscribe('refreshState', _applyRefreshState);
  _applyRefreshState(getState('refreshState'));

  let _lastInvalidation = getState('libraryInvalidation');
  _unsubInvalidation = subscribe('libraryInvalidation', (val) => {
    if (val !== _lastInvalidation) {
      _lastInvalidation = val;
      _fetchLibrary();
    }
  });

  let _prevScanningIds = /** @type {Set<number>} */ (new Set());
  _unsubScanning = subscribe('scanningMangaIds', (/** @type {Set<number>} */ ids) => {
    if (!_gridEl) return;
    for (const id of ids) {
      if (!_prevScanningIds.has(id)) setMangaCardScanning(id, true, _gridEl);
    }
    for (const id of _prevScanningIds) {
      if (!ids.has(id)) setMangaCardScanning(id, false, _gridEl);
    }
    _prevScanningIds = ids;
  });

  _unsubDownloads = subscribe('chaptersProgress', (/** @type {Map<number, import('../cache.js').ChapterProgress>} */ map) => {
    if (!_gridEl) return;
    /** @type {Map<number, { completed: number, total: number }>} */
    const byManga = new Map();
    for (const ch of map.values()) {
      if (ch.status !== 'in_progress') continue;
      const cur = byManga.get(ch.mangaId) ?? { completed: 0, total: 0 };
      byManga.set(ch.mangaId, {
        completed: cur.completed + ch.completedPages,
        total: cur.total + ch.totalPages,
      });
    }
    for (const card of /** @type {NodeListOf<HTMLElement>} */ (_gridEl.querySelectorAll('[data-manga-id]'))) {
      const id = Number(card.dataset.mangaId);
      const prog = byManga.get(id);
      if (prog && prog.total > 0) {
        setMangaCardDownloadProgress(id, Math.round((prog.completed / prog.total) * 100), _gridEl);
      } else {
        setMangaCardDownloadProgress(id, null, _gridEl);
      }
    }
  });

  _unsubScanResult = subscribe('scanResult', (/** @type {{ total: number, failed: number, newChapters: number, perManga: Map<number, number> } | null} */ result) => {
    if (!result || !_scanInProgress) return;
    _scanInProgress = false;
    if (scanAllBtn) {
      scanAllBtn.disabled = false;
      scanAllBtn.textContent = t('library.scan_all');
    }

    const parts = [t('library.scan.result', { count: result.total })];
    if (result.newChapters > 0) parts.push(t('library.scan.new_chapters', { count: result.newChapters }));
    else parts.push(t('library.scan.no_new_chapters'));
    if (result.failed > 0) parts.push(t('library.scan.failed', { count: result.failed }));
    showToast(parts.join(' — '), { type: result.failed > 0 ? 'warn' : 'success' });

    if (_gridEl && result.perManga.size > 0) {
      for (const [mangaId, count] of result.perManga) {
        setNewChapterCount(mangaId, count, _gridEl);
      }
    }
  });

  if (shelfEl) {
    _shelfHandle = mountContinueShelf(shelfEl, {
      loadItems: () => api.getContinueReadingShelf(12),
    });
  }

  _fetchLibrary();

  // Refreshing has to reset the page: _fetchLibrary appends while _page > 1, so
  // calling it bare re-appends whatever page the list happens to be on.
  _removePullToRefresh = addPullToRefresh(
    document.getElementById('page-content') ?? document.documentElement,
    () => { _page = 1; _fetchLibrary(); },
  );
}


function _updateUrl(replace = false) {
  const params = {
    page:            _page > 1                    ? _page          : null,
    search:          _search                      || null,
    status:          _statusFilter                || null,
    tag_id:          _tagFilter                   ?? null,
    author_id:       _authorFilter                ?? null,
    artist_id:       _artistFilter                ?? null,
    source_id:       _sourceFilter                ?? null,
    cat_id:          _catFilter                   ?? null,
    collection_id:   _collectionFilter            ?? null,
    reading_status:  _readingStatusFilter != null ? _readingStatusFilter : null,
    hide_no_unread:  _hideNoUnread                ? '1' : null,
    hide_completed:  _hideCompletedStatus         ? '1' : null,
    sort:            _sortOrder && _sortOrder !== 'updated_desc' ? _sortOrder : null,
  };
  if (replace) replaceState(params);
  else pushState(params);
}


function _fetchLibrary() {
  if (!_gridEl || !_paginEl) return;
  const infinite = _isInfinite();
  const isAppend = infinite && _page > 1;
  _placePagination(infinite);

  _abort?.abort();
  _abort = new AbortController();

  _sentinelObserver?.disconnect();
  _sentinelObserver = null;
  _destroyPagination?.();
  _destroyPagination = null;
  _paginEl.innerHTML = '';

  if (isAppend) {
    _paginEl.innerHTML = '<div class="h-14 mx-3 my-2 skeleton rounded-lg"></div>';
  } else {
    _gridEl.classList.add('opacity-50', 'pointer-events-none');
  }
  startLoading();

  api.getLibrary({
    page: _page,
    page_size: _pageSize,
    search: _search || undefined,
    status_filter: _statusFilter ? STATUS_VALUES[_statusFilter] : undefined,
    reading_status_filter: _readingStatusFilter ?? undefined,
    hide_no_unread: _hideNoUnread || undefined,
    hide_completed_status: _hideCompletedStatus || undefined,
    tag_filter: _tagFilter ?? undefined,
    author_filter: _authorFilter ?? undefined,
    artist_filter: _artistFilter ?? undefined,
    source_id: _sourceFilter ?? undefined,
    category_filter: _catFilter ?? undefined,
    collection_id: _collectionFilter ?? undefined,
    sort_by: _sortOrder,
  }, _abort.signal).then(result => {
    if (!_gridEl || !_paginEl) return;
    finishLoading();
    _paginEl.innerHTML = '';

    const items = Array.isArray(result?.items) ? result.items
      : Array.isArray(result?.manga)            ? result.manga
      : Array.isArray(result)                   ? result
      : [];
    for (const m of items) _mangaById.set(Number(m.id), { id: Number(m.id), title: m.title, cover_url: m.cover_url ?? null });

    // Clear old content only once new data is ready — prevents blank-flash flicker
    _cancelInitSkeleton?.();
    _cancelInitSkeleton = null;
    if (!isAppend) _gridEl.innerHTML = '';

    if (items.length === 0 && !isAppend) {
      const hasFilters = _hasActiveFilters();
      const hasCategoryOnly = !hasFilters && _catFilter != null;
      let emptyOpts;
      if (hasCategoryOnly) {
        emptyOpts = { icon: iconBookOpen, title: t('library.empty.category') };
      } else if (hasFilters || _catFilter != null) {
        emptyOpts = { icon: iconBookOpen, title: t('library.empty.filters'), subtitle: t('library.empty.filters.subtitle'), action: { label: t('library.empty.filters.action'), onClick: _clearAllFilters } };
      } else {
        emptyOpts = { icon: iconBookOpen, title: t('library.empty.title'), subtitle: t('library.empty.subtitle'), action: { label: t('library.empty.action'), href: '/sources' } };
      }
      _gridEl.appendChild(createEmptyState(emptyOpts));
    } else if (items.length > 0) {
      if (infinite) {
        _appendMangaCards(_gridEl, items);
      } else {
        renderMangaGrid(_gridEl, {
          items: items.map(m => ({ id: m.id, title: m.title, cover_image_url: m.cover_url ?? null, new_chapter_count: m.new_chapter_count ?? 0, resume: m.resume ?? null, match_field: m.match_field ?? null, match_text: m.match_text ?? null, import_link_status: m.import_link_status ?? null, is_orphaned: m.is_orphaned ?? false })),
          getHref: (m) => `/manga/${m.id}`,
          getCaption: _matchCaption,
          onCardClick: (m) => {
            const cardEl = /** @type {HTMLElement} */ (_gridEl.querySelector(`[data-manga-id="${m.id}"]`));
            if (cardEl) _onCardClick(m, cardEl);
          },
          onMenuClick: (m, btn) => {
            const cardEl = /** @type {HTMLElement} */ (btn.closest('[data-manga-id]'));
            if (_selectMode) { _toggleMangaSelected(m.id, cardEl); return; }
            const rect = btn.getBoundingClientRect();
            _showMangaContextMenu({ id: m.id, title: m.title }, rect.left, rect.bottom + 4, cardEl);
          },
          onResumeClick: _onResumeClick,
        });
      }
    }

    // Remove dim only after new content is in place
    _gridEl.classList.remove('opacity-50', 'pointer-events-none');

    const hasNext = hasNextPage(result, items.length, _pageSize);
    const rendered = [...(_gridEl?.querySelectorAll('[data-manga-id]') ?? [])]
      .map((el) => /** @type {HTMLElement} */ (el).dataset.mangaId);
    _tracePagination(
      `p${_page} size${_pageSize} got${items.length} next=${hasNext}`
      + ` cards=${rendered.length} dup=${rendered.length - new Set(rendered).size}`
      + ` first=${items[0]?.id ?? '-'} last=${items[items.length - 1]?.id ?? '-'}`,
    );
    if (infinite) {
      _setupSentinel(hasNext);
    } else if (_page > 1 || hasNext) {
      const { destroy } = renderPagination(_paginEl, {
        page: _page,
        hasNext,
        total: result?.total_pages ?? undefined,
        onPageChange: (p) => { _page = p; _updateUrl(); _fetchLibrary(); scrollPageTop(); },
      });
      _destroyPagination = destroy;
    }
  }).catch(e => {
    _cancelInitSkeleton?.();
    _cancelInitSkeleton = null;
    if (e?.name === 'AbortError') return;
    if (!_gridEl) return;
    finishLoading();
    _paginEl.innerHTML = '';
    if (!isAppend) {
      _gridEl.classList.remove('opacity-50', 'pointer-events-none');
      _gridEl.innerHTML = '';
      _gridEl.appendChild(createErrorState({
        message: t('library.error.load'),
        onRetry: () => _fetchLibrary(),
      }));
    }
  });
}

/** Appends manga cards to the persistent grid inside `_gridEl`. */
function _appendMangaCards(gridEl, items) {
  let grid = /** @type {HTMLElement|null} */ (gridEl.querySelector('.manga-grid'));
  if (!grid) {
    grid = document.createElement('div');
    grid.className = 'manga-grid';
    gridEl.appendChild(grid);
  }
  for (const m of items) {
    const card = createMangaCard({
      manga: { id: m.id, title: m.title, cover_image_url: m.cover_url ?? null, new_chapter_count: m.new_chapter_count ?? 0, resume: m.resume ?? null, import_link_status: m.import_link_status ?? null, is_orphaned: m.is_orphaned ?? false },
      href: `/manga/${m.id}`,
      onCardClick: (manga) => {
        const cardEl = /** @type {HTMLElement} */ (gridEl.querySelector(`[data-manga-id="${manga.id}"]`));
        if (cardEl) _onCardClick(manga, cardEl);
      },
      onMenuClick: (manga, btn) => {
        const cardEl = /** @type {HTMLElement} */ (btn.closest('[data-manga-id]'));
        if (_selectMode) { _toggleMangaSelected(manga.id, cardEl); return; }
        const rect = btn.getBoundingClientRect();
        _showMangaContextMenu({ id: manga.id, title: manga.title }, rect.left, rect.bottom + 4, cardEl);
      },
      onResumeClick: _onResumeClick,
    });
    grid.appendChild(mangaGridCell(card, _matchCaption(m)));
  }
}

/** @param {{ resume?: { chapter_id: number } | null }} manga */
function _onResumeClick(manga) {
  if (manga.resume) navigate(`/reader/${manga.resume.chapter_id}`);
}

/**
 * Puts the pagination slot on the right side of the scroll boundary.
 *
 * A pager is chrome and pins below the grid. The infinite-scroll sentinel
 * lives in the same slot, and a pinned sentinel is permanently on screen —
 * the observer would fire, fetch, and fire again until it had loaded every
 * page in the library. In infinite mode the slot goes back inside the
 * scroller, where reaching it means the reader actually reached the end.
 *
 * @param {boolean} infinite
 */
function _placePagination(infinite) {
  if (!_paginEl) return;
  const scroller = _paginEl.closest('.page-col')?.querySelector('.js-scroll');
  const wanted = infinite ? scroller : scroller?.parentElement;
  if (wanted && _paginEl.parentElement !== wanted) wanted.appendChild(_paginEl);
}

/** Sets up (or clears) the IntersectionObserver sentinel for infinite scroll. */
function _setupSentinel(hasNext) {
  _sentinelObserver?.disconnect();
  _sentinelObserver = null;
  if (!_paginEl || !hasNext) return;

  const sentinel = document.createElement('div');
  sentinel.className = 'js-sentinel h-px';
  _paginEl.appendChild(sentinel);

  _sentinelObserver = new IntersectionObserver((entries) => {
    if (entries[0].isIntersecting) {
      _sentinelObserver?.disconnect();
      _sentinelObserver = null;
      _page++;
      _fetchLibrary();
    }
  }, { rootMargin: '200px' });
  _sentinelObserver.observe(sentinel);
}


/**
 * Per-card click handler — handles both navigation (normal mode) and selection toggling
 * (select mode). Using per-card callbacks means the click is processed directly on the
 * <a> element, which is completely isolated from the context-menu button's event path,
 * eliminating any ghost-click interaction.
 * @param {{ id: number, title: string }} manga
 * @param {HTMLElement} cardEl
 */
function _onCardClick(manga, cardEl) {
  // Absorb the click that fires on pointer-up after a long-press that entered select mode
  if (_absorbNextCardClick) { _absorbNextCardClick = false; return; }
  if (_selectMode) {
    _toggleMangaSelected(manga.id, cardEl);
  } else {
    navigate('/manga/' + manga.id);
  }
}

function _enterSelectMode() {
  _selectModeEnteredAt = Date.now();
  _selectMode = true;
  _selected.clear();
  _gridEl?.classList.add('cursor-pointer', 'select-none', 'is-select-mode');
  if (!_bulkBarEl) {
    _bulkBarEl = document.createElement('div');
    document.body.appendChild(_bulkBarEl);
  }
  _renderBulkBar();
}

function _exitSelectMode() {
  _selectMode = false;
  _selected.clear();
  _gridEl?.querySelectorAll('[data-manga-id]').forEach(card => {
    card.classList.remove('ring-2', 'ring-accent');
    card.querySelector('.js-select-overlay')?.remove();
  });
  _gridEl?.classList.remove('cursor-pointer', 'select-none', 'is-select-mode');
  if (_bulkBarEl) render(null, _bulkBarEl);
  _bulkBarEl?.remove();
  _bulkBarEl = null;
}

/**
 * @param {number} id
 * @param {HTMLElement} cardEl
 */
function _toggleMangaSelected(id, cardEl) {
  const isSelected = _selected.has(id);
  if (isSelected) {
    _selected.delete(id);
    cardEl.classList.remove('ring-2', 'ring-accent');
    cardEl.querySelector('.js-select-overlay')?.remove();
  } else {
    _selected.add(id);
    cardEl.classList.add('ring-2', 'ring-accent', 'rounded-sm');
    if (!cardEl.querySelector('.js-select-overlay')) {
      const overlay = document.createElement('div');
      overlay.className = 'js-select-overlay absolute top-1 right-1 w-5 h-5 bg-accent rounded-full flex items-center justify-center text-on-accent text-xs font-bold pointer-events-none z-10';
      overlay.innerHTML = iconCheck;
      overlay.classList.add('icon-2xs');
      const coverWrap = cardEl.querySelector('.relative');
      if (coverWrap) /** @type {HTMLElement} */ (coverWrap).appendChild(overlay);
    }
  }
  _renderBulkBar();
}

function _renderBulkBar() {
  if (!_bulkBarEl) return;

  const _onSelectAll = () => {
    const cards = /** @type {NodeListOf<HTMLElement>} */ (_gridEl?.querySelectorAll('[data-manga-id]') ?? []);
    for (const card of cards) {
      const id = parseInt(card.dataset.mangaId ?? '', 10);
      if (!isNaN(id) && !_selected.has(id)) _toggleMangaSelected(id, card);
    }
  };

  const _onDownload = async () => {
    const ids = [..._selected];
    let done = 0;
    for (const id of ids) {
      try { await api.downloadAll(id); } catch { }
      done++;
    }
    showToast(t('library.bulk.toast.downloaded', { count: done }));
    _exitSelectMode();
  };

  const _onScan = async () => {
    const ids = [..._selected].map(Number);
    _scanInProgress = true;
    try {
      // withBusy disables all bulk actions for the duration and restores them after.
      await withBusy(/** @type {HTMLElement} */ (_bulkBarEl).querySelectorAll('.js-bulk-action'), () => api.scanMangaMultiple(ids));
      // Card spinners and completion toast driven by SSE events.
      // Exit select mode so the user can see scan progress on cards.
      _exitSelectMode();
    } catch (e) {
      showApiError(e);
      _scanInProgress = false;
      setState('scanningMangaIds', new Set());
    }
  };

  const _onMarkRead = async () => {
    const ids = [..._selected];
    let done = 0;
    for (const id of ids) {
      try { await api.markChaptersUpTo(id, 99999, true); done++; } catch { }
    }
    showToast(t('library.bulk.toast.read', { count: done }));
    _exitSelectMode();
  };

  const _onMarkUnread = async () => {
    const ids = [..._selected];
    let done = 0;
    for (const id of ids) {
      try { await api.markChaptersUpTo(id, 99999, false); done++; } catch { }
    }
    showToast(t('library.bulk.toast.unread', { count: done }));
    _exitSelectMode();
  };

  const _onCategories = () => {
    _showBulkCategoryModal([..._selected]);
  };

  const _onDelete = async () => {
    const count = _selected.size;
    const ok = await showConfirm(t('library.remove.message.bulk', { count }), { title: t('library.remove.title'), confirmLabel: t('library.remove.confirm'), danger: true });
    if (!ok) return;
    const ids = [..._selected];
    _exitSelectMode();
    let done = 0;
    for (const id of ids) {
      try { await api.deleteManga(id); done++; } catch { }
    }
    showToast(t('library.bulk.toast.deleted', { count: done }));
    _page = 1;
    _fetchLibrary();
  };

  const _onMigrate = () => {
    const manga = [..._selected].map(Number).map(id => _mangaById.get(id) ?? { id, title: String(id), cover_url: null });
    _exitSelectMode();
    openBulkMigrationDialogue({ manga, onMigrated: () => { _page = 1; _fetchLibrary(); } });
  };

  const hasSelection = _selected.size > 0;
  render(html`<${BulkBar}
    countLabel=${t('library.bulk.selected', { count: _selected.size })}
    helpers=${[{ label: t('library.bulk.all'), title: t('library.bulk.select_all'), onClick: _onSelectAll }]}
    actions=${[
      { label: t('library.bulk.download'), title: t('library.bulk.download.title'), onClick: _onDownload, disabled: !hasSelection },
      { label: t('library.bulk.scan'), title: t('library.bulk.scan.title'), onClick: _onScan, disabled: !hasSelection },
      { label: t('library.bulk.mark_read'), onClick: _onMarkRead, disabled: !hasSelection },
      { label: t('library.bulk.mark_unread'), onClick: _onMarkUnread, disabled: !hasSelection },
      { label: t('library.bulk.categories'), onClick: _onCategories, disabled: !hasSelection },
      ...(hasPermission('library:manage') ? [{ label: t('library.bulk.migrate'), title: t('library.bulk.migrate.title'), onClick: _onMigrate, disabled: !hasSelection }] : []),
      { label: t('library.bulk.delete'), kind: 'danger', onClick: _onDelete, disabled: !hasSelection },
    ]}
    onCancel=${_exitSelectMode}
  />`, _bulkBarEl);
}

/**
 * @param {{ id: number, title: string }} manga
 * @param {number} x
 * @param {number} y
 * @param {HTMLElement} cardEl
 */
function _showMangaContextMenu(manga, x, y, cardEl) {
  _closeContextMenu();

  /** @type {import('../components/menu.js').MenuItem[]} */
  const items = [
    { label: t('library.menu.select'), action: () => {
      if (!_selectMode) _enterSelectMode();
      if (!_selected.has(manga.id)) _toggleMangaSelected(manga.id, cardEl);
    }},
    { divider: true },
    ...(hasPermission('chapter:download') ? [{ label: t('library.menu.download_all'), action: async () => {
      try { await api.downloadAll(manga.id); showToast(t('library.toast.download_queued')); }
      catch { showToast(t('library.toast.download_failed')); }
    }}] : []),
    { label: t('library.menu.mark_all_read'), action: async () => {
      try { await api.markChaptersUpTo(manga.id, 99999, true); showToast(t('library.toast.marked_read')); }
      catch { showToast(t('common.error.failed')); }
    }},
    { label: t('library.menu.mark_all_unread'), action: async () => {
      try { await api.markChaptersUpTo(manga.id, 99999, false); showToast(t('library.toast.marked_unread')); }
      catch { showToast(t('common.error.failed')); }
    }},
    { label: t('library.menu.set_categories'), action: () => _showBulkCategoryModal([manga.id]) },
    { divider: true },
    { label: t('library.menu.remove'), danger: true, action: async () => {
      const ok = await showConfirm(t('library.remove.message', { title: manga.title }), { title: t('library.remove.title'), confirmLabel: t('library.remove.confirm'), danger: true });
      if (!ok) return;
      try {
        await api.deleteManga(manga.id);
        showToast(t('library.toast.removed'));
        _page = 1;
        _fetchLibrary();
      } catch { showToast(t('library.toast.remove_failed')); }
    }},
  ];

  _closeCtxMenu = showContextMenu(items, { x, y });
}

function _closeContextMenu() {
  _closeCtxMenu?.();
  _closeCtxMenu = null;
}

/** @param {number[]} mangaIds */
function _showBulkCategoryModal(mangaIds) {
  showCategoryAssignModal(mangaIds, {
    onApplied: () => {
      _exitSelectMode();
      _page = 1;
      _fetchLibrary();
    },
  });
}


/** @param {HTMLElement} container */
export function destroy(container) {
  clearPageHeader();
  // The filter panel may be parented into the sheet; take the sheet down first
  // so it is not left mounted over the next page.
  mountIntoModalRoot(null);
  _abort?.abort();
  _abort = null;
  _removePullToRefresh?.();
  _removePullToRefresh = null;
  _shelfHandle?.destroy();
  _shelfHandle = null;
  _unsubRefresh?.();
  _unsubRefresh = null;
  _unsubInvalidation?.();
  _unsubInvalidation = null;
  _unsubScanning?.();
  _unsubScanning = null;
  _unsubDownloads?.();
  _unsubDownloads = null;
  _unsubScanResult?.();
  _unsubScanResult = null;
  _destroyPagination?.();
  _destroyPagination = null;
  _sentinelObserver?.disconnect();
  _sentinelObserver = null;
  _destroyTabs?.();
  _destroyTabs = null;
  _mangaById.clear();
  if (_tagsContainer)   render(null, _tagsContainer);
  if (_authorContainer) render(null, _authorContainer);
  if (_artistContainer) render(null, _artistContainer);
  for (const el of _sortMountEls) render(null, el);
  if (_statusMountEl)        render(null, _statusMountEl);
  if (_readingStatusMountEl) render(null, _readingStatusMountEl);
  for (const el of _displayMountEls) render(null, el);
  if (_savedSearchesEl)      render(null, _savedSearchesEl);
  _savedSearchesEl = null;
  _tagsContainer   = null;
  _sourceContainer = null;
  _authorContainer = null;
  _artistContainer = null;
  _sortMountEls = [];
  _statusMountEl = null;
  _readingStatusMountEl = null;
  _displayMountEls = [];
  _stopColumnWatch?.();
  _stopColumnWatch = null;
  _gridEl = null;
  _paginEl = null;
  _searchEls = null;
  _container = null;
  _mountComboboxesFn = null;
  _renderFilterControlsFn = null;
  _updateFilterCountFn = null;
  _cancelInitSkeleton?.();
  _cancelInitSkeleton = null;
  if (_bulkBarEl) render(null, _bulkBarEl);
  _bulkBarEl?.remove();
  _bulkBarEl = null;
  _closeContextMenu();
  if (_escHandler) { document.removeEventListener('keydown', _escHandler); _escHandler = null; }
  _selectMode = false;
  _selected.clear();
  container.innerHTML = '';
}


/** @param {string} queryJson */
function _applySearchQuery(queryJson) {
  try {
    const q = JSON.parse(queryJson);
    _search           = q.search ?? '';
    _statusFilter     = q.status_filter != null ? Object.keys({ ongoing: 0, completed: 1, hiatus: 2, cancelled: 3, unknown: 4 }).find(k => ({ ongoing: 0, completed: 1, hiatus: 2, cancelled: 3, unknown: 4 }[k] === q.status_filter) ?? null) ?? null : null;
    _readingStatusFilter = q.reading_status_filter ?? null;
    _hideNoUnread     = q.hide_no_unread ?? false;
    _hideCompletedStatus = q.hide_completed_status ?? false;
    _tagFilter        = q.tag_filter ?? null;
    _authorFilter     = q.author_filter ?? null;
    _artistFilter     = q.artist_filter ?? null;
    _sourceFilter     = q.source_id ?? null;
    _catFilter        = q.category_filter ?? null;
    _collectionFilter = null;
    _page = 1;
    if (_searchEls) for (const el of _searchEls) el.value = _search;
    _mountComboboxesFn?.();
    _renderFilterControlsFn?.();
    _updateFilterCountFn?.();
    _updateUrl(true);
    _fetchLibrary();
  } catch { }
}

function _currentFiltersForSavedSearch() {
  return {
    search: _search || undefined,
    status_filter: _statusFilter ? ({ ongoing: 0, completed: 1, hiatus: 2, cancelled: 3, unknown: 4 }[_statusFilter]) : undefined,
    reading_status_filter: _readingStatusFilter ?? undefined,
    hide_no_unread: _hideNoUnread || undefined,
    hide_completed_status: _hideCompletedStatus || undefined,
    tag_filter: _tagFilter ?? undefined,
    author_filter: _authorFilter ?? undefined,
    artist_filter: _artistFilter ?? undefined,
    source_id: _sourceFilter ?? undefined,
    category_filter: _catFilter ?? undefined,
  };
}
