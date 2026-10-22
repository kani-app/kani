// @ts-check

import { h, render } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { hasPermission } from '../session.js';
import { navigate } from '../router.js';
import { getLocal, getLocalInt, setLocal, formatChapterTitle, hasNextPage, isChapterDownloaded } from '../utils.js';
import { replaceState } from '../url-params.js';
import { VirtualChapterList, readChapterRowHeight } from '../components/virtual-chapter-list.js';
import { renderPagination } from '../components/pagination.js';
import { Select } from '../components/form/select.js';
import { PageSizeSelect } from '../components/page-size-select.js';
import { skeletonMangaHero } from '../components/skeletons.js';
import { startLoading, finishLoading } from '../components/page-loading-bar.js';
import { createErrorState } from '../components/error-state.js';
import { createEmptyState } from '../components/empty-state.js';
import { mountMigrationDialogue } from '../components/migration-dialogue.js';
import { setPageHeader, clearPageHeader } from '../components/app-header.js';
import { renderTabs } from '../components/tabs.js';
import { ContextMenu } from '../components/menu.js';
import { showToast, showApiError } from '../components/toast.js';
import { Modal, mountIntoModalRoot, showConfirm } from '../components/modal.js';
import { iconDocument, iconX} from '../icons.js';
import { getCachedChapterIds, onChapterCached } from '../offline.js';
import { mountMangaHeader } from '../components/manga-details/manga-header.js';
import { mountLibrarySettingsPanel } from '../components/manga-details/library-settings-panel.js';
import { mountMetadataPanel } from '../components/manga-details/metadata-panel.js';
import { mountTrackerPanel } from '../components/manga-details/tracker-panel.js';
import { mountVolumesPanel } from '../components/manga-details/volumes-panel.js';
import { mountCategoryPicker } from '../components/manga-details/category-picker.js';
import { mountDownloadRulesPanel } from '../components/manga-details/download-rules-panel.js';
import { mountScanlatorPrefsPanel } from '../components/manga-details/scanlator-prefs-panel.js';
import { mountSuppressedBanner } from '../components/manga-details/suppressed-banner.js';
import { mountImportLinkBanner } from '../components/manga-details/import-link-banner.js';
import { mkSectionHeader, mkCard, mkTitledCard, mkRow, mkItem } from '../components/manga-details/_shared.js';
import { subscribeJob } from '../sse.js';
import { t } from '../i18n.js';
import { prefersInfiniteScroll } from '../pagination-mode.js';
const html = htm.bind(h);


/** Opens the migration dialogue for the manga on screen. */
function _openMigrationDialogue() {
  if (!_sid || !_dbId) return;
  _unmountMigration = mountMigrationDialogue({
    dbId: _dbId,
    currentSourceId: _sid,
    currentSourceName: _mangaData?.source_name ?? '',
    currentTitle: _mangaData?.title ?? '',
    currentCoverUrl: api.getMangaCoverUrl(_dbId, 'lg'),
    onComplete: (newSid, newMid) => { _unmountMigration?.(); navigate(`/source/${newSid}/manga/${encodeURIComponent(newMid)}`); },
    onClose: () => { _unmountMigration?.(); _unmountMigration = null; },
  });
}

function _updateUrl() {
  replaceState({
    from_source: _fromSourceId || null,
    page: _page > 1 ? _page : null,
    sort: (_sortOrder && _sortOrder !== 'chapter_desc') ? _sortOrder : null,
    rsort: (!_isLocal && _remoteSort) ? _remoteSort : null,
    dl: _filterDownloaded ? '1' : null,
    unread: _filterUnread ? '1' : null,
    orphaned: _filterOrphaned ? '1' : null,
    scanlator: _filterScanlator || null,
  });
}


let _isLocal = false;
let _dbId = 0;
let _sid = 0;
let _sourceUrl = /** @type {string|null} */ (null);
let _mangaId = '';
let _page = 1;
let _chapterPageSize = 0;
let _sortOrder = 'chapter_desc';
let _addedDbId = /** @type {number|null} */ (null);
let _existingDbId = /** @type {number|null} */ (null);
let _autoScan = false;
let _mangaData = /** @type {any} */ (null);
let _scanlatorMode = 'priority';
let _downloadAllPreferredOnly = true;
let _upgradeAutoReplace = false;
let _suppressedCount = 0;
let _importLinkStatus = /** @type {string|null} */ (null);
let _filterDownloaded = false;
let _filterUnread = false;
let _filterOrphaned = false;
/** @type {{ update: (activeId: string, tabs?: any[]) => void }|null} */
let _tabsHandle = null;
/** @type {HTMLElement|null} */
let _chapterControlsSlot = null;
let _chapterTotal = /** @type {number|null} */ (null);

/** Tab labels; the chapter tab states the count so no separate line has to. */
function _tabDefs() {
  return [
    {
      id: 'chapters',
      name: _chapterTotal != null && _chapterTotal > 0
        ? t('manga.details.tab.chapters_count', { count: _chapterTotal })
        : t('manga.details.tab.chapters'),
    },
    { id: 'manage', name: t('manga.details.tab.manage') },
  ];
}
let _orphanCount = 0;
let _orphanCountLoaded = false;

/**
 * How many chapters a migration kept from this series' previous source.
 * Drives the disabled state of the bulk "Orphaned" selector; one cheap request
 * per series, refreshed when a delete could have changed it.
 */
async function _refreshOrphanCount() {
  if (!_isLocal || !_dbId) { _orphanCount = 0; return; }
  try {
    const res = await api.getLocalChapters(_dbId, 1, 1, _sortOrder, undefined, { filterOrphaned: true });
    _orphanCount = Number(res?.total ?? 0);
  } catch {
    _orphanCount = 0;
  _orphanCountLoaded = false;
  }
  _orphanCountLoaded = true;
}
let _filterCached = false;
let _filterScanlator = /** @type {string|null} */ (null);
let _cachedChapterIds = /** @type {Set<number>} */ (new Set());
let _kccAvailable = false;
/** @type {(() => void)|null} */ let _unsubscribeCacheMsgs = null;
/** @type {string[]} */ let _availableScanlators = [];
let _allSelected = false;
let _fromSourceId = /** @type {string|null} */ (null);

/** @type {Array<{id:string,name:string}>|null} */ let _remoteChapterSorts = null;
/** @type {string|null} */ let _remoteSort = null;

let _activeTab = 'chapters';
let _manageMounted = false;
/** @type {HTMLElement|null} */ let _contentSection = null;

/** @type {AbortController|null} */ let _abort = null;
/** @type {((e: Event) => void)|null} */ let _sseListener = null;
/** @type {((e: KeyboardEvent) => void)|null} */ let _escHandler = null;
/** @type {(() => void)|null} */   let _destroyPagination = null;
/** @type {(() => void)|null} */   let _unmountMigration = null;
/** @type {HTMLElement|null} */ let _listContainerEl = null;
/** @type {HTMLElement[]} */ let _toolbarMounts = [];
/** @type {HTMLElement|null} */ let _paginEl = null;
/** @type {(() => void)|null} */ let _destroyHeader = null;
/** @type {HTMLElement|null} */ let _streamingBannerEl = null;

let _chapters = /** @type {any[]} */ ([]);
let _chaptersHasMore = false;
let _chaptersLoading = false;
/** @type {Set<number>} */         let _notedChapterIds = new Set();
/** @type {Array<{chapter_id:number,chapter_number:number,note:string}>} */
let _chapterNotes = [];
/** @type {any[] | null} */       let _allRemoteChapters = null;
let _selectMode = false;
/** @type {Set<number>} */        let _selected = new Set();
/** @type {(() => void)|null} */  let _chapterResizeListener = null;
/** @type {(() => void)|null} */  let _manageResizeListener = null;

/** @type {Array<{id:number,manga_id:number,scanlator:string,priority:number,blocked:boolean}>} */
let _scanlatorPrefs = [];


/**
 * @param {HTMLElement} container
 * @param {{ id?: string, manga_id?: string, db_id?: string }} params
 */
export async function init(container, params) {
  _page = 1;
  _chapterPageSize = getLocalInt('kani_chapter_page_size', 50);
  _sortOrder = getLocal('kani_chapter_sort_order') || 'chapter_desc';
  _addedDbId = null;
  _existingDbId = null;
  _listContainerEl = null;
  _paginEl = null;
  _destroyHeader = null;
  _chapters = [];
  _chaptersHasMore = false;
  _chaptersLoading = false;
  _allRemoteChapters = null;
  _selectMode = false;
  _selected = new Set();
  _activeTab = 'chapters';
  _manageMounted = false;
  _contentSection = null;
  _scanlatorPrefs = [];
  _isLocal = !!params.db_id;
  _dbId = params.db_id ? Number(params.db_id) : 0;
  _sid = params.id ? Number(params.id) : 0;
  _mangaId = params.manga_id ?? '';
  _sourceUrl = null;
  _scanlatorMode = 'priority';
  _downloadAllPreferredOnly = true;
  _filterDownloaded = _dbId ? getLocal(`kani_filter_downloaded_${_dbId}`) === 'true' : false;
  _filterUnread = false;
  _filterOrphaned = false;
  _orphanCount = 0;
  _filterCached = false;
  _filterScanlator = null;
  _cachedChapterIds = new Set();
  _kccAvailable = false;
  _unsubscribeCacheMsgs?.();
  _unsubscribeCacheMsgs = null;
  _remoteChapterSorts = null;
  _remoteSort = null;
  _availableScanlators = [];
  _allSelected = false;

  {
    const _urlParams = new URLSearchParams(location.search);
    const _pageParam = _urlParams.get('page');
    if (_pageParam) _page = Math.max(1, parseInt(_pageParam, 10) || 1);
    const _sortParam = _urlParams.get('sort');
    if (_sortParam) { _sortOrder = _sortParam; setLocal('kani_chapter_sort_order', _sortOrder); }
    if (_urlParams.get('dl') === '1') _filterDownloaded = true;
    if (_urlParams.get('unread') === '1') _filterUnread = true;
    if (_urlParams.get('orphaned') === '1') _filterOrphaned = true;
    const _scanlatorParam = _urlParams.get('scanlator');
    if (_scanlatorParam) _filterScanlator = _scanlatorParam;
  }

  _abort = new AbortController();
  container.innerHTML = skeletonMangaHero();

  let info, source, autoDownload;
  try {
    if (_isLocal) {
      const res = await api.getMangaDetails(_dbId, _abort.signal);
      info = res.info ?? res;
      source = res.source ?? null;
      autoDownload = res.auto_download ?? false;
      _autoScan = res.auto_scan ?? false;
      _scanlatorMode = res.scanlator_mode ?? 'priority';
      _downloadAllPreferredOnly = res.download_all_preferred_only ?? true;
      _upgradeAutoReplace = res.upgrade_auto_replace ?? false;
      _suppressedCount = res.suppressed_chapter_count ?? 0;
      _importLinkStatus = res.import_link_status ?? null;
      if (info) {
        if (res.notes !== undefined) info.notes = res.notes;
        info.cover_overridden   = res.cover_overridden ?? false;
        info.local_name         = res.local_name ?? null;
        info.local_description  = res.local_description ?? null;
        info.local_status       = res.local_status ?? null;
        info.local_authors      = res.local_authors ?? [];
        info.local_artists      = res.local_artists ?? [];
        info.local_tags         = res.local_tags ?? [];
        info.chapter_count      = res.chapter_count ?? null;
        info.added_at           = res.added_at ?? null;
        info.has_local_people   = res.has_local_people ?? false;
        info.has_local_tags     = res.has_local_tags ?? false;
        info.source_name        = res.source_name ?? info.title;
        info.source_description = res.source_description ?? null;
        info.source_status      = res.source_status ?? null;
        info.source_authors     = res.source_authors ?? [];
        info.source_artists     = res.source_artists ?? [];
        info.source_tags        = res.source_tags ?? [];
      }
      _sid = source?.id ?? 0;
      _mangaId = info?.source_manga_id ?? '';
      const [prefs, scanlators] = await Promise.all([
        api.getScanlatorPrefs(_dbId).catch(() => []),
        api.getChapterScanlators(_dbId).catch(() => []),
      ]);
      _scanlatorPrefs = Array.isArray(prefs) ? prefs : [];
      _availableScanlators = Array.isArray(scanlators) ? scanlators : [];
      if (_sid && _mangaId) {
        _sourceUrl = await api.getSourceMangaUrl(_sid, _mangaId).then(r => r?.url ?? null).catch(() => null);
      }
    } else {
      const [details, src, inLib, sourceUrlResult] = await Promise.all([
        api.getRemoteMangaDetails(_sid, _mangaId, _abort.signal),
        api.getSource(_sid).catch(() => null),
        api.checkInLibrary(_sid, _mangaId).catch(() => ({ db_id: null })),
        api.getSourceMangaUrl(_sid, _mangaId).catch(() => null),
      ]);
      info = details;
      source = src;
      autoDownload = false;
      _existingDbId = inLib?.db_id ?? null;
      _sourceUrl = sourceUrlResult?.url ?? null;
    }
  } catch (e) {
    container.innerHTML = '';
    const msg = /** @type {any} */ (e)?.code === 'source_disabled'
      ? 'Extension is disabled — enable it in Settings > Sources.'
      : 'Failed to load manga details.';
    container.appendChild(createErrorState({ message: msg }));
    return;
  }

  _mangaData = info;
  document.title = (info?.title ?? 'Manga') + ' - Kani';
  if (_isLocal && _dbId) api.markMangaSeen(_dbId).catch(() => {});

  container.innerHTML = '';
  // Desktop and tablet fill the shell exactly and never scroll the page: the
  // rail and the chapter list are their own scroll regions. Mobile keeps
  // ordinary document flow, where swiping is the natural gesture.
  container.classList.add('page-fixed');
  const wrap = document.createElement('div');
  wrap.className = 'max-w-page w-full mx-auto px-4 md:px-6 py-4 md:py-6 flex flex-col gap-6 md:gap-8 page-body-host';
  container.appendChild(wrap);

  _fromSourceId = new URLSearchParams(location.search).get('from_source');
  // Breadcrumbs are mutually exclusive: either we came from the library
  // or from a source — never both at once.
  const _mangaTitle = info?.title ?? 'Manga';
  let crumbs;
  if (!_fromSourceId && _isLocal) {
    crumbs = [{ label: t('library.crumb'), href: '/' }, { label: _mangaTitle }];
  } else if (source) {
    crumbs = [
      { label: t('sources.crumb'), href: '/sources' },
      { label: source.name, href: `/source/${source.id}` },
      { label: _mangaTitle },
    ];
  } else {
    crumbs = [{ label: _mangaTitle }];
  }
  const _headerActions = (() => {
    if (!_sourceUrl) return undefined;
    const a = document.createElement('a');
    a.href = _sourceUrl;
    a.target = '_blank';
    a.rel = 'noopener';
    a.className = 'btn-secondary btn-sm';
    a.textContent = t('manga.details.view_on_source');
    return a;
  })();
  setPageHeader({ crumbs, actions: _headerActions });

  // The page had no heading at all — the title only appeared in the breadcrumb.
  // The backdrop sits behind the top of the content so the cover (first child of
  // leftCol) floats on it.
  const heroCoverUrl = info?.cover_url
    ?? info?.cover_image_url
    ?? (_isLocal ? api.getMangaCoverUrl(_dbId, 'lg') : null);

  const hero = document.createElement('div');
  hero.className = 'manga-hero';
  if (heroCoverUrl) {
    const backdrop = document.createElement('div');
    backdrop.className = 'manga-hero__backdrop';
    backdrop.style.backgroundImage = `url("${heroCoverUrl.replace(/"/g, '%22')}")`;
    hero.appendChild(backdrop);
  }
  const heroInner = document.createElement('div');
  heroInner.className = 'manga-hero__inner';
  const heroTitle = document.createElement('h1');
  heroTitle.className = 'manga-hero__title';
  heroTitle.textContent = _mangaTitle;
  heroInner.appendChild(heroTitle);
  hero.appendChild(heroInner);
  // The hero sits OUTSIDE the max-w-page wrapper: the band must bleed across the whole pane (on
  // wide monitors a wrapper-bound band floats as an island with hard vertical seams). The inner
  // div re-centres the title onto the same content grid as the columns below.
  container.insertBefore(hero, wrap);

  const layout = document.createElement('div');
  layout.className = 'flex flex-col lg:flex-row gap-6 lg:gap-8 lg:items-stretch page-fill manga-hero__body';
  wrap.appendChild(layout);

  const leftCol = document.createElement('div');
  leftCol.className = 'manga-rail w-full flex flex-col lg:shrink-0 page-col';
  layout.appendChild(leftCol);

  const rightCol = document.createElement('div');
  rightCol.className = 'w-full min-w-0 flex flex-col gap-4 lg:flex-1 page-col';
  layout.appendChild(rightCol);

  // The notice is about chapters, so it heads the chapter column rather than
  // spanning the page under the title: it keeps the hero band's overlap intact
  // and sits next to the list it is talking about.
  if (_isLocal && _dbId && _suppressedCount > 0) {
    mountSuppressedBanner(rightCol, _dbId, _suppressedCount);
  }

  // An imported entry has no chapters until its id is matched to the source,
  // which otherwise reads as an empty series.
  if (_isLocal && _dbId && _importLinkStatus) {
    mountImportLinkBanner(rightCol, _dbId, _importLinkStatus, source?.name ?? '', {
      onMigrate: (hasPermission('library:manage') && _sid) ? _openMigrationDialogue : null,
    });
  }

  const { destroy: destroyHeader } = mountMangaHeader(leftCol, info, source, {
    isLocal: _isLocal,
    dbId: _dbId,
    sid: _sid,
    mangaId: _mangaId,
    existingDbId: () => _existingDbId,
    addedDbId: () => _addedDbId,
    findNextPreferredChapter: _findNextPreferredChapter,
    getChapters: () => _chapters,
    onDownloadAll: async () => {
      const { job_id } = await api.downloadAll(_dbId);
      showToast(t('manga.download_all.queued'));
      const refreshChapters = () => {
        _page = 1;
        if (_activeTab !== 'chapters') {
          /** @type {HTMLElement|null} */ (document.querySelector('[data-tab="chapters"]'))?.click();
        } else if (_contentSection) {
          _fetchChapters(_contentSection);
        }
      };
      if (job_id) {
        subscribeJob(job_id, {
          onComplete: () => { showToast(t('manga.download_all.done')); refreshChapters(); },
          onFailed: (/** @type {any} */ e) => { showApiError(e); refreshChapters(); },
          onCancelled: () => refreshChapters(),
        });
      } else {
        refreshChapters();
      }
      return { jobId: job_id ?? null };
    },
    onCancelAll: async () => {
      await api.cancelAllDownloads(_dbId);
      showToast(t('manga.download_all.cancelled'));
      _page = 1;
      if (_activeTab !== 'chapters') {
        /** @type {HTMLElement|null} */ (document.querySelector('[data-tab="chapters"]'))?.click();
      } else if (_contentSection) {
        _fetchChapters(_contentSection);
      }
    },
    onScan: async () => {
      return await api.scanManga(_dbId);
    },
    onAddedToLibrary: (newDbId) => { _addedDbId = newDbId; },
    onSwitchToChapters: () => {
      if (_activeTab !== 'chapters' && _contentSection) {
        _activeTab = 'chapters';
        _fetchChapters(_contentSection);
      }
    },
  });
  _destroyHeader = destroyHeader;

  if (info?.tags?.length) {
    const tags = document.createElement('div');
    tags.className = 'flex flex-wrap gap-2';
    for (const tag of info.tags) {
      const a = document.createElement('a');
      if (_isLocal) {
        a.href = `/?tag_id=${tag.id}`;
        a.addEventListener('click', e => { e.preventDefault(); navigate(`/?tag_id=${tag.id}`); });
      } else {
        a.href = `/source/${_sid}?q=${encodeURIComponent(tag.name)}`;
        a.addEventListener('click', e => {
          e.preventDefault();
          import('../components/manga-details/manga-header.js').then(({ buildSourceMetaUrl }) =>
            buildSourceMetaUrl(_sid, tag.name, 'Tag').then(url => navigate(url))
          );
        });
      }
      a.className = 'chip text-xs';
      a.textContent = tag.name;
      tags.appendChild(a);
    }
    rightCol.appendChild(tags);
  }

  if (!_isLocal && _sid) {
    const sorts = await api.getRemoteChapterSorts(_sid, _mangaId).catch(() => []);
    _remoteChapterSorts = Array.isArray(sorts) ? sorts : [];
    const _rsortParam = new URLSearchParams(location.search).get('rsort');
    if (_remoteChapterSorts.length > 0) {
      if (_rsortParam && _remoteChapterSorts.some(s => s.id === _rsortParam)) {
        _remoteSort = _rsortParam;
      } else {
        _remoteSort = _remoteChapterSorts[0].id;
      }
    }
  }

  if (_isLocal) {
    getCachedChapterIds().then(ids => { _cachedChapterIds = ids; _renderChapterList(); });
    fetch('/rest/system/capabilities').then(r => r.json()).then(d => { _kccAvailable = !!d.kcc; _renderChapterList(); }).catch(() => {});
    _unsubscribeCacheMsgs = onChapterCached(id => { _cachedChapterIds = new Set(_cachedChapterIds); _cachedChapterIds.add(id); _renderChapterList(); });
    if (_dbId) api.getMangaChapterNotes(_dbId).then(res => {
      _chapterNotes = res?.notes ?? [];
      _notedChapterIds = new Set(_chapterNotes.map(n => n.chapter_id));
      _renderChapterList();
    }).catch(() => {});
    _renderTabs(rightCol);
    await _fetchChapters(/** @type {HTMLElement} */(_contentSection));
  } else {
    const chapterSection = document.createElement('div');
    rightCol.appendChild(chapterSection);
    _contentSection = chapterSection;
    await _fetchChapters(chapterSection);
  }

  _sseListener = (e) => {
    const data = /** @type {CustomEvent} */ (e).detail;
    if (!data) return;
    if (
      data.type === 'manga_refreshed' &&
      (data.manga_id === _dbId || data.db_id === _dbId)
    ) {
      if (_activeTab === 'chapters' && _contentSection) _fetchChapters(_contentSection);
    }
    if (data.type === 'chapter_completed' && _isLocal) {
      const chId = Number(data.chapter_id);
      let updated = false;
      _chapters = _chapters.map(ch => {
        if (ch.id === chId && !ch.downloaded) { updated = true; return { ...ch, downloaded: true }; }
        return ch;
      });
      if (updated) _renderChapterList();
    }
    if (data.manga_id === _dbId) {
      if (data.type === 'chapter_list_partial') {
        _updateStreamingCounter(data.received, false);
      } else if (data.type === 'chapter_list_complete') {
        _updateStreamingCounter(data.total, true);
        if (_activeTab === 'chapters' && _contentSection) _fetchChapters(_contentSection);
      } else if (data.type === 'chapter_list_error') {
        _clearStreamingCounter();
      }
    }
  };
  window.addEventListener('kani:sse', _sseListener);

  _escHandler = (e) => {
    if (e.key === 'Escape' && _selectMode) {
      _selectMode = false; _selected.clear(); _allSelected = false; _renderChapterList();
    }
  };
  document.addEventListener('keydown', _escHandler);
}


function _renderTabs(wrap) {
  const tabContent = document.createElement('div');
  tabContent.className = 'page-fill page-col';
  _contentSection = tabContent;

  // Chapter controls share the tab row to preserve vertical reading space.
  const tabRow = document.createElement('div');
  tabRow.className = 'chapter-tabrow flex items-end justify-between gap-4 flex-wrap';

  const tabBar = document.createElement('div');
  tabBar.className = 'chapter-tabs';
  const tabsHandle = renderTabs(tabBar, {
    tabs: _tabDefs(),
    activeId: _activeTab,
    onSelect: switchTab,
  });
  _tabsHandle = tabsHandle;

  _chapterControlsSlot = document.createElement('div');
  _chapterControlsSlot.className = 'flex items-center gap-2 flex-wrap pb-2';

  tabRow.appendChild(tabBar);
  tabRow.appendChild(_chapterControlsSlot);
  wrap.appendChild(tabRow);
  wrap.appendChild(tabContent);

  function switchTab(/** @type {string} */ tab) {
    _activeTab = tab;
    tabsHandle.update(tab);
    if (_listContainerEl) { render(null, _listContainerEl); _listContainerEl = null; }
    _destroyPagination?.();
    _destroyPagination = null;
    if (_manageResizeListener) { window.removeEventListener('resize', _manageResizeListener); _manageResizeListener = null; }
    tabContent.style.height = '';
    tabContent.style.overflowY = '';
    tabContent.style.scrollbarWidth = '';
    tabContent.innerHTML = '';

    if (tab === 'chapters') {
      _manageMounted = false;
      _fetchChapters(tabContent);
    } else {
      _manageMounted = false;
      _renderManageTab(tabContent);
    }
  }

  switchTab(_activeTab);
}


async function _renderManageTab(contentEl) {
  if (_manageMounted) return;
  _manageMounted = true;

  contentEl.className = 'flex flex-col gap-4';

  const manageTabBar = document.createElement('div');
  manageTabBar.className = 'sticky top-0 z-10 bg-bg pb-1';
  contentEl.appendChild(manageTabBar);

  const groupsWrap = document.createElement('div');
  contentEl.appendChild(groupsWrap);

  const mkGroup = () => {
    const g = document.createElement('div');
    g.className = 'flex flex-col gap-8';
    return g;
  };
  const manageGroups = {
    metadata: mkGroup(),
    library: mkGroup(),
    tracking: mkGroup(),
    downloads: mkGroup(),
    danger: mkGroup(),
  };

  function applyManageHeight() {
    if (window.innerWidth >= 1024) {
      const top = contentEl.getBoundingClientRect().top;
      contentEl.style.height = Math.max(200, window.innerHeight - top - 48) + 'px';
      contentEl.style.overflowY = 'auto';
      contentEl.style.scrollbarWidth = 'none';
    } else {
      contentEl.style.height = '';
      contentEl.style.overflowY = '';
      contentEl.style.scrollbarWidth = '';
    }
  }
  applyManageHeight();
  _manageResizeListener = applyManageHeight;
  window.addEventListener('resize', _manageResizeListener);


  if (hasPermission('library:manage') && _isLocal) {
    const metaSection = document.createElement('div');
    metaSection.className = 'flex flex-col gap-3';
    metaSection.appendChild(mkSectionHeader(t('manga.details.metadata.section'), t('manga.details.metadata.section.desc')));
    mountMetadataPanel(metaSection, { dbId: _dbId, mangaData: _mangaData });
    manageGroups.metadata.appendChild(metaSection);
  }


  if (hasPermission('library:manage') && _isLocal) {
    const enrichSection = document.createElement('div');
    enrichSection.className = 'flex flex-col gap-3';
    enrichSection.appendChild(mkSectionHeader(t('manga.details.enrich.section'), t('manga.details.enrich.section.desc')));
    const enrichCard = mkCard();
    const enrichBtn = document.createElement('button');
    enrichBtn.type = 'button';
    enrichBtn.className = 'btn-secondary btn-sm';
    enrichBtn.textContent = t('manga.details.enrich');
    enrichBtn.addEventListener('click', () => _openEnrichMetadataModal());
    enrichCard.appendChild(mkItem(mkRow(t('manga.details.enrich.row'), t('manga.details.enrich.row.desc'), enrichBtn)));
    enrichSection.appendChild(enrichCard);
    manageGroups.metadata.appendChild(enrichSection);
  }


  const hasLibSection =
    hasPermission('library:refresh') ||
    hasPermission('library:manage');

  if (hasLibSection) {
    const section = document.createElement('div');
    section.className = 'flex flex-col gap-3';
    section.appendChild(mkSectionHeader(t('manga.details.library.section'), t('manga.details.library.section.desc')));
    mountLibrarySettingsPanel(section, { dbId: _dbId, autoScan: _autoScan });
    manageGroups.library.appendChild(section);
  }


  if (hasPermission('library:manage') && _isLocal) {
    const volumesSection = document.createElement('div');
    volumesSection.className = 'flex flex-col gap-3';
    volumesSection.appendChild(mkSectionHeader(t('manga.details.volumes.section'), t('manga.details.volumes.section.desc')));
    manageGroups.library.appendChild(volumesSection);
    mountVolumesPanel(volumesSection, _dbId);
  }


  {
    const trackSection = document.createElement('div');
    trackSection.className = 'flex flex-col gap-3';
    trackSection.appendChild(mkSectionHeader(t('manga.details.tracking.section'), t('manga.details.tracking.section.desc')));
    mountTrackerPanel(trackSection, { dbId: _dbId, title: _mangaData?.title ?? '' });
    manageGroups.tracking.appendChild(trackSection);
  }


  if (hasPermission('library:manage')) {
    const notesSection = document.createElement('div');
    notesSection.className = 'flex flex-col gap-3';
    notesSection.appendChild(mkSectionHeader(t('manga.details.notes.section'), t('manga.details.notes.section.desc')));
    const notesCard = mkCard();
    const notesArea = document.createElement('textarea');
    notesArea.className = 'input w-full text-sm resize-y min-h-24 p-3';
    notesArea.placeholder = t('manga.details.notes.placeholder');
    notesArea.value = _mangaData?.notes ?? '';
    let _notesSaveTimer = /** @type {ReturnType<typeof setTimeout>|null} */ (null);
    const notesSaveStatus = document.createElement('span');
    notesSaveStatus.className = 'text-xs text-muted mt-1 hidden';
    notesArea.addEventListener('input', () => {
      if (_notesSaveTimer) clearTimeout(_notesSaveTimer);
      _notesSaveTimer = setTimeout(async () => {
        try {
          await api.updateMangaNotes(_dbId, notesArea.value);
          notesSaveStatus.textContent = t('manga.details.notes.saved');
          notesSaveStatus.classList.remove('hidden', 'text-error');
          notesSaveStatus.classList.add('text-success');
          setTimeout(() => notesSaveStatus.classList.add('hidden'), 2000);
        } catch {
          notesSaveStatus.textContent = t('manga.details.notes.failed');
          notesSaveStatus.classList.remove('hidden', 'text-success');
          notesSaveStatus.classList.add('text-error');
        }
      }, 500);
    });
    const notesWrap = document.createElement('div');
    notesWrap.className = 'flex flex-col gap-1 px-4 py-3';
    notesWrap.appendChild(notesArea);
    notesWrap.appendChild(notesSaveStatus);

    if (_chapterNotes.length > 0) {
      const chapNotesDiv = document.createElement('div');
      chapNotesDiv.className = 'flex flex-col gap-1 border-t border-border-subtle pt-3';
      const chapNotesTitle = document.createElement('p');
      chapNotesTitle.className = 'text-xs font-medium text-muted px-0';
      chapNotesTitle.textContent = t('manga.details.chapter_notes');
      chapNotesDiv.appendChild(chapNotesTitle);
      for (const n of _chapterNotes) {
        const item = document.createElement('div');
        item.className = 'flex flex-col gap-0.5 py-1.5 border-t border-border-subtle';
        const lbl = document.createElement('p');
        lbl.className = 'text-xs text-muted font-medium';
        lbl.textContent = t('manga.details.chapter_notes.label', { num: n.chapter_number });
        const txt = document.createElement('p');
        txt.className = 'text-sm text-text whitespace-pre-wrap';
        txt.textContent = n.note;
        item.appendChild(lbl);
        item.appendChild(txt);
        chapNotesDiv.appendChild(item);
      }
      notesWrap.appendChild(chapNotesDiv);
    }

    notesCard.appendChild(notesWrap);
    notesSection.appendChild(notesCard);
    manageGroups.metadata.appendChild(notesSection);
  }


  if (hasPermission('library:manage')) {
    const [cats, mangaCats, rules, scanlatorPrefs] = await Promise.allSettled([
      api.getCategories(),
      api.getMangaCategories(_dbId),
      api.getDownloadRules(_dbId),
      api.getScanlatorPrefs(_dbId),
    ]).then(r => r.map(s => s.status === 'fulfilled' ? s.value : []));
    _scanlatorPrefs = Array.isArray(scanlatorPrefs) ? scanlatorPrefs : [];

    const catsSection = document.createElement('div');
    catsSection.className = 'flex flex-col gap-3';
    const catsCard = mkTitledCard(t('manga.details.categories.title'), t('manga.details.categories.desc'));
    mountCategoryPicker(catsCard, cats, mangaCats, _dbId);
    catsSection.appendChild(catsCard);
    manageGroups.library.appendChild(catsSection);

    const dlSection = document.createElement('div');
    dlSection.className = 'flex flex-col gap-3';

    const rulesCard = mkTitledCard(t('manga.details.download_filters.title'), t('manga.details.download_filters.desc'));
    mountDownloadRulesPanel(rulesCard, rules, _dbId);
    dlSection.appendChild(rulesCard);

    const prefsCard = mkTitledCard(t('manga.details.scanlator_prefs.title'), t('manga.details.scanlator_prefs.desc'));
    mountScanlatorPrefsPanel(prefsCard, _scanlatorPrefs, _scanlatorMode, _dbId, (updated) => {
      _scanlatorPrefs = updated;
    }, _upgradeAutoReplace, (on) => {
      _upgradeAutoReplace = on;
    });
    dlSection.appendChild(prefsCard);

    manageGroups.downloads.appendChild(dlSection);
  }


  const hasDangerSection =
    (hasPermission('library:manage') && _sid) ||
    hasPermission('library:delete');

  if (hasDangerSection) {
    const section = document.createElement('div');
    section.className = 'flex flex-col gap-3';
    section.appendChild(mkSectionHeader(t('manga.details.danger.section'), t('manga.details.danger.section.desc')));

    const card = mkCard();

    if (hasPermission('library:manage') && _sid) {
      const migrateBtn = document.createElement('button');
      migrateBtn.type = 'button';
      migrateBtn.className = 'btn-ghost btn-sm';
      migrateBtn.textContent = t('manga.details.migrate');
      migrateBtn.addEventListener('click', _openMigrationDialogue);
      card.appendChild(mkItem(mkRow(t('manga.details.migrate.row'), t('manga.details.migrate.row.desc'), migrateBtn)));
    }

    if (hasPermission('library:delete')) {
      const removeBtn = document.createElement('button');
      removeBtn.type = 'button';
      removeBtn.className = 'btn-danger btn-sm';
      removeBtn.textContent = t('manga.details.remove');
      removeBtn.addEventListener('click', async () => {
        const confirmed = await showConfirm(t('manga.details.remove.confirm.body'), {
          title: t('manga.details.remove.confirm.title'),
          confirmLabel: t('manga.details.remove'),
          danger: true,
        });
        if (!confirmed) return;
        removeBtn.disabled = true;
        try {
          const dbId = _dbId;
          const result = await api.deleteManga(dbId);
          navigate('/');
          const token = result?.undo_token;
          if (token) {
            showToast(t('manga.removed'), {
              duration: 10000,
              action: {
                label: t('common.undo'),
                onClick: async () => {
                  try {
                    await api.untrashMangaByToken(token);
                    navigate(`/manga/${dbId}`);
                    showToast(t('manga.restored'), { type: 'success' });
                  } catch (e) { showApiError(e); }
                },
              },
            });
          }
        }
        catch { removeBtn.disabled = false; }
      });
      card.appendChild(mkItem(mkRow(t('manga.details.remove.row'), t('manga.details.remove.row.desc'), removeBtn)));
    }

    section.appendChild(card);
    manageGroups.danger.appendChild(section);
  }

  const tabDefs = /** @type {Array<[keyof typeof manageGroups, string]>} */ ([
    ['metadata', t('manga.manage.tab.metadata')],
    ['library', t('manga.manage.tab.library')],
    ['tracking', t('manga.manage.tab.tracking')],
    ['downloads', t('manga.manage.tab.downloads')],
    ['danger', t('manga.manage.tab.danger')],
  ]).filter(([id]) => manageGroups[id].children.length > 0);

  if (tabDefs.length === 0) return;

  let activeGroup = /** @type {string} */ (getLocal('kani_manage_tab') ?? '');
  if (!tabDefs.some(([id]) => id === activeGroup)) activeGroup = tabDefs[0][0];

  for (const [id] of tabDefs) {
    manageGroups[id].classList.toggle('hidden', id !== activeGroup);
    groupsWrap.appendChild(manageGroups[id]);
  }

  const tabsHandle = renderTabs(manageTabBar, {
    tabs: tabDefs.map(([id, name]) => ({ id, name })),
    activeId: activeGroup,
    variant: 'pill',
    onSelect: (id) => {
      activeGroup = id;
      setLocal('kani_manage_tab', id);
      for (const [gid] of tabDefs) manageGroups[gid].classList.toggle('hidden', gid !== id);
      tabsHandle.update(id);
      contentEl.scrollTop = 0;
    },
  });
}


/** @param {any} ch */
function _mapChapter(ch) {
  return {
    id: Number(ch.id),
    title: formatChapterTitle(ch),
    chapter_number: ch.number ?? ch.chapter_number,
    source_chapter_id: ch.source_chapter_id ?? null,
    scanlator: ch.scanlator ?? null,
    date_uploaded: ch.date_uploaded ?? null,
    downloaded: isChapterDownloaded(ch, null),
    read: ch.is_read ?? false,
    last_page_read: ch.last_page_read ?? 0,
    is_orphaned: ch.is_orphaned ?? false,
    download_error: ch.download_error ?? null,
    upgrade_available: ch.upgrade_available ?? null,
  };
}

/**
 * @param {any[]} chapters
 * @param {string} order
 * @returns {any[]}
 */
function _sortChaptersClientSide(chapters, order) {
  const cmp = (a, b, key, asc) => {
    const va = a[key] ?? null, vb = b[key] ?? null;
    if (va === null && vb === null) return 0;
    if (va === null) return asc ? -1 : 1;
    if (vb === null) return asc ? 1 : -1;
    return asc ? (va > vb ? 1 : va < vb ? -1 : 0) : (va < vb ? 1 : va > vb ? -1 : 0);
  };
  const sorted = [...chapters];
  switch (order) {
    case 'chapter_asc':    sorted.sort((a, b) => cmp(a, b, 'chapter_number', true));  break;
    case 'chapter_desc':   sorted.sort((a, b) => cmp(a, b, 'chapter_number', false)); break;
    case 'uploaded_asc':   sorted.sort((a, b) => cmp(a, b, 'date_uploaded', true));   break;
    case 'uploaded_desc':  sorted.sort((a, b) => cmp(a, b, 'date_uploaded', false));  break;
    case 'scanlator_asc':  sorted.sort((a, b) => cmp(a, b, 'scanlator', true));       break;
    case 'scanlator_desc': sorted.sort((a, b) => cmp(a, b, 'scanlator', false));      break;
    default: sorted.sort((a, b) => cmp(a, b, 'chapter_number', false)); break;
  }
  return sorted;
}

/** @returns {any|null} */
function _findNextPreferredChapter() {
  const candidates = _chapters.filter(ch => !ch.read && !ch.downloaded);
  if (!candidates.length) return null;

  /** @type {Map<number|string, any[]>} */
  const byNumber = new Map();
  for (const ch of candidates) {
    const num = ch.chapter_number ?? ch.id;
    if (!byNumber.has(num)) byNumber.set(num, []);
    byNumber.get(num).push(ch);
  }
  const sortedNums = [...byNumber.keys()].sort((a, b) => Number(a) - Number(b));

  for (const num of sortedNums) {
    const group = byNumber.get(num);
    let eligible = group;
    if (_scanlatorMode === 'whitelist') {
      eligible = group.filter(ch => _scanlatorPrefs.some(p => p.scanlator === ch.scanlator && !p.blocked));
    } else if (_scanlatorMode === 'priority') {
      eligible = group.filter(ch => !_scanlatorPrefs.some(p => p.scanlator === ch.scanlator && p.blocked));
    }
    if (!eligible.length) continue;
    let best = eligible[0];
    for (const ch of eligible.slice(1)) {
      const chPrio = _scanlatorPrefs.find(p => p.scanlator === ch.scanlator)?.priority ?? -1;
      const bestPrio = _scanlatorPrefs.find(p => p.scanlator === best.scanlator)?.priority ?? -1;
      if (chPrio > bestPrio) best = ch;
    }
    return best;
  }
  return null;
}


function _updateStreamingCounter(received, complete) {
  if (!_streamingBannerEl) {
    _streamingBannerEl = document.createElement('div');
    _streamingBannerEl.className = 'fixed bottom-4 left-1/2 -translate-x-1/2 z-toast bg-surface border border-border rounded-lg shadow-lg px-4 py-2 text-sm text-text flex items-center gap-2';
    document.body.appendChild(_streamingBannerEl);
  }
  if (complete) {
    _streamingBannerEl.textContent = t('manga.details.streaming.loaded', { count: received });
    setTimeout(() => _clearStreamingCounter(), 2500);
  } else {
    _streamingBannerEl.textContent = t('manga.details.streaming.loading', { count: received });
  }
}

function _clearStreamingCounter() {
  _streamingBannerEl?.remove();
  _streamingBannerEl = null;
}


function _openEnrichMetadataModal() {
  function EnrichModal({ onClose }) {
    const [providers, setProviders] = useState(/** @type {Array<{id:string,name:string}>} */ ([]));
    const [selected, setSelected] = useState('');
    const [loading, setLoading] = useState(true);
    const [submitting, setSubmitting] = useState(false);

    useEffect(() => {
      api.listMetadataProviders().then(ps => {
        setProviders(ps);
        if (ps.length > 0) setSelected(ps[0].id);
        setLoading(false);
      }).catch(e => { showApiError(e); onClose(); });
    }, []);

    const handleSubmit = async (e) => {
      e.preventDefault();
      if (!selected) return;
      setSubmitting(true);
      try {
        const result = await api.enrichMangaMetadata(_dbId, selected);
        onClose();
        if (result.fields_updated.length > 0) {
          showToast(t('manga.details.enrich.updated', { fields: result.fields_updated.join(', ') }));
        } else {
          showToast(t('manga.details.enrich.none'));
        }
      } catch (err) {
        showApiError(err);
      } finally {
        setSubmitting(false);
      }
    };

    const footer = html`
      <button type="button" class="btn-ghost btn-sm" onClick=${onClose} disabled=${submitting}>${t('common.cancel')}</button>
      <button type="submit" form="enrich-form" class="btn-primary btn-sm" disabled=${submitting || loading || !selected}>
        ${submitting ? t('manga.details.enrich.ing') : t('manga.details.enrich.action')}
      </button>
    `;

    return html`
      <${Modal} open=${true} title=${t('manga.details.enrich.section')} onClose=${onClose} footer=${footer}>
        <form id="enrich-form" class="flex flex-col gap-4 px-1" onSubmit=${handleSubmit}>
          ${loading
            ? html`<p class="text-sm text-muted">${t('manga.details.enrich.loading')}</p>`
            : providers.length === 0
              ? html`<p class="text-sm text-muted">${t('manga.details.enrich.empty')}</p>`
              : html`
                  <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-text" for="enrich-provider">${t('manga.details.enrich.provider')}</label>
                    <select id="enrich-provider" class="input"
                      value=${selected}
                      onChange=${(e) => setSelected(e.target.value)}
                      disabled=${submitting}
                    >
                      ${providers.map(p => html`<option key=${p.id} value=${p.id}>${p.name}</option>`)}
                    </select>
                  </div>
                  <p class="text-xs text-muted">${t('manga.details.enrich.hint')}</p>
                `
          }
        </form>
      </${Modal}>
    `;
  }

  const unmount = mountIntoModalRoot(html`<${EnrichModal} onClose=${() => unmount()} />`);
}


function _openChapterNotesModal() {
  const overlay = document.createElement('div');
  overlay.className = 'fixed inset-0 z-modal flex items-center justify-center p-4 bg-scrim';

  const card = document.createElement('div');
  card.className = 'bg-surface rounded-xl shadow-lg w-full max-w-md max-h-[70vh] flex flex-col overflow-hidden';

  const hdr = document.createElement('div');
  hdr.className = 'flex items-center justify-between gap-3 px-5 py-4 border-b border-border shrink-0';
  const title = document.createElement('h2');
  title.className = 'text-lg font-semibold text-text';
  title.textContent = t('manga.details.chapter_notes.modal_title');
  const closeBtn = document.createElement('button');
  closeBtn.className = 'btn-icon';
  closeBtn.setAttribute('aria-label', t('common.close'));
  closeBtn.innerHTML = iconX;
  hdr.appendChild(title);
  hdr.appendChild(closeBtn);

  const body = document.createElement('div');
  body.className = 'overflow-y-auto flex-1 divide-y divide-border';
  for (const n of _chapterNotes) {
    const item = document.createElement('div');
    item.className = 'px-5 py-3';
    const lbl = document.createElement('p');
    lbl.className = 'text-xs text-muted font-medium mb-1';
    lbl.textContent = t('manga.details.chapter_notes.chapter', { num: n.chapter_number });
    const text = document.createElement('p');
    text.className = 'text-sm text-text whitespace-pre-wrap';
    text.textContent = n.note;
    item.appendChild(lbl);
    item.appendChild(text);
    body.appendChild(item);
  }

  card.appendChild(hdr);
  card.appendChild(body);
  overlay.appendChild(card);
  document.body.appendChild(overlay);

  const _onKey = (/** @type {KeyboardEvent} */ e) => { if (e.key === 'Escape') _close(); };
  const _close = () => {
    overlay.remove();
    document.removeEventListener('keydown', _onKey);
  };
  overlay.addEventListener('click', (e) => { if (e.target === overlay) _close(); });
  closeBtn.addEventListener('click', _close);
  document.addEventListener('keydown', _onKey);
}


/** @param {HTMLElement} sectionEl */
async function _fetchChapters(sectionEl) {
  const infinite = prefersInfiniteScroll('kani_chapter_pagination');

  if (_listContainerEl) { render(null, _listContainerEl); _listContainerEl = null; }
  for (const el of _toolbarMounts) render(null, el);
  _toolbarMounts = [];
  _destroyPagination?.();
  _destroyPagination = null;
  if (_chapterResizeListener) { window.removeEventListener('resize', _chapterResizeListener); _chapterResizeListener = null; }

  if (_page === 1) { _chapters = []; _chaptersHasMore = false; _chaptersLoading = false; }

  sectionEl.className = 'flex flex-col gap-3 page-fill';
  sectionEl.innerHTML = '';
  startLoading();

  // Controls live in the tab row when there is one (the local library view);
  // the remote-source view has no tabs, so it keeps a header of its own.
  const header = document.createElement('div');
  header.className = 'flex items-center justify-end gap-3 flex-wrap';

  const controls = _chapterControlsSlot ?? document.createElement('div');
  if (!_chapterControlsSlot) controls.className = 'flex items-center gap-2 flex-wrap';
  controls.innerHTML = '';

  const mountControl = (/** @type {any} */ vnode) => {
    const el = document.createElement('div');
    el.className = 'shrink-0';
    render(vnode, el);
    _toolbarMounts.push(el);
    controls.appendChild(el);
  };

  if (!_isLocal && _remoteChapterSorts && _remoteChapterSorts.length > 0) {
    mountControl(html`<${Select}
      options=${_remoteChapterSorts.map((/** @type {{id:string,name:string}} */ s) => ({ value: s.id, label: s.name }))}
      value=${_remoteSort}
      ariaLabel=${t('manga.details.sort.aria')}
      onChange=${(/** @type {string} */ v) => {
        _remoteSort = v;
        _allRemoteChapters = null;
        _page = 1; _updateUrl(); _fetchChapters(sectionEl);
      }}
    />`);
  } else {
    const sortOptions = [
      ['chapter_desc', t('manga.details.sort.chapter_desc')],
      ['chapter_asc', t('manga.details.sort.chapter_asc')],
      ['uploaded_desc', t('manga.details.sort.uploaded_desc')],
      ['uploaded_asc', t('manga.details.sort.uploaded_asc')],
      ['volume_desc', t('manga.details.sort.volume_desc')],
      ['volume_asc', t('manga.details.sort.volume_asc')],
      ['language_asc', t('manga.details.sort.language_asc')],
      ['language_desc', t('manga.details.sort.language_desc')],
      ['scanlator_asc', t('manga.details.sort.scanlator_asc')],
      ['scanlator_desc', t('manga.details.sort.scanlator_desc')],
    ].map(([value, label]) => ({ value, label }));
    mountControl(html`<${Select}
      options=${sortOptions}
      value=${_sortOrder}
      ariaLabel=${t('manga.details.sort.aria')}
      onChange=${(/** @type {string} */ v) => {
        _sortOrder = v;
        setLocal('kani_chapter_sort_order', _sortOrder);
        _page = 1;
        if (_allRemoteChapters !== null) _allRemoteChapters = _sortChaptersClientSide(_allRemoteChapters, _sortOrder);
        _updateUrl(); _fetchChapters(sectionEl);
      }}
    />`);
  }

  if (!infinite) {
    mountControl(html`<${PageSizeSelect}
      options=${[20, 50, 100]}
      value=${_chapterPageSize}
      ariaLabel=${t('manga.details.page_size.aria')}
      onChange=${(/** @type {number} */ n) => {
        _chapterPageSize = n;
        setLocal('kani_chapter_page_size', String(_chapterPageSize));
        _page = 1; _updateUrl(); _fetchChapters(sectionEl);
      }}
    />`);
  }

  if (_isLocal) {
    const filterDefs = [
      {
        key: 'downloaded',
        label: t('manga.details.filter.downloaded'),
        get active() { return _filterDownloaded; },
        toggle: () => {
          _filterDownloaded = !_filterDownloaded;
          setLocal(`kani_filter_downloaded_${_dbId}`, String(_filterDownloaded));
          _page = 1; _updateUrl(); _fetchChapters(sectionEl);
        },
      },
      {
        key: 'unread',
        label: t('manga.details.filter.unread'),
        get active() { return _filterUnread; },
        toggle: () => { _filterUnread = !_filterUnread; _page = 1; _updateUrl(); _fetchChapters(sectionEl); },
      },
      {
        key: 'orphaned',
        label: t('manga.details.filter.orphaned'),
        get active() { return _filterOrphaned; },
        toggle: () => { _filterOrphaned = !_filterOrphaned; _page = 1; _updateUrl(); _fetchChapters(sectionEl); },
      },
      {
        key: 'cached',
        label: t('manga.details.filter.cached'),
        get active() { return _filterCached; },
        toggle: () => { _filterCached = !_filterCached; _renderChapterList(); },
      },
    ];

    const activeFilters = filterDefs.filter(f => f.active);
    const filterBtn = document.createElement('button');
    filterBtn.type = 'button';
    filterBtn.className = (activeFilters.length ? 'chip chip-active' : 'chip') + ' chapter-filter-btn';
    filterBtn.setAttribute('aria-haspopup', 'menu');
    filterBtn.setAttribute('aria-expanded', 'false');
    filterBtn.setAttribute('aria-controls', 'chapter-filter-menu');
    filterBtn.textContent = activeFilters.length
      ? t('manga.details.filter.active', { count: activeFilters.length })
      : t('manga.details.filter.label');
    filterBtn.title = activeFilters.length
      ? activeFilters.map(f => f.label).join(', ')
      : t('manga.details.filter.label.title');
    controls.appendChild(filterBtn);

    let filterMenu = /** @type {HTMLElement|null} */ (null);
    filterBtn.addEventListener('click', () => {
      if (filterMenu) { render(null, filterMenu); filterMenu.remove(); filterMenu = null; filterBtn.setAttribute('aria-expanded', 'false'); return; }
      filterMenu = document.createElement('div');
      document.body.appendChild(filterMenu);
      filterBtn.setAttribute('aria-expanded', 'true');
      const close = () => {
        if (!filterMenu) return;
        render(null, filterMenu); filterMenu.remove(); filterMenu = null;
        filterBtn.setAttribute('aria-expanded', 'false');
      };
      render(
        html`<${ContextMenu}
          items=${filterDefs.map(f => ({
            label: f.active ? t('manga.details.filter.on', { name: f.label }) : f.label,
            action: () => f.toggle(),
          }))}
          id="chapter-filter-menu"
          trigger=${{ current: filterBtn }}
          onClose=${close}
        />`,
        filterMenu,
      );
    });

    if (_availableScanlators.length > 1) {
      mountControl(html`<${Select}
        options=${[
          { value: '', label: t('manga.details.all_scanlators') },
          ..._availableScanlators.map((/** @type {string} */ s) => ({ value: s, label: s })),
        ]}
        value=${_filterScanlator ?? ''}
        ariaLabel=${t('manga.details.scanlator.aria')}
        onChange=${(/** @type {string} */ v) => {
          _filterScanlator = v || null;
          _page = 1; _updateUrl(); _fetchChapters(sectionEl);
        }}
      />`);
    }
  }

  // Only mount the local header when the controls are not already in the tab row.
  if (!_chapterControlsSlot) {
    header.appendChild(controls);
    sectionEl.appendChild(header);
  }

  const listEl = document.createElement('div');
  listEl.className = 'page-body--fit';
  sectionEl.appendChild(listEl);

  const paginEl = document.createElement('div');
  paginEl.className = 'md:shrink-0 md:pt-3 md:border-t md:border-border-subtle';
  if (!infinite) sectionEl.appendChild(paginEl);

  listEl.innerHTML = [1,2,3,4,5].map(() => '<div class="h-14 mx-0 my-1 skeleton rounded-lg"></div>').join('');

  let result;
  if (!_isLocal && _allRemoteChapters !== null) {
    result = null;
  } else {
    try {
      result = _isLocal
        ? await api.getLocalChapters(_dbId, _page, _chapterPageSize, _sortOrder, _abort?.signal, {
            filterDownloaded: _filterDownloaded ? true : null,
            filterUnread: _filterUnread ? true : null,
            filterScanlator: _filterScanlator,
            filterOrphaned: _filterOrphaned ? true : null,
          })
        : await api.getRemoteChapters(_sid, _mangaId, _page, _chapterPageSize, _abort?.signal,
            _remoteChapterSorts?.length ? _remoteSort : null);
    } catch (e) {
      if (/** @type {any} */(e)?.name === 'AbortError') return;
      finishLoading();
      listEl.innerHTML = '';
      const msg = /** @type {any} */ (e)?.code === 'source_disabled'
        ? 'Extension is disabled — enable it in Settings > Sources.'
        : 'Failed to load chapters.';
      listEl.appendChild(createErrorState({ message: msg }));
      return;
    }
  }

  listEl.innerHTML = '';
  finishLoading();

  if (_isLocal && !_orphanCountLoaded) {
    _refreshOrphanCount().then(() => { if (_orphanCount > 0) _renderChapterList(); });
  }

  if (!_isLocal && result !== null && _allRemoteChapters === null && _page === 1) {
    const raw = Array.isArray(result?.chapters) ? result.chapters : Array.isArray(result) ? result : [];
    const serverPaged = result?.has_next_page === true || result?.has_next === true;
    if (!serverPaged) _allRemoteChapters = _sortChaptersClientSide(raw.map(_mapChapter), _sortOrder);
  }

  let mapped, hasNext;

  if (!_isLocal && _allRemoteChapters !== null) {
    const start = (_page - 1) * _chapterPageSize;
    mapped = _allRemoteChapters.slice(start, start + _chapterPageSize);
    hasNext = start + _chapterPageSize < _allRemoteChapters.length;
    if (mapped.length === 0 && _allRemoteChapters.length === 0) {
      listEl.appendChild(createEmptyState({ icon: iconDocument, title: 'No chapters found.' }));
      return;
    }
  } else {
    const rawChapters = Array.isArray(result?.chapters) ? result.chapters : Array.isArray(result) ? result : [];
    if (rawChapters.length === 0 && _chapters.length === 0) {
      listEl.appendChild(createEmptyState({ icon: iconDocument, title: 'No chapters found.' }));
      return;
    }
    mapped = rawChapters.map(_mapChapter);
    hasNext = hasNextPage(result, rawChapters.length, _chapterPageSize);
  }

  const totalCount = _isLocal
    ? (typeof result?.total === 'number' ? result.total : null)
    : (_allRemoteChapters !== null ? _allRemoteChapters.length : null);
  if (totalCount != null && totalCount > 0 && totalCount !== _chapterTotal) {
    _chapterTotal = totalCount;
    _tabsHandle?.update(_activeTab, _tabDefs());
  }

  if (infinite) {
    if (!_isLocal && _allRemoteChapters !== null) {
      const start = (_page - 1) * _chapterPageSize;
      _chapters = _allRemoteChapters.slice(0, start + _chapterPageSize);
      _chaptersHasMore = start + _chapterPageSize < _allRemoteChapters.length;
    } else {
      _chapters = [..._chapters, ...mapped];
      _chaptersHasMore = hasNext;
    }
    _listContainerEl = listEl;
    _renderChapterList();
    _chapterResizeListener = () => _renderChapterList();
    window.addEventListener('resize', _chapterResizeListener);
  } else {
    _chapters = mapped;
    _chaptersHasMore = false;
    if (_page > 1 || hasNext) {
      const { destroy } = renderPagination(paginEl, {
        page: _page,
        hasNext,
        total: !_isLocal && _allRemoteChapters !== null
          ? Math.ceil(_allRemoteChapters.length / _chapterPageSize)
          : result?.total_pages ?? undefined,
        onPageChange: (p) => { _page = p; _updateUrl(); _fetchChapters(sectionEl); },
      });
      _destroyPagination = destroy;
      _paginEl = paginEl;
    } else {
      _paginEl = null;
    }
    _listContainerEl = listEl;
    _renderChapterList();
    _chapterResizeListener = () => _renderChapterList();
    window.addEventListener('resize', _chapterResizeListener);
  }
}

function _renderChapterList() {
  if (!_listContainerEl) return;
  const readerHrefFn = (ch) => _isLocal
    ? `/reader/${ch.id}`
    : `/source/${_sid}/manga/${encodeURIComponent(_mangaId)}/chapter/${encodeURIComponent(ch.source_chapter_id ?? ch.id)}`;
  const displayChapters = _filterCached ? _chapters.filter(ch => _cachedChapterIds.has(ch.id)) : _chapters;
  const paginH = _paginEl ? (_paginEl.offsetHeight + 12) : 0;
  const height = window.innerWidth >= 1024
    ? (() => {
        const section = _listContainerEl.parentElement;
        const bottom = section
          ? section.getBoundingClientRect().bottom
          : window.innerHeight - 48;
        // No 200 px floor: on a short window that floor was larger than the
        // space actually left, which pushed the page into overflow — the one
        // thing this layout exists to prevent. The list takes what there is.
        const available = Math.max(64, bottom - _listContainerEl.getBoundingClientRect().top - paginH - 12);
        const needed = displayChapters.length * readChapterRowHeight() + (_chaptersHasMore ? 48 : 0);
        return Math.min(available, Math.max(needed, 120));
      })()
    : undefined;
  render(html`<${VirtualChapterList}
    chapters=${displayChapters}
    readerHrefFn=${readerHrefFn}
    inLibrary=${_isLocal}
    mangaId=${_dbId || null}
    notedChapterIds=${_notedChapterIds}
    hasMore=${_chaptersHasMore}
    loading=${_chaptersLoading}
    canDownload=${hasPermission('chapter:download')}
    canDelete=${hasPermission('chapter:delete')}
    allSelectedProp=${_allSelected}
    onLoadMore=${_loadMoreChapters}
    onToggleRead=${(id, isRead) => {
      const ch = _chapters.find(c => c.id === id);
      if (!ch) return;
      const coalesce = getLocal('kani_coalesce_read') === 'true';
      if (coalesce && ch.chapter_number != null) {
        const siblingIds = _chapters.filter(c => c.id !== id && c.chapter_number === ch.chapter_number).map(c => c.id);
        if (siblingIds.length) api.setChapterReadStatus(siblingIds, isRead).catch(() => {});
        _chapters = _chapters.map(c => c.chapter_number === ch.chapter_number ? { ...c, read: isRead, last_page_read: isRead ? 0 : c.last_page_read } : c);
      } else {
        _chapters = _chapters.map(c => c.id === id ? { ...c, read: isRead, last_page_read: isRead ? 0 : c.last_page_read } : c);
      }
      _renderChapterList();
    }}
    onMarkUpTo=${(chapterNumber, isRead) => {
      _chapters = _chapters.map(ch => {
        if (ch.chapter_number == null) return ch;
        if (isRead ? ch.chapter_number <= chapterNumber : ch.chapter_number >= chapterNumber) return { ...ch, read: isRead };
        return ch;
      });
      _renderChapterList();
    }}
    selectMode=${_selectMode}
    selected=${_selected}
    onToggleSelect=${(id) => {
      if (_selected.has(id)) { _selected.delete(id); _allSelected = false; } else _selected.add(id);
      _renderChapterList();
    }}
    onSelectAll=${async () => {
      let allIds;
      if (_isLocal) {
        const res = await api.getChapterIds(_dbId, { filterDownloaded: _filterDownloaded ? true : null, filterUnread: _filterUnread ? true : null, filterScanlator: _filterScanlator, sortOrder: _sortOrder }).catch(() => null);
        allIds = res?.ids ?? _chapters.map(ch => ch.id);
      } else {
        allIds = _chapters.map(ch => ch.id);
      }
      const allAlreadySelected = allIds.every(id => _selected.has(id));
      if (allAlreadySelected) { _selected.clear(); _allSelected = false; }
      else { for (const id of allIds) _selected.add(id); _allSelected = true; }
      _renderChapterList();
    }}
    onFlipSelection=${async () => {
      let allIds;
      if (_isLocal) {
        const res = await api.getChapterIds(_dbId, { filterDownloaded: _filterDownloaded ? true : null, filterUnread: _filterUnread ? true : null, filterScanlator: _filterScanlator, sortOrder: _sortOrder }).catch(() => null);
        allIds = res?.ids ?? _chapters.map(ch => ch.id);
      } else {
        allIds = _chapters.map(ch => ch.id);
      }
      _selected = new Set(allIds.filter(id => !_selected.has(id)));
      _allSelected = false;
      _renderChapterList();
    }}
    onSelectUndownloaded=${async () => {
      let ids;
      if (_isLocal) {
        const res = await api.getChapterIds(_dbId, { filterDownloaded: false, sortOrder: _sortOrder }).catch(() => null);
        ids = res?.ids ?? _chapters.filter(ch => !ch.downloaded).map(ch => ch.id);
      } else {
        ids = _chapters.filter(ch => !ch.downloaded).map(ch => ch.id);
      }
      _selected = new Set(ids); _allSelected = false; _renderChapterList();
    }}
    orphanCount=${_orphanCount}
    onSelectOrphaned=${async () => {
      const res = await api.getChapterIds(_dbId, { filterOrphaned: true, sortOrder: _sortOrder }).catch(() => null);
      const ids = res?.ids ?? _chapters.filter(ch => ch.is_orphaned).map(ch => ch.id);
      _selected = new Set(ids); _allSelected = false; _renderChapterList();
    }}
    onSelectUnread=${async () => {
      let ids;
      if (_isLocal) {
        const res = await api.getChapterIds(_dbId, { filterUnread: true, sortOrder: _sortOrder }).catch(() => null);
        ids = res?.ids ?? _chapters.filter(ch => !ch.read).map(ch => ch.id);
      } else {
        ids = _chapters.filter(ch => !ch.read).map(ch => ch.id);
      }
      _selected = new Set(ids); _allSelected = false; _renderChapterList();
    }}
    onBulkRead=${async (isRead) => {
      const ids = [..._selected];
      if (!ids.length) return;
      try {
        await api.setChapterReadStatus(ids, isRead);
        const idSet = new Set(ids);
        _chapters = _chapters.map(ch => idSet.has(ch.id) ? { ...ch, read: isRead } : ch);
        showToast(t('manga.details.bulk.read', { count: ids.length, s: ids.length !== 1 ? 's' : '', status: isRead ? t('manga.details.bulk.read.read') : t('manga.details.bulk.read.unread') }));
        _selected.clear(); _selectMode = false; _allSelected = false; _renderChapterList();
      } catch (err) { showApiError(err); }
    }}
    onBulkDownload=${async () => {
      const ids = [..._selected].filter(id => { const ch = _chapters.find(c => c.id === id); return ch && !ch.downloaded; });
      if (!ids.length) return;
      await Promise.allSettled(ids.map(id => api.downloadChapter(id)));
      _selected.clear(); _selectMode = false; _allSelected = false; _renderChapterList();
      showToast(t('manga.details.bulk.download', { count: ids.length, s: ids.length !== 1 ? 's' : '' }));
    }}
    onBulkDelete=${async () => {
      const ids = [..._selected].filter(id => { const ch = _chapters.find(c => c.id === id); return ch && ch.downloaded; });
      if (!ids.length) return;
      const ok = await showConfirm(t('manga.details.bulk.delete.body'), {
        title: t('manga.details.bulk.delete.confirm', { count: ids.length, s: ids.length !== 1 ? 's' : '' }),
        confirmLabel: t('common.delete'),
        danger: true,
      });
      if (!ok) return;
      await Promise.allSettled(ids.map(id => api.deleteChapter(id)));
      const idSet = new Set(ids);
      _chapters = _chapters.filter(ch => !(idSet.has(ch.id) && ch.is_orphaned)).map(ch => idSet.has(ch.id) ? { ...ch, download_status: 0, page_count: null, downloaded: false } : ch);
      _selected.clear(); _selectMode = false; _allSelected = false; _renderChapterList();
      _refreshOrphanCount().then(_renderChapterList);
      showToast(t('manga.details.bulk.deleted', { count: ids.length, s: ids.length !== 1 ? 's' : '' }));
    }}
    onExitSelect=${() => { _selectMode = false; _selected.clear(); _allSelected = false; _renderChapterList(); }}
    onEnterSelectWithChapter=${(id) => { _selectMode = true; _selected.clear(); _allSelected = false; _selected.add(id); _renderChapterList(); }}
    onDelete=${(id) => {
      const ch = _chapters.find(c => c.id === id);
      if (!ch) return;
      if (ch.is_orphaned) { _chapters = _chapters.filter(c => c.id !== id); }
      else { _chapters = _chapters.map(c => c.id === id ? { ...c, download_status: 0, page_count: null, downloaded: false } : c); }
      _renderChapterList();
    }}
    cachedChapterIds=${_cachedChapterIds}
    kccAvailable=${_kccAvailable}
    onCacheChange=${(id, isCached) => {
      const next = new Set(_cachedChapterIds);
      if (isCached) next.add(id); else next.delete(id);
      _cachedChapterIds = next;
      _renderChapterList();
    }}
    height=${height}
  />`, _listContainerEl);
}

async function _loadMoreChapters() {
  if (_chaptersLoading || !_chaptersHasMore || !_listContainerEl) return;
  _chaptersLoading = true;
  _renderChapterList();
  _page++;
  _updateUrl();

  if (!_isLocal && _allRemoteChapters !== null) {
    const end = _page * _chapterPageSize;
    _chapters = _allRemoteChapters.slice(0, end);
    _chaptersHasMore = end < _allRemoteChapters.length;
    _chaptersLoading = false;
    _renderChapterList();
    return;
  }

  try {
    const result = _isLocal
      ? await api.getLocalChapters(_dbId, _page, _chapterPageSize, _sortOrder, _abort?.signal, {
          filterDownloaded: _filterDownloaded ? true : null,
          filterUnread: _filterUnread ? true : null,
          filterScanlator: _filterScanlator,
          filterOrphaned: _filterOrphaned ? true : null,
        })
      : await api.getRemoteChapters(_sid, _mangaId, _page, _chapterPageSize, _abort?.signal,
          _remoteChapterSorts?.length ? _remoteSort : null);
    const rawChapters = Array.isArray(result?.chapters) ? result.chapters : Array.isArray(result) ? result : [];
    const mapped = rawChapters.map(_mapChapter);
    _chapters = [..._chapters, ...mapped];
    _chaptersHasMore = hasNextPage(result, rawChapters.length, _chapterPageSize);
  } catch (e) {
    if (/** @type {any} */(e)?.name !== 'AbortError') console.error('Failed to load more chapters:', e);
    _page--;
    _updateUrl();
  }
  _chaptersLoading = false;
  _renderChapterList();
}


/** @param {HTMLElement} container */
export function destroy(container) {
  _abort?.abort();
  _abort = null;
  if (_sseListener) { window.removeEventListener('kani:sse', _sseListener); _sseListener = null; }
  if (_escHandler) { document.removeEventListener('keydown', _escHandler); _escHandler = null; }
  _clearStreamingCounter();
  _unsubscribeCacheMsgs?.();
  _unsubscribeCacheMsgs = null;
  _destroyPagination?.();
  _destroyPagination = null;
  _destroyHeader?.();
  _destroyHeader = null;
  if (_chapterResizeListener) { window.removeEventListener('resize', _chapterResizeListener); _chapterResizeListener = null; }
  if (_manageResizeListener) { window.removeEventListener('resize', _manageResizeListener); _manageResizeListener = null; }
  if (_listContainerEl) { render(null, _listContainerEl); _listContainerEl = null; }
  for (const el of _toolbarMounts) render(null, el);
  _toolbarMounts = [];
  _chapters = [];
  _chaptersHasMore = false;
  _chaptersLoading = false;
  _allRemoteChapters = null;
  _unmountMigration?.();
  _unmountMigration = null;
  _activeTab = 'chapters';
  _manageMounted = false;
  _contentSection = null;
  clearPageHeader();
  container.innerHTML = '';
}
