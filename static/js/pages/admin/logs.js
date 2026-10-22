// @ts-check

import { h, render } from 'preact';
import htm from 'htm';
import * as api from '../../api.js';
import { hasPermission } from '../../session.js';
import { escapeHtml, debounce } from '../../utils.js';
import { renderTabs } from '../../components/tabs.js';
import { renderChipGroup } from '../../components/chip-group.js';
import { DateRange } from '../../components/form/date-range.js';
import { renderPagination } from '../../components/pagination.js';
import { showConfirm } from '../../components/modal.js';
import { showToast, showApiError } from '../../components/toast.js';
import { setPageHeader, clearPageHeader } from '../../components/app-header.js';
import { startLoading, finishLoading } from '../../components/page-loading-bar.js';
import { createErrorState } from '../../components/error-state.js';
import { iconDownload } from '../../icons.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);


const LOG_LEVELS = ['ERROR', 'WARN', 'INFO', 'DEBUG', 'TRACE'];
// Semantic severity only: red means error, amber means warning. INFO is ordinary
// text — colouring it accent made every log line read as an alarm.
const LEVEL_CLASSES = {
  ERROR: 'text-danger font-semibold',
  WARN:  'text-warn font-semibold',
  INFO:  'text-text',
  DEBUG: 'text-text-muted',
  TRACE: 'text-text-muted opacity-70',
};

const PAGE_SIZE = 100;
const MAX_LIVE_ROWS = 500;


/** @type {HTMLElement | null} */    let _container = null;
/** @type {(() => void) | null} */  let _destroyTabs = null;
/** @type {(() => void) | null} */  let _destroyPage = null;
/** @type {'app' | 'audit'} */      let _activeTab = 'app';
/** @type {EventSource | null} */   let _sse = null;


/** @param {HTMLElement} container */
export async function init(container) {
  document.title = 'Logs - Kani';
  _container = container;
  _activeTab = 'app';

  if (!hasPermission('admin:view_logs')) {
    container.innerHTML = `
      <div class="flex flex-col items-center justify-center gap-3 py-20 text-text-muted">
        <p class="text-base font-medium text-text">${escapeHtml(t('logs.access_denied'))}</p>
        <p class="text-sm">${escapeHtml(t('logs.access_denied.desc'))}</p>
      </div>
    `;
    return;
  }

  const downloadBtn = document.createElement('button');
  downloadBtn.className = 'btn-secondary btn-sm flex items-center gap-1.5';
  downloadBtn.innerHTML = `<span class="icon-xs">${iconDownload}</span>${escapeHtml(t('logs.download'))}`;
  downloadBtn.addEventListener('click', _handleDownload);

  const purgeBtn = document.createElement('button');
  purgeBtn.className = 'btn-secondary btn-sm text-danger';
  purgeBtn.textContent = t('logs.purge');
  purgeBtn.addEventListener('click', _handlePurge);

  const actions = document.createElement('div');
  actions.className = 'flex items-center gap-2';
  actions.append(downloadBtn, purgeBtn);

  setPageHeader({
    crumbs: [{ label: t('admin.crumb') }, { label: t('logs.crumb') }],
    actions,
  });

  const wrap = document.createElement('div');
  wrap.className = 'flex flex-col overflow-hidden flex-1 min-h-0';
  container.appendChild(wrap);

  const tabBar = document.createElement('div');
  tabBar.className = 'px-4 md:px-6 pt-4 shrink-0';
  wrap.appendChild(tabBar);

  const content = document.createElement('div');
  content.className = 'flex flex-col flex-1 overflow-hidden';
  wrap.appendChild(content);

  const tabsHandle = renderTabs(tabBar, {
    tabs: [
      { id: 'app',   name: t('logs.tab.app') },
      { id: 'audit', name: t('logs.tab.audit') },
    ],
    activeId: _activeTab,
    onSelect: (id) => {
      _stopSse();
      _activeTab = /** @type {'app' | 'audit'} */ (id);
      tabsHandle.update(id);
      _renderTab(content);
    },
  });
  _destroyTabs = tabsHandle.destroy;

  _renderTab(content);
}

/** @param {HTMLElement} _container */
export function destroy(_container) {
  _stopSse();
  _destroyTabs?.();
  _destroyTabs = null;
  _destroyPage?.();
  _destroyPage = null;
  clearPageHeader();
}


/** @param {HTMLElement} content */
function _renderTab(content) {
  _stopSse();
  _destroyPage?.();
  _destroyPage = null;
  content.innerHTML = '';

  if (_activeTab === 'app') {
    _destroyPage = _mountAppLogsTab(content);
  } else {
    _destroyPage = _mountAuditTab(content);
  }
}


/** @param {HTMLElement} container @returns {() => void} */
function _mountAppLogsTab(container) {
  /** @type {{ level: string[], search: string, from: string, to: string, live: boolean, page: number }} */
  let state = { level: [], search: '', from: '', to: '', live: false, page: 1 };
  /** @type {AbortController | null} */ let abort = null;
  /** @type {(() => void) | null} */   let destroyPagin = null;

  const root = document.createElement('div');
  root.className = 'flex flex-col flex-1 overflow-hidden';
  container.appendChild(root);

  const filterBar = document.createElement('div');
  filterBar.className = 'flex flex-wrap items-center gap-2 px-4 md:px-6 py-3 border-b border-border shrink-0';
  filterBar.innerHTML = `
    <div id="level-filters" class="w-full sm:w-auto shrink-0"></div>
    <input type="search" id="log-search" placeholder="${escapeHtml(t('logs.filter.search_placeholder'))}"
      class="input input-sm w-full sm:flex-1 min-w-32" value="" />
    <div id="log-dates" class="flex flex-wrap items-center gap-2 w-full sm:w-auto shrink-0"></div>
    <label class="flex items-center gap-2 text-sm cursor-pointer select-none ml-auto">
      <span>${t('logs.filter.live')}</span>
      <span class="kani-toggle">
        <input type="checkbox" id="log-live" class="kani-toggle__input" />
        <span class="kani-toggle__track"></span>
      </span>
    </label>
  `;
  root.appendChild(filterBar);

  const levelChips = renderChipGroup(
    /** @type {HTMLElement} */ (filterBar.querySelector('#level-filters')),
    {
      items: LOG_LEVELS.map(l => ({ id: l, label: l })),
      selected: new Set(),
      onToggle: () => {
        state.level = [...levelChips.selected()];
        state.page = 1;
        if (state.live) _startSseMode(); else _fetch();
      },
    },
  );

  const datesEl = /** @type {HTMLElement} */ (filterBar.querySelector('#log-dates'));
  const _renderDates = () => {
    render(html`
      <${DateRange} from=${state.from} to=${state.to}
        onChange=${(/** @type {{ from: string, to: string }} */ r) => {
          state.from = r.from; state.to = r.to; state.page = 1; _renderDates(); _fetch();
        }} />
    `, datesEl);
  };
  _renderDates();

  const tableWrap = document.createElement('div');
  tableWrap.className = 'flex-1 overflow-auto font-mono text-xs';
  root.appendChild(tableWrap);

  const logList = document.createElement('div');
  logList.id = 'log-list';
  logList.className = 'flex flex-col';
  tableWrap.appendChild(logList);

  const paginEl = document.createElement('div');
  paginEl.className = 'px-4 md:px-6 py-3 shrink-0';
  root.appendChild(paginEl);

  const searchEl = /** @type {HTMLInputElement} */ (filterBar.querySelector('#log-search'));
  const liveEl   = /** @type {HTMLInputElement} */ (filterBar.querySelector('#log-live'));

  const doSearch = debounce(() => { state.page = 1; _fetch(); }, 300);

  searchEl.addEventListener('input', () => { state.search = searchEl.value; doSearch(); });
  liveEl.addEventListener('change', () => {
    state.live = liveEl.checked;
    if (state.live) { _startSseMode(); } else { _stopSse(); _fetch(); }
  });

  function _startSseMode() {
    _stopSse();
    logList.innerHTML = '';
    paginEl.innerHTML = '';
    const levelParam = state.level.length ? state.level.join(',') : '';
    const url = `/rest/admin/logs/stream` + (levelParam ? `?level=${encodeURIComponent(levelParam)}` : '');
    _sse = new EventSource(url, { withCredentials: true });
    _sse.addEventListener('message', (e) => {
      try {
        const entry = JSON.parse(e.data);
        const row = _buildLogRow(entry);
        logList.insertBefore(row, logList.firstChild);
        // Cap DOM rows
        while (logList.children.length > MAX_LIVE_ROWS) {
          logList.removeChild(logList.lastChild);
        }
      } catch { }
    });
    _sse.onerror = () => {
      if (_sse?.readyState === EventSource.CLOSED) {
        showToast(t('logs.sse.disconnected'), 'warn');
      }
    };
  }

  async function _fetch() {
    if (state.live) return;
    abort?.abort();
    abort = new AbortController();
    startLoading();
    try {
      /** @type {{ entries: any[], total: number, page: number, page_size: number }} */
      const res = await api.getAdminLogs({
        level:     state.level.join(',') || undefined,
        search:    state.search || undefined,
        from:      state.from   || undefined,
        to:        state.to     || undefined,
        page:      state.page,
        page_size: PAGE_SIZE,
      });
      logList.innerHTML = '';
      for (const entry of (res.entries ?? [])) {
        logList.appendChild(_buildLogRow(entry));
      }
      if (!res.entries?.length) {
        logList.innerHTML = `<div class="px-4 py-8 text-center text-text-muted text-sm">${t('logs.app.empty')}</div>`;
      }
      destroyPagin?.();
      const totalPages = Math.ceil((res.total ?? 0) / PAGE_SIZE);
      const hasNext = state.page < totalPages;
      if (state.page > 1 || hasNext) {
        const { destroy } = renderPagination(paginEl, {
          page: state.page,
          hasNext,
          total: totalPages || undefined,
          onPageChange: (p) => { state.page = p; _fetch(); },
        });
        destroyPagin = destroy;
      } else {
        paginEl.innerHTML = '';
        destroyPagin = null;
      }
    } catch (err) {
      logList.innerHTML = '';
      logList.appendChild(createErrorState({ message: err?.message ?? t('logs.error.load_failed') }));
    } finally {
      finishLoading();
    }
  }

  _fetch();

  return () => {
    abort?.abort();
    destroyPagin?.();
    doSearch.cancel();
    levelChips.destroy();
    render(null, datesEl);
  };
}

/**
 * Builds a single log row element.
 * @param {{ timestamp: string, level: string, target: string, message: string }} entry
 * @returns {HTMLElement}
 */
function _buildLogRow(entry) {
  const row = document.createElement('div');
  const lvl = (entry.level ?? 'INFO').toUpperCase();
  const levelCls = LEVEL_CLASSES[lvl] ?? 'text-text-muted';
  // Stacked on a phone, one line from md up. The single-line row is ~1900px
  // wide, and scrolling sideways through a log you read downwards is no way to
  // reach a message.
  row.className = 'flex flex-col gap-0.5 px-4 py-1 border-b border-border/30'
    + ' md:flex-row md:items-baseline md:gap-2 md:py-0.5 md:w-max md:min-w-full hover:bg-surface-2';
  row.title = entry.target ?? '';
  row.innerHTML = `
    <span class="flex items-baseline gap-2 shrink-0">
      <span class="text-text-muted/60 whitespace-nowrap">${escapeHtml(entry.timestamp ?? '')}</span>
      <span class="w-11 text-right ${levelCls}">${escapeHtml(lvl)}</span>
      <span class="hidden md:inline text-text-muted truncate max-w-[14rem]">${escapeHtml(entry.target ?? '')}</span>
    </span>
    <span class="min-w-0 break-words md:shrink-0 md:whitespace-nowrap">${escapeHtml(entry.message ?? '')}</span>
  `;
  return row;
}


/** @param {HTMLElement} container @returns {() => void} */
function _mountAuditTab(container) {
  let state = { search: '', from: '', to: '', page: 1 };
  /** @type {AbortController | null} */ let abort = null;
  /** @type {(() => void) | null} */   let destroyPagin = null;

  const root = document.createElement('div');
  root.className = 'flex flex-col flex-1 overflow-hidden';
  container.appendChild(root);

  const filterBar = document.createElement('div');
  filterBar.className = 'flex flex-wrap items-center gap-2 px-4 md:px-6 py-3 border-b border-border shrink-0';
  filterBar.innerHTML = `
    <input type="search" id="audit-search" placeholder="${escapeHtml(t('logs.audit.search_placeholder'))}" class="input input-sm flex-1 min-w-40" />
    <div id="audit-dates" class="flex items-center gap-2 shrink-0"></div>
  `;
  root.appendChild(filterBar);

  const datesEl = /** @type {HTMLElement} */ (filterBar.querySelector('#audit-dates'));
  const _renderDates = () => {
    render(html`
      <${DateRange} from=${state.from} to=${state.to}
        onChange=${(/** @type {{ from: string, to: string }} */ r) => {
          state.from = r.from; state.to = r.to; state.page = 1; _renderDates(); _fetch();
        }} />
    `, datesEl);
  };
  _renderDates();

  const tableWrap = document.createElement('div');
  tableWrap.className = 'flex-1 overflow-y-auto';
  root.appendChild(tableWrap);

  const tbody = document.createElement('div');
  tbody.id = 'audit-list';
  tableWrap.appendChild(tbody);

  const paginEl = document.createElement('div');
  paginEl.className = 'px-4 md:px-6 py-3 shrink-0';
  root.appendChild(paginEl);

  const searchEl = /** @type {HTMLInputElement} */ (filterBar.querySelector('#audit-search'));

  const doSearch = debounce(() => { state.page = 1; _fetch(); }, 300);

  searchEl.addEventListener('input', () => { state.search = searchEl.value; doSearch(); });

  async function _fetch() {
    abort?.abort();
    abort = new AbortController();
    startLoading();
    try {
      /** @type {{ entries: any[], has_next: boolean, total_pages?: number }} */
      const res = await api.getAdminAuditLog({
        search:    state.search || undefined,
        from:      state.from   || undefined,
        to:        state.to     || undefined,
        page:      state.page,
        page_size: PAGE_SIZE,
      });
      tbody.innerHTML = '';

      if (!res.entries?.length) {
        tbody.innerHTML = `<div class="px-4 py-8 text-center text-text-muted text-sm">${t('logs.audit.empty')}</div>`;
      } else {
        for (const e of res.entries) {
          tbody.appendChild(_buildAuditRow(e));
        }
      }

      destroyPagin?.();
      const hasNext = res.has_next ?? false;
      const totalPages = res.total_pages ?? undefined;
      if (state.page > 1 || hasNext) {
        const { destroy } = renderPagination(paginEl, {
          page: state.page,
          hasNext,
          total: totalPages,
          onPageChange: (p) => { state.page = p; _fetch(); },
        });
        destroyPagin = destroy;
      } else {
        paginEl.innerHTML = '';
        destroyPagin = null;
      }
    } catch (err) {
      tbody.innerHTML = '';
      tbody.appendChild(createErrorState({ message: err?.message ?? t('logs.error.audit_load_failed') }));
    } finally {
      finishLoading();
    }
  }

  _fetch();

  return () => {
    abort?.abort();
    destroyPagin?.();
    doSearch.cancel();
    render(null, datesEl);
  };
}

/**
 * @param {{ id: number, created_at: string, username?: string, action: string, target?: string, details?: string }} entry
 * @returns {HTMLElement}
 */
function _buildAuditRow(entry) {
  const row = document.createElement('div');
  row.className = 'flex items-start gap-3 px-4 py-2 border-b border-border/30 hover:bg-surface-2 text-sm';
  row.innerHTML = `
    <span class="shrink-0 text-text-muted text-xs whitespace-nowrap mt-0.5">${escapeHtml(entry.created_at ?? '')}</span>
    <span class="shrink-0 font-medium w-24 truncate" title="${escapeHtml(entry.username ?? '')}">${escapeHtml(entry.username ?? '—')}</span>
    <span class="shrink-0 font-mono text-xs text-text w-40 truncate mt-0.5" title="${escapeHtml(entry.action ?? '')}">${escapeHtml(entry.action ?? '')}</span>
    <span class="shrink-0 text-text-muted w-32 truncate" title="${escapeHtml(entry.target ?? '')}">${escapeHtml(entry.target ?? '')}</span>
    ${entry.details ? `<details class="flex-1 min-w-0"><summary class="cursor-pointer text-text-muted">${t('logs.audit.details')}</summary><pre class="text-xs mt-1 whitespace-pre-wrap break-all">${escapeHtml(entry.details)}</pre></details>` : ''}
  `;
  return row;
}


async function _handlePurge() {
  const ok = await showConfirm(t('logs.purge.confirm'), {
    title: t('logs.purge'),
    confirmLabel: t('logs.purge'),
    danger: true,
  });
  if (!ok) return;
  try {
    await api.purgeAdminLogs();
    showToast(t('logs.purge.done'), { type: 'success' });
    window.location.reload();
  } catch (e) {
    showApiError(e);
  }
}

function _handleDownload() {
  const qs = new URLSearchParams();
  if (_activeTab === 'app') {
    qs.set('format', 'text');
    window.location.href = `/rest/admin/logs/download?${qs}`;
  } else {
    qs.set('format', 'csv');
    window.location.href = `/rest/admin/audit-log/download?${qs}`;
  }
}


function _stopSse() {
  if (_sse) {
    _sse.close();
    _sse = null;
  }
}
