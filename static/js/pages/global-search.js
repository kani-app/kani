// @ts-check

import * as api from '../api.js';
import { hasPermission } from '../session.js';
import { navigate } from '../router.js';
import { getParam, replaceState as urlReplaceState } from '../url-params.js';
import { debounce, escapeHtml } from '../utils.js';
import { startLoading, finishLoading } from '../components/page-loading-bar.js';
import { createErrorState } from '../components/error-state.js';
import { createEmptyState } from '../components/empty-state.js';
import { createMangaCard } from '../components/manga-card.js';
import { createSearchInput } from '../components/form/search-input.js';
import { iconSearch, iconChevronLeft, iconChevronRight } from '../icons.js';
import { setPageHeader, clearPageHeader } from '../components/app-header.js';
import { t } from '../i18n.js';
import { TileSizeSelect } from '../components/tile-size-select.js';
import { getTileSize, setTileSize, tileWidth } from '../tile-size.js';
import { h, render } from 'preact';
import htm from 'htm';

const html = htm.bind(h);

/** Floor on how many results are asked of each source. */
const PER_SOURCE_PAGE = 24;

let _query = '';
/** @type {'FavouritedOnly'|'AllEnabled'|{Sources: number[]}} */
let _scope = 'FavouritedOnly';
/** @type {AbortController|null} */ let _abort = null;
/** @type {any[]} */               let _sources = [];
/** Per-source pagination state: sourceId → { page, hasNext, loading } */
/** @type {Map<number, { page: number, hasNext: boolean, loading: boolean }>} */
let _sourcePages = new Map();
/** @type {Map<number, IntersectionObserver>} */
let _sourceObservers = new Map();
/** Per-rail nav sync, re-run when the tile size or the window changes. */
/** @type {Array<() => void>} */ let _navUpdaters = [];
/** @type {HTMLElement|null} */ let _heroEl = null;


/** @param {HTMLElement} container */
export async function init(container) {
  document.title = 'Search - Kani';
  _query = getParam('q') ?? '';
  _scope = 'FavouritedOnly';
  const scopeParam = getParam('scope');
  if (scopeParam === 'AllEnabled') {
    _scope = 'AllEnabled';
  } else if (scopeParam?.startsWith('sources:')) {
    const ids = scopeParam.slice(8).split(',').map(Number).filter(Boolean);
    if (ids.length) _scope = { Sources: ids };
  }
  setPageHeader({ crumbs: [{ label: t('global_search.crumb') }] });

  if (!hasPermission('source:browse')) {
    container.innerHTML = '';
    container.appendChild(createErrorState({ message: t('global_search.no_permission') }));
    return;
  }

  container.classList.add('page-fixed');
  container.innerHTML = `
    <div class="w-full overflow-x-hidden px-4 md:px-6 py-4 md:py-6 flex flex-col gap-3 page-body-host page-col">
      <!-- Search bar, centred, with the tile-size control on its own line's end -->
      <div class="js-hero flex flex-col items-center gap-3 py-4 md:py-8 transition-[padding] duration-200">
        <div class="w-full grid grid-cols-1 md:grid-cols-[1fr_minmax(0,42rem)_1fr] items-center gap-3">
          <div class="hidden md:block"></div>
          <div class="js-search-slot w-full"></div>
          <div class="flex justify-center md:justify-end"><div class="js-tile-size-mount w-28"></div></div>
        </div>
        <!-- Scope chips -->
        <div class="flex flex-wrap justify-center gap-2" id="scope-chips" role="group" aria-label="${t('global_search.scope.label')}"></div>
      </div>

      <!-- Results -->
      <div id="search-results" class="page-body pb-nav-safe" aria-live="polite" aria-busy="false"></div>
    </div>
  `;

  const searchSlot = /** @type {HTMLElement} */ (container.querySelector('.js-search-slot'));
  const { el: searchEl, input: searchInput } = createSearchInput({
    value: _query,
    placeholder: t('global_search.placeholder'),
    ariaLabel: t('global_search.input.label'),
    inputClass: 'h-12 text-base',
    id: 'search-input',
  });
  searchSlot.appendChild(searchEl);
  searchInput.focus();

  _heroEl = /** @type {HTMLElement} */ (container.querySelector('.js-hero'));
  const chipsEl     = /** @type {HTMLElement} */ (container.querySelector('#scope-chips'));
  const resultsEl   = /** @type {HTMLElement} */ (container.querySelector('#search-results'));

  try {
    const all = await api.getSources();
    _sources = (Array.isArray(all) ? all : []).filter(s => s.enabled);
  } catch {
    _sources = [];
  }

  function _updateUrl() {
    /** @type {Record<string, string>} */
    const params = {};
    if (_query) params.q = _query;
    if (_scope === 'AllEnabled') params.scope = 'AllEnabled';
    else if (typeof _scope === 'object' && 'Sources' in _scope)
      params.scope = `sources:${_scope.Sources.join(',')}`;
    urlReplaceState(params);
  }

  const tileMountEl = /** @type {HTMLElement} */ (container.querySelector('.js-tile-size-mount'));
  const _renderTileSize = () => {
    render(html`<${TileSizeSelect}
      value=${getTileSize()}
      onChange=${(/** @type {import('../tile-size.js').TileSize} */ size) => {
        setTileSize(size);
        _renderTileSize();
        // Tiles resize through the token alone; only the scroll affordances
        // need recomputing once the new widths are laid out.
        requestAnimationFrame(() => { for (const update of _navUpdaters) update(); });
      }}
    />`, tileMountEl);
  };
  _renderTileSize();

  _renderChips(chipsEl, resultsEl);

  if (_query) _fetchSearch(resultsEl);
  else _renderPreQueryState(resultsEl);

  const _debouncedSearch = debounce(() => {
    _query = searchInput.value.trim();
    _updateUrl();
    if (_query) _fetchSearch(resultsEl);
    else _renderPreQueryState(resultsEl);
  }, 500);

  searchInput.addEventListener('input', _debouncedSearch);

  function _renderChips(el, results) {
    el.innerHTML = '';

    const isFav = _scope === 'FavouritedOnly';
    const isAll = _scope === 'AllEnabled';
    const sourcesScope = typeof _scope === 'object' && 'Sources' in _scope ? _scope.Sources : null;

    const mkChip = (label, active, onClick) => {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = active ? 'chip chip-active' : 'chip';
      btn.textContent = label;
      btn.setAttribute('aria-pressed', String(active));
      btn.addEventListener('click', onClick);
      el.appendChild(btn);
    };

    mkChip(t('global_search.scope.favourites'), isFav, () => {
      _scope = 'FavouritedOnly';
      _updateUrl();
      _renderChips(el, results);
      if (_query) _fetchSearch(results);
    });

    mkChip(t('global_search.scope.all_enabled'), isAll, () => {
      _scope = 'AllEnabled';
      _updateUrl();
      _renderChips(el, results);
      if (_query) _fetchSearch(results);
    });

    for (const src of _sources) {
      const isActive = sourcesScope?.includes(src.id) ?? false;
      mkChip(src.name, isActive, () => {
        if (sourcesScope) {
          const next = isActive
            ? sourcesScope.filter(id => id !== src.id)
            : [...sourcesScope, src.id];
          _scope = next.length === 0 ? 'AllEnabled' : { Sources: next };
        } else {
          _scope = { Sources: [src.id] };
        }
        _updateUrl();
        _renderChips(el, results);
        if (_query) _fetchSearch(results);
      });
    }
  }
}

/** @param {HTMLElement} resultsEl */
function _renderPreQueryState(resultsEl) {
  _setHeroCompact(false);
  resultsEl.innerHTML = '';
  resultsEl.setAttribute('aria-busy', 'false');
  resultsEl.appendChild(createEmptyState({
    icon: iconSearch,
    title: t('global_search.prequery.title'),
    subtitle: t('global_search.prequery.subtitle'),
  }));
}


/** @param {HTMLElement} resultsEl */
async function _fetchSearch(resultsEl) {
  _abort?.abort();
  _abort = new AbortController();

  _setHeroCompact(true);
  resultsEl.innerHTML = `<div class="flex flex-col gap-8">${[1,2,3].map(() => `
    <div class="flex flex-col gap-3">
      <div class="skeleton h-4 w-32 rounded"></div>
      <div class="manga-row-wrapper"><div class="manga-row">${[1,2,3,4,5,6].map(() =>
        `<div class="manga-row__item"><div class="skeleton rounded-sm w-full aspect-[2/3]"></div></div>`).join('')}</div></div>
    </div>
  `).join('')}</div>`;
  resultsEl.setAttribute('aria-busy', 'true');
  startLoading();

  let result;
  try {
    result = await api.globalSearch(_query, _scope, 1, PER_SOURCE_PAGE, _abort.signal);
  } catch (e) {
    if (e?.name === 'AbortError') return;
    resultsEl.innerHTML = '';
    resultsEl.setAttribute('aria-busy', 'false');
    finishLoading();
    resultsEl.appendChild(createErrorState({ message: t('global_search.error') }));
    return;
  }

  finishLoading();
  resultsEl.innerHTML = '';
  resultsEl.setAttribute('aria-busy', 'false');

  const sourceResults = Array.isArray(result?.results) ? result.results
    : Array.isArray(result)                             ? result
    : [];

  if (sourceResults.length === 0) {
    resultsEl.appendChild(createEmptyState({
      icon: iconSearch,
      title: t('global_search.empty.title'),
      subtitle: t('global_search.empty.subtitle'),
    }));
    return;
  }

  _sourcePages = new Map();
  _navUpdaters = [];
  for (const sr of sourceResults) {
    _sourcePages.set(sr.source_id, { page: 1, hasNext: sr.has_next_page ?? false, loading: false });
  }

  const wrap = document.createElement('div');
  wrap.className = 'flex flex-col gap-8 min-w-0';
  wrap.setAttribute('role', 'list');

  for (const sourceResult of sourceResults) {
    const sid = sourceResult.source_id;
    const section = document.createElement('div');
    section.className = 'flex flex-col gap-3';
    section.setAttribute('role', 'listitem');

    const loaded = sourceResult.manga?.length ?? 0;
    const count = loaded > 0
      ? t('global_search.source.count', { count: loaded, more: sourceResult.has_next_page ? '+' : '' })
      : '';
    const header = document.createElement('div');
    header.className = 'flex items-baseline justify-between gap-3';
    header.innerHTML = `
      <h2 class="eyebrow flex items-baseline gap-2 min-w-0">
        <span class="truncate">${escapeHtml(sourceResult.source_name ?? String(sid))}</span>
        <span class="font-normal normal-case tracking-normal">${escapeHtml(count)}</span>
      </h2>
      <a href="/source/${encodeURIComponent(sid)}?q=${encodeURIComponent(_query)}" class="text-xs text-accent hover:underline focus-visible:outline-none focus-visible:underline shrink-0">${t('global_search.see_all')}</a>
    `;
    section.appendChild(header);

    {
      const wrapper = document.createElement('div');
      wrapper.className = 'manga-row-wrapper';

      const navLeft = document.createElement('button');
      navLeft.type = 'button';
      navLeft.className = 'manga-row-nav';
      navLeft.setAttribute('data-dir', 'left');
      navLeft.setAttribute('aria-label', t('global_search.nav.prev'));
      navLeft.disabled = true;
      navLeft.innerHTML = iconChevronLeft;

      const navRight = document.createElement('button');
      navRight.type = 'button';
      navRight.className = 'manga-row-nav';
      navRight.setAttribute('data-dir', 'right');
      navRight.setAttribute('aria-label', t('global_search.nav.next'));
      navRight.disabled = true;
      navRight.innerHTML = iconChevronRight;

      const row = document.createElement('div');
      row.className = 'manga-row';
      row.setAttribute('role', 'list');
      // Focusable so the rail can be scrolled with the arrow keys; without it a
      // keyboard user cannot reach anything past the first screenful.
      row.tabIndex = 0;
      row.setAttribute('aria-label', sourceResult.source_name ?? String(sid));

      /** Sync the arrows and the edge fades to the current scroll position. */
      function _updateNav() {
        const atStart = row.scrollLeft <= 2;
        const atEnd   = row.scrollLeft + row.clientWidth >= row.scrollWidth - 2;
        navLeft.disabled  = atStart;
        navRight.disabled = atEnd;
        row.classList.toggle('is-scrollable-start', !atStart);
        row.classList.toggle('is-scrollable-end', !atEnd);
      }

      if (sourceResult.manga?.length) {
        _appendCards(row, sid, sourceResult.manga);

        // Sentinel triggers append-on-scroll for this row
        const sentinel = document.createElement('div');
        sentinel.className = 'js-sentinel w-px shrink-0 self-stretch';
        row.appendChild(sentinel);

        const { hasNext: initialHasNext } = _sourcePages.get(sid) ?? { hasNext: false };
        if (initialHasNext) _observeRow(row, sentinel, sid, _updateNav);
      } else {
        row.appendChild(_rowNotice(sourceResult, () => _refetchSource(sid, row, _updateNav)));
      }

      _navUpdaters.push(_updateNav);
      row.addEventListener('scroll', _updateNav, { passive: true });
      // Re-check after images load (layout may shift)
      requestAnimationFrame(_updateNav);

      navLeft.addEventListener('click', () => {
        row.scrollBy({ left: -_pageStep(row), behavior: 'smooth' });
      });

      navRight.addEventListener('click', () => {
        row.scrollBy({ left: _pageStep(row), behavior: 'smooth' });
      });

      wrapper.appendChild(navLeft);
      wrapper.appendChild(row);
      wrapper.appendChild(navRight);
      section.appendChild(wrapper);
    }

    wrap.appendChild(section);
  }

  resultsEl.appendChild(wrap);
}


/**
 * The hero is generous while the page is empty and compact once it has rails —
 * on a fixed-height page that padding is height the results do not get.
 *
 * @param {boolean} compact
 */
function _setHeroCompact(compact) {
  if (!_heroEl) return;
  _heroEl.classList.toggle('py-4', !compact);
  _heroEl.classList.toggle('md:py-8', !compact);
  _heroEl.classList.toggle('py-1', compact);
}

/**
 * @param {HTMLElement} row
 * @param {number} sid
 * @param {any[]} list
 * @param {Node | null} [before] Insert ahead of this node, e.g. the sentinel.
 */
function _appendCards(row, sid, list, before = null) {
  for (const manga of list) {
    const navId  = manga.source_manga_id ?? manga.id;
    const cardId = manga.db_id ?? manga.id;
    const card = createMangaCard({
      manga: { id: cardId, title: manga.title, source_id: sid, cover_image_url: manga.cover_url ?? null },
      href: `/source/${sid}/manga/${encodeURIComponent(navId)}`,
      extraClass: 'manga-row__item',
    });
    card.setAttribute('role', 'listitem');
    row.insertBefore(card, before);
  }
}

/**
 * @param {HTMLElement} row
 * @param {number} count
 * @param {Node | null} [before]
 * @returns {HTMLElement[]} The placeholders, for the caller to remove.
 */
function _appendSkeletons(row, count, before = null) {
  const skels = [];
  for (let i = 0; i < count; i++) {
    const cell = document.createElement('div');
    cell.className = 'manga-row__item';
    const inner = document.createElement('div');
    inner.className = 'skeleton rounded-sm w-full aspect-[2/3]';
    cell.appendChild(inner);
    row.insertBefore(cell, before);
    skels.push(cell);
  }
  return skels;
}

/**
 * Enough to fill the rail twice over, so there is always something to scroll
 * to — a wide window at the smallest tile would otherwise show a full row with
 * nothing past its edge.
 *
 * @param {HTMLElement} row
 * @returns {number}
 */
function _perSourceCount(row) {
  const tile = tileWidth();
  if (tile <= 0) return PER_SOURCE_PAGE;
  const gap = parseFloat(getComputedStyle(row).columnGap) || 0;
  const perScreen = Math.max(1, Math.floor((row.clientWidth + gap) / (tile + gap)));
  return Math.max(PER_SOURCE_PAGE, perScreen * 2);
}

/**
 * Retries one source after it failed, leaving the other rails alone.
 *
 * @param {number} sid
 * @param {HTMLElement} row
 * @param {() => void} onUpdate
 */
async function _refetchSource(sid, row, onUpdate) {
  row.innerHTML = '';
  _appendSkeletons(row, 4);
  try {
    const res = await api.searchManga(sid, _query, 1, _perSourceCount(row), undefined, _abort?.signal);
    const manga = Array.isArray(res?.manga) ? res.manga : Array.isArray(res) ? res : [];
    const hasNext = res?.has_next_page ?? false;
    _sourcePages.set(sid, { page: 1, hasNext, loading: false });
    row.innerHTML = '';
    if (manga.length === 0) {
      row.appendChild(_rowNotice({ error: null }, () => {}));
    } else {
      _appendCards(row, sid, manga);
      if (hasNext) {
        const sentinel = document.createElement('div');
        sentinel.className = 'js-sentinel w-px shrink-0 self-stretch';
        row.appendChild(sentinel);
        _observeRow(row, sentinel, sid, onUpdate);
      }
    }
  } catch (e) {
    if (/** @type {any} */ (e)?.name === 'AbortError') return;
    row.innerHTML = '';
    row.appendChild(_rowNotice({ error: String(e) }, () => _refetchSource(sid, row, onUpdate)));
  }
  requestAnimationFrame(onUpdate);
}

/**
 * How far one arrow press scrolls: whole tiles, so a press never leaves a
 * sliced cover against the edge.
 *
 * @param {HTMLElement} row
 * @returns {number} Pixels.
 */
function _pageStep(row) {
  const tile = tileWidth();
  if (tile <= 0) return row.clientWidth * 0.8;
  const gap = parseFloat(getComputedStyle(row).columnGap) || 0;
  const perScreen = Math.max(1, Math.floor((row.clientWidth + gap) / (tile + gap)));
  return perScreen * (tile + gap);
}

/**
 * The rail's content when a source contributed nothing. A source that failed
 * and a source that matched nothing are different facts, and only one of them
 * is worth retrying.
 *
 * @param {{ error?: string | null }} sourceResult
 * @param {() => void} onRetry
 * @returns {HTMLElement}
 */
function _rowNotice(sourceResult, onRetry) {
  const notice = document.createElement('div');
  notice.className = 'manga-row__notice';

  const text = document.createElement('p');
  text.textContent = sourceResult.error
    ? t('global_search.source.failed')
    : t('global_search.source.empty');
  notice.appendChild(text);

  if (sourceResult.error) {
    const retry = document.createElement('button');
    retry.type = 'button';
    retry.className = 'btn-secondary btn-sm';
    retry.textContent = t('global_search.source.retry');
    retry.addEventListener('click', onRetry);
    notice.appendChild(retry);
  }
  return notice;
}

/**
 * @param {HTMLElement} row
 * @param {HTMLElement} sentinel
 * @param {number} sid
 */
function _observeRow(row, sentinel, sid, onUpdate) {
  _sourceObservers.get(sid)?.disconnect();

  const observer = new IntersectionObserver(async ([entry]) => {
    if (!entry.isIntersecting) return;
    const state = _sourcePages.get(sid);
    if (!state?.hasNext || state?.loading) return;

    _sourcePages.set(sid, { ...state, loading: true });

    const skels = _appendSkeletons(row, 4, sentinel);

    try {
      const res = await api.searchManga(sid, _query, state.page + 1, _perSourceCount(row), undefined, _abort?.signal);
      const manga = Array.isArray(res?.manga) ? res.manga : Array.isArray(res) ? res : [];
      const nextHasNext = res?.has_next_page ?? false;
      _sourcePages.set(sid, { page: state.page + 1, hasNext: nextHasNext, loading: false });
      skels.forEach(s => s.remove());
      _appendCards(row, sid, manga, sentinel);
      if (!nextHasNext) {
        observer.disconnect();
        _sourceObservers.delete(sid);
        sentinel.remove();
      }
      requestAnimationFrame(onUpdate);
    } catch (e) {
      if (e?.name !== 'AbortError') {
        _sourcePages.set(sid, { ...state, loading: false });
      }
      skels.forEach(s => s.remove());
    }
  }, { root: row, rootMargin: '0px 200px 0px 0px' });

  observer.observe(sentinel);
  _sourceObservers.set(sid, observer);
}


/** @param {HTMLElement} container */
export function destroy(container) {
  clearPageHeader();
  _abort?.abort();
  _abort = null;
  _sourcePages = new Map();
  for (const obs of _sourceObservers.values()) obs.disconnect();
  _sourceObservers = new Map();
  _navUpdaters = [];
  _heroEl = null;
  container.innerHTML = '';
}
