// @ts-check
// Source details page — browse / search manga within a single source.
// Desktop: sidebar + tabbed panel (Browse / Settings). Mobile: back button + panel.

import { h, render } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import { replaceState } from '../url-params.js';
import * as api from '../api.js';
import { hasPermission } from '../session.js';
import { subscribe as subscribeCache, updateState as updateCacheState } from '../cache.js';
import { subscribe as subscribeUiState } from '../ui-state.js';
import { navigate, scrollPageTop } from '../router.js';
import { debounce, hasNextPage, fmtCompactDate, errorCountAriaLabel } from '../utils.js';
import { getTileSize, setTileSize } from '../tile-size.js';
import { skeletonSettingsCards, skeletonKeyValueRows } from '../components/skeletons.js';
import { renderPagination } from '../components/pagination.js';
import { createMangaCard } from '../components/manga-card.js';
import { fetchPagedGrid } from '../components/paged-grid.js';
import { Modal, mountIntoModalRoot, showConfirm } from '../components/modal.js';
import { showApiError } from '../components/toast.js';
import { SourcesSidebar, AddSourceModal } from '../components/sources-sidebar.js';
import { PreferenceRow, PreferenceDetailView } from '../components/preference-row.js';
import { TileSizeSelect } from '../components/tile-size-select.js';
import { FALLBACK_PAGE_SIZE, MAX_PAGE_SIZE, gridCapacity, gridFitEnabled, observeCapacity } from '../grid-columns.js';
import { Icon } from '../components/icon.js';
import { setPageHeader, clearPageHeader } from '../components/app-header.js';
import { createSourcesHeaderActions } from '../components/sources-header.js';
import { createErrorState } from '../components/error-state.js';
import { createEmptyState } from '../components/empty-state.js';
import { mountFilterModal } from '../components/filter-panel.js';
import { renderTabs } from '../components/tabs.js';
import { iconSearch, iconChevronDown, iconWarning } from '../icons.js';
import { t } from '../i18n.js';
import { prefersInfiniteScroll } from '../pagination-mode.js';
const html = htm.bind(h);


let _sourceId = 0;
let _page = 1;
let _pageSize = 0;
/** @type {import('../tile-size.js').TileSize} */
let _tileSize = 'md';
/** @type {Array<() => void>} */ let _stopColumnWatches = [];

/**
 * @param {HTMLElement | null} gridEl A grid container fetchPagedGrid renders into.
 * @returns {HTMLElement | null}
 */
function _mangaGridIn(gridEl) {
  return /** @type {HTMLElement|null} */ (gridEl?.querySelector('.manga-grid') ?? null);
}

/**
 * The page holds exactly what fits. Where the shell does not pin the page to
 * the viewport the question has no answer, so a fixed batch is used instead.
 *
 * @param {HTMLElement | null} gridEl
 */
function _syncPageSize(gridEl) {
  const next = gridFitEnabled() && !_isInfinite()
    ? gridCapacity(_mangaGridIn(gridEl), gridEl)
    : 0;
  if (next > 0) _pageSize = Math.min(next, MAX_PAGE_SIZE);
  else if (_pageSize < 1) _pageSize = FALLBACK_PAGE_SIZE;
}

function _isInfinite() {
  return prefersInfiniteScroll('kani_source_pagination');
}
let _query = '';
let _sourceName = '';
let _sourceEnabled = true;
let _activeTab = 'popular';
/** @type {AbortController | null} */
let _abort = null;
/** @type {(() => void) | null} */
let _destroyPaginationSearch = null;
/** @type {(() => void) | null} */
let _destroyPaginationPopular = null;
/** @type {(() => void) | null} */
let _unsubSourcesInvalidation = null;
/** @type {(() => void) | null} */
let _unsubPrefVersion = null;
/** @type {IntersectionObserver | null} */
let _sentinelObserver = null;
/** @type {HTMLElement | null} */
let _settingsMountEl = null;
/** @type {HTMLElement | null} */
let _prefsMountEl = null;
/** @type {HTMLElement | null} */
let _asideEl = null;
/** @type {HTMLButtonElement | null} */
let _addSourceBtn = null;
/** @type {HTMLElement[] | null} */
let _headerActions = null;
/** @type {HTMLElement | null} */
let _popularPanelEl = null;
/** @type {HTMLElement | null} */
let _searchPanelEl = null;
/** @type {((activeId: string, tabs?: any[]) => void) | null} */
let _tabsUpdateFn = null;
let _settingsMounted = false;
let _prefsMounted = false;
let _hasPrefs = false;
/** @type {any[]} */
let _filterDefs = [];
/** @type {Record<string, string>} */
let _filters = {};
/** @type {(() => void) | null} */
let _filterModalDestroy = null;
/** @type {Record<string, string>} */
let _pendingFilterParams = {};


/**
 * Normalize a filter state value from either adjacently-tagged ({kind, data}) or
 * externally-tagged ({Selection: data}) serde format to the adjacently-tagged form
 * expected by the filter panel: { kind: 'Selection'|'Checkbox'|'TextInput', data: ... }.
 * @param {any} raw
 * @returns {any}
 */
/** Builds the "source disabled" empty-state element shown in place of browse content. */
function _disabledStateEl() {
  return createEmptyState({
    icon: iconWarning,
    title: t('source.disabled.title'),
    subtitle: t('source.disabled.hint'),
  });
}

/** Shows a "disabled" placeholder in a panel, replacing its content. */
function _showDisabledPanel(panelEl) {
  const inner = panelEl.querySelector('.flex.flex-col');
  if (!inner) return;
  inner.innerHTML = '';
  inner.appendChild(_disabledStateEl());
}

function _normalizeFilterState(raw) {
  if (!raw || typeof raw !== 'object') return raw;
  if (typeof raw.kind === 'string') return raw;
  const entries = Object.entries(raw);
  if (entries.length === 1) return { kind: entries[0][0], data: entries[0][1] };
  return raw;
}

/**
 * Build an initial filter state from filter defaults.
 * @param {any[]} filterDefs
 * @returns {Record<string, any>}
 */
function _buildDefaultFilters(filterDefs) {
  /** @type {Record<string, any>} */
  const defaults = {};
  for (const f of filterDefs) {
    if (f.default_value) defaults[f.id] = _normalizeFilterState(f.default_value);
  }
  return defaults;
}


/**
 * Renders the source's full preference list. Complex preference types open in
 * a modal, giving the same flow on desktop and mobile.
 * @param {{ sourceId: number }} props
 */
function SourcePreferencesPanel({ sourceId }) {
  const [schema, setSchema] = useState(/** @type {any[]} */ ([]));
  const [liveValues, setLiveValues] = useState(/** @type {Record<string,any>} */ ({}));
  const [loading, setLoading] = useState(true);
  const [activeDescriptor, setActiveDescriptor] = useState(/** @type {any} */ (null));
  const [collapsedGroups, setCollapsedGroups] = useState(/** @type {Set<string>} */ (new Set()));

  useEffect(() => {
    Promise.all([api.getPreferenceSchema(sourceId), api.getPreferences(sourceId)])
      .then(([schemaRes, prefsRes]) => {
        setSchema(Array.isArray(schemaRes) ? schemaRes : []);
        setLiveValues(Array.isArray(prefsRes)
          ? Object.fromEntries(prefsRes.map(pr => [pr.key, pr.value]))
          : {});
      })
      .catch(showApiError)
      .finally(() => setLoading(false));
  }, [sourceId]);

  /** @type {Map<string, any[]>} */
  const groups = new Map();
  for (const d of schema) {
    const g = d.group ?? '';
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g).push(d);
  }

  const _toggleGroup = (/** @type {string} */ group) => setCollapsedGroups(prev => {
    const next = new Set(prev);
    next.has(group) ? next.delete(group) : next.add(group);
    return next;
  });

  if (loading) {
    return html`<div class="max-w-narrow py-1" dangerouslySetInnerHTML=${{ __html: skeletonSettingsCards(4) }} />`;
  }
  if (schema.length === 0) {
    return html`<p class="text-sm text-text-muted py-4">${t('source.prefs.empty')}</p>`;
  }

  return html`
    <div class="bg-surface border border-border rounded-xl px-4 md:px-6 py-1 max-w-narrow">
      <div class="flex flex-col">
        ${[...groups.entries()].map(([group, descriptors]) => {
          const isCollapsed = collapsedGroups.has(group);
          return html`
            <div key=${group} class="flex flex-col">
              ${group && html`
                <button
                  class="flex items-center justify-between gap-2 py-2 w-full text-left text-xs font-semibold uppercase tracking-wider text-text-muted hover:text-text transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded"
                  onClick=${() => _toggleGroup(group)}
                  aria-expanded=${!isCollapsed}
                >
                  ${group}
                  <span class=${'icon-xs transition-transform ' + (isCollapsed ? '' : 'rotate-180')}>
                    <${Icon} svg=${iconChevronDown} />
                  </span>
                </button>
              `}
              ${!isCollapsed && descriptors.map(d => html`
                <${PreferenceRow}
                  key=${d.key}
                  sourceId=${sourceId}
                  descriptor=${d}
                  currentValue=${liveValues[d.key]}
                  liveValues=${liveValues}
                  onValueChange=${(key, val) => setLiveValues(prev => ({ ...prev, [key]: val }))}
                  onOpenDetail=${(desc) => setActiveDescriptor(desc)}
                />
              `)}
            </div>
          `;
        })}
      </div>
      <${Modal}
        open=${activeDescriptor != null}
        onClose=${() => setActiveDescriptor(null)}
        title=${activeDescriptor ? (activeDescriptor.label ?? activeDescriptor.title ?? '') : ''}
      >
        ${activeDescriptor && html`
          <${PreferenceDetailView}
            sourceId=${sourceId}
            descriptor=${activeDescriptor}
            currentValue=${liveValues[activeDescriptor.key]}
            liveValues=${liveValues}
            onValueChange=${(key, val) => setLiveValues(prev => ({ ...prev, [key]: val }))}
            onBack=${() => setActiveDescriptor(null)}
            showHeader=${false}
          />
        `}
      </${Modal}>
    </div>
  `;
}


/**
 * Full-page settings panel for a single source, designed for the Settings tab.
 * On desktop, preferences are rendered inline. On mobile, they open in a modal.
 * @param {{
 *   source: any,
 *   activeIds: Set<number>,
 *   onDeleted: () => void,
 *   onEnabledChange?: (enabled: boolean) => void,
 * }} props
 */
function SourceSettingsPage({ source, activeIds, onDeleted, onEnabledChange }) {
  const sid = source.id;
  const isActive = activeIds.has(sid);

  const [enabled, setEnabled] = useState(source.enabled ?? false);
  const [confirming, setConfirming] = useState(false);

  const [dlConcurrency, setDlConcurrency] = useState(
    /** @type {string} */ (source.download_concurrency != null ? String(source.download_concurrency) : '')
  );
  const [dlConcurrencySaving, setDlConcurrencySaving] = useState(false);

  const canGrantLocal = hasPermission('admin:manage');
  const [localHosts, setLocalHosts] = useState('');
  const [localHostsSaving, setLocalHostsSaving] = useState(false);
  const [localHostsError, setLocalHostsError] = useState(/** @type {string|null} */ (null));

  const [health, setHealth] = useState(/** @type {any|null} */ (null));
  const [healthLoading, setHealthLoading] = useState(true);
  const [reloading, setReloading] = useState(false);
  const [reloadMsg, setReloadMsg] = useState(/** @type {'ok'|'err'|null} */ (null));
  const [capabilities, setCapabilities] = useState(/** @type {{streaming_chapters:boolean}|null} */ (null));

  useEffect(() => {
    api.getSourcesHealth().then(rows => {
      if (Array.isArray(rows)) {
        setHealth(rows.find(r => r.source_id === sid) ?? null);
      }
    }).catch(() => {}).finally(() => setHealthLoading(false));
    api.getSourceCapabilities(sid).then(setCapabilities).catch(() => {});
    if (canGrantLocal) {
      api.getSourceLocalHosts(sid)
        .then(r => setLocalHosts((r.hosts ?? []).join('\n')))
        .catch(() => {});
    }
  }, [sid]);

  async function toggleEnabled(val) {
    if (val && source.unrestricted_http && !confirming) {
      setConfirming(true);
      return;
    }
    setConfirming(false);
    try {
      await api.toggleSourceEnabled(sid, val);
      setEnabled(val);
      onEnabledChange?.(val);
    } catch { }
  }

  async function handleSaveDlConcurrency() {
    const val = dlConcurrency.trim() === '' ? null : Number(dlConcurrency);
    setDlConcurrencySaving(true);
    try {
      await api.setSourceDownloadConcurrency(sid, val);
    } finally {
      setDlConcurrencySaving(false);
    }
  }

  async function handleSaveLocalHosts() {
    const hosts = localHosts.split('\n').map(h => h.trim()).filter(Boolean);
    setLocalHostsSaving(true);
    setLocalHostsError(null);
    try {
      const saved = await api.setSourceLocalHosts(sid, hosts);
      setLocalHosts((saved.hosts ?? []).join('\n'));
    } catch (e) {
      setLocalHostsError(/** @type {any} */ (e)?.message ?? t('common.error.failed'));
    } finally {
      setLocalHostsSaving(false);
    }
  }

  async function handleReload() {
    setReloading(true);
    setReloadMsg(null);
    try {
      await api.reloadSource(sid);
      setReloadMsg('ok');
    } catch {
      setReloadMsg('err');
    } finally {
      setReloading(false);
      setTimeout(() => setReloadMsg(null), 3000);
    }
  }

  async function handleDelete() {
    const confirmed = await showConfirm(t('source.delete.message'), {
      title: t('source.delete.title'),
      confirmLabel: t('common.delete'),
      danger: true,
    });
    if (!confirmed) return;
    try {
      await api.deleteSource(sid);
      onDeleted();
    } catch (e) {
      showApiError(e);
    }
  }



  /** @param {string} title @param {string} subtitle */
  const mkSectionHdr = (title, subtitle) => html`
    <div class="flex flex-col gap-0.5 pb-2 border-b border-border-subtle">
      <h2 class="text-sm font-semibold text-text">${title}</h2>
      <p class="text-xs text-text-muted">${subtitle}</p>
    </div>
  `;

  let sourceLanguages = [];
  if (source.languages) {
    try {
      const parsed = JSON.parse(source.languages);
      if (Array.isArray(parsed)) sourceLanguages = parsed;
    } catch { }
  }

  return html`
    <div class="flex flex-col gap-8">

      <!-- 0. About -->
      <div class="flex flex-col gap-3">
        <div class="bg-surface border border-border rounded-xl px-4 md:px-6 py-4 flex items-start gap-4">
          ${source.icon
            ? html`<img src=${`data:image/png;base64,${source.icon}`} alt="" class="w-12 h-12 rounded-lg shrink-0 object-contain bg-surface-2" />`
            : html`<span class="w-12 h-12 rounded-lg shrink-0 flex items-center justify-center text-lg font-medium" style="background:var(--color-surface-3);color:var(--color-text-muted)">${(source.name ?? '?')[0]?.toUpperCase()}</span>`
          }
          <div class="flex flex-col gap-1.5 min-w-0">
            <p class="text-sm font-medium text-text truncate">${source.name}</p>
            <p class="text-xs text-text-muted">${source.description || t('source.about.no_description')}</p>
            ${sourceLanguages.length > 0 && html`
              <div class="flex items-center gap-1.5 flex-wrap mt-0.5">
                <span class="text-2xs text-text-faint">${t('source.about.languages')}:</span>
                ${sourceLanguages.map(lang => html`<span key=${lang} class="text-2xs px-1.5 py-0.5 rounded bg-surface-2 text-text-muted">${lang}</span>`)}
              </div>
            `}
            ${source.backend && html`
              <div class="flex items-center gap-1.5 mt-0.5">
                <span
                  class="text-2xs px-1.5 py-0.5 rounded bg-surface-2 text-text-muted cursor-help"
                  data-tooltip=${t('source.backend.tooltip')}
                >
                  ${source.backend === 'yaml' ? t('source.backend.yaml') : t('source.backend.wasm')}
                </span>
              </div>
            `}
          </div>
        </div>
      </div>

      <!-- 1. General -->
      <div class="flex flex-col gap-3">
        ${mkSectionHdr(t('source.settings.general'), t('source.settings.general.desc'))}
        <div class=${'bg-surface border rounded-xl px-4 md:px-6 py-1 ' + (source.unrestricted_http ? 'border-warn/50' : 'border-border')}>

          <div class="py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0">
            <div class="flex items-center justify-between gap-4">
              <div>
                <p class="text-sm font-medium text-text">${t('source.settings.runtime_status')}</p>
                <p class="text-xs text-text-muted mt-0.5">${t('source.settings.runtime_status.desc')}</p>
              </div>
              <span class=${'shrink-0 inline-flex items-center px-2 py-0.5 text-xs font-medium rounded-full ' + (isActive ? 'bg-success/20 text-success' : 'bg-surface-2 text-text-muted')}>
                ${isActive ? t('source.settings.loaded') : t('source.settings.unloaded')}
              </span>
            </div>
          </div>

          <div class="py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0">
            <div class="flex items-center justify-between gap-4">
              <div>
                <p class="text-sm font-medium text-text">${t('source.settings.enabled')}</p>
                <p class="text-xs text-text-muted mt-0.5">${t('source.settings.enabled.desc')}</p>
              </div>
              <label class="kani-toggle shrink-0 cursor-pointer">
                <input
                  type="checkbox"
                  class="kani-toggle__input"
                  checked=${enabled}
                  aria-label=${enabled ? t('source.settings.disable') : t('source.settings.enable')}
                  onChange=${(e) => toggleEnabled(/** @type {HTMLInputElement} */ (e.target).checked)}
                />
                <span class="kani-toggle__track"></span>
              </label>
            </div>
            ${confirming && html`
              <div class="mt-3 rounded-lg bg-warn/10 border border-warn/30 p-3 flex flex-col gap-2">
                <p class="text-sm text-warn flex items-center gap-1.5">
                  <${Icon} svg=${iconWarning} />
                  ${t('source.settings.unrestricted_warning')}
                </p>
                <div class="flex items-center gap-2 justify-end">
                  <button class="btn-ghost btn-sm" onClick=${() => setConfirming(false)}>${t('common.cancel')}</button>
                  <button class="btn-danger btn-sm" onClick=${() => toggleEnabled(true)}>${t('source.settings.enable_anyway')}</button>
                </div>
              </div>
            `}
          </div>

          ${source.unrestricted_http && html`
            <div class="py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0">
              <div class="flex items-center gap-1.5 text-warn icon-sm">
                <${Icon} svg=${iconWarning} />
                <p class="text-sm font-medium">${t('source.settings.unrestricted_http')}</p>
              </div>
              <p class="text-xs text-text-muted mt-0.5">${t('source.settings.unrestricted_http.desc')}</p>
            </div>
          `}

          <div class="py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0">
            <div class="flex items-center justify-between gap-4">
              <p class="text-sm font-medium text-text">${t('source.settings.version')}</p>
              <span class="flex items-center gap-2 shrink-0">
                ${source.version?.includes('+debug') && html`
                  <span class="text-2xs px-1.5 py-0.5 rounded bg-warn/20 text-warn font-medium leading-none" title="Built with debug info — readable WASM backtraces, larger binary">DEBUG</span>
                `}
                <span class="text-sm text-text-muted">v${(source.version ?? '?').replace('+debug', '')}</span>
              </span>
            </div>
          </div>

          ${capabilities?.streaming_chapters && html`
            <div class="py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0">
              <div class="flex items-center gap-1.5 text-text icon-sm">
                <p class="text-sm font-medium">${t('source.settings.streaming')}</p>
              </div>
              <p class="text-xs text-text-muted mt-0.5">${t('source.settings.streaming.desc')}</p>
            </div>
          `}

          ${hasPermission('source:install') && html`
            <div class="py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0">
              <div class="flex items-center justify-between gap-4">
                <div>
                  <p class="text-sm font-medium text-text">${t('source.settings.reload')}</p>
                  <p class="text-xs text-text-muted mt-0.5">${t('source.settings.reload.desc')}</p>
                </div>
                <div class="flex items-center gap-2 shrink-0">
                  ${reloadMsg === 'ok' && html`<span class="text-xs text-success">${t('source.settings.reloaded')}</span>`}
                  ${reloadMsg === 'err' && html`<span class="text-xs text-danger">${t('common.error.failed')}</span>`}
                  <button class="btn-ghost btn-sm" disabled=${reloading} onClick=${handleReload}>
                    ${reloading ? t('source.settings.reloading') : t('source.settings.reload.action')}
                  </button>
                </div>
              </div>
            </div>

            <div class="py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0">
              <div class="flex items-center justify-between gap-4">
                <div>
                  <p class="text-sm font-medium text-text">${t('source.concurrency.title')}</p>
                  <p class="text-xs text-text-muted mt-0.5">${t('source.concurrency.desc')}</p>
                </div>
                <div class="flex items-center gap-2 shrink-0">
                  <input
                    type="number"
                    class="input w-20 text-sm"
                    min="1"
                    max="16"
                    placeholder=${t('source.concurrency.placeholder')}
                    value=${dlConcurrency}
                    disabled=${dlConcurrencySaving}
                    onInput=${(/** @type {any} */ e) => setDlConcurrency(e.target.value)}
                  />
                  <button
                    class="btn-ghost btn-sm"
                    disabled=${dlConcurrencySaving}
                    onClick=${handleSaveDlConcurrency}
                  >${dlConcurrencySaving ? t('common.saving') : t('common.save')}</button>
                </div>
              </div>
            </div>
          `}

          ${canGrantLocal && html`
            <div class="py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0">
              <div class="flex flex-col gap-2">
                <div>
                  <p class="text-sm font-medium text-text">${t('source.local_hosts.title')}</p>
                  <p class="text-xs text-text-muted mt-0.5">${t('source.local_hosts.desc')}</p>
                </div>
                <textarea
                  class="input text-sm font-mono"
                  rows="3"
                  aria-label=${t('source.local_hosts.title')}
                  placeholder=${t('source.local_hosts.placeholder')}
                  value=${localHosts}
                  disabled=${localHostsSaving}
                  onInput=${(/** @type {any} */ e) => setLocalHosts(e.target.value)}
                ></textarea>
                ${localHostsError && html`<p class="text-xs text-danger">${localHostsError}</p>`}
                <div class="flex justify-end">
                  <button
                    class="btn-secondary btn-sm"
                    disabled=${localHostsSaving}
                    onClick=${handleSaveLocalHosts}
                  >${localHostsSaving ? t('common.saving') : t('common.save')}</button>
                </div>
              </div>
            </div>
          `}

        </div>
      </div>

      <!-- 3. Health -->
      <div class="flex flex-col gap-3">
        ${mkSectionHdr(t('source.settings.health'), t('source.settings.health.desc'))}
        <div class="bg-surface border border-border rounded-xl px-4 md:px-6 py-1">
          ${healthLoading
            ? html`<div dangerouslySetInnerHTML=${{ __html: skeletonKeyValueRows(4) }} />`
            : health == null
              ? html`<p class="text-sm text-text-muted py-3">${t('source.settings.health.empty')}</p>`
              : html`
                <div class="flex flex-col divide-y divide-border-subtle">
                  <div class="flex items-center justify-between gap-4 py-3">
                    <p class="text-sm text-text">${t('source.health.last_success')}</p>
                    <span class="text-sm text-text-muted">${health.last_success_at ? fmtCompactDate(health.last_success_at) : '—'}</span>
                  </div>
                  <div class="flex items-center justify-between gap-4 py-3">
                    <p class="text-sm text-text">${t('source.health.last_error')}</p>
                    <span class="text-sm text-text-muted">${health.last_error_at ? fmtCompactDate(health.last_error_at) : '—'}</span>
                  </div>
                  <div class="flex items-center justify-between gap-4 py-3">
                    <p class="text-sm text-text">${t('source.health.consecutive_errors')}</p>
                    ${(health.consecutive_error_count ?? 0) >= 3
                      ? html`<span class="text-xs font-semibold px-1.5 py-0.5 rounded bg-danger/20 text-danger" aria-label=${errorCountAriaLabel(health.consecutive_error_count ?? 0)}>${health.consecutive_error_count}</span>`
                      : (health.consecutive_error_count ?? 0) > 0
                        ? html`<span class="text-xs font-semibold px-1.5 py-0.5 rounded bg-warn/20 text-warn" aria-label=${errorCountAriaLabel(health.consecutive_error_count ?? 0)}>${health.consecutive_error_count}</span>`
                        : html`<span class="text-sm text-success" aria-label="${t('source.health.zero_errors')}">0</span>`
                    }
                  </div>
                  <div class="flex items-center justify-between gap-4 py-3">
                    <p class="text-sm text-text">${t('source.health.avg_response')}</p>
                    <span class="text-sm text-text-muted">${health.avg_response_ms != null ? Math.round(health.avg_response_ms) + ' ms' : '—'}</span>
                  </div>
                </div>
              `
          }
        </div>
      </div>

      <!-- 4. Danger Zone -->
      <div class="flex flex-col gap-3">
        ${mkSectionHdr(t('source.settings.danger'), t('source.settings.danger.desc'))}
        <div class="bg-surface border border-border rounded-xl px-4 md:px-6 py-1">
          <div class="py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0">
            <div class="flex items-center justify-between gap-4">
              <div>
                <p class="text-sm font-medium text-text">${t('source.delete.action')}</p>
                <p class="text-xs text-text-muted mt-0.5">${t('source.delete.desc')}</p>
              </div>
              <button class="btn-danger btn-sm shrink-0" onClick=${handleDelete}>${t('common.delete')}</button>
            </div>
          </div>
        </div>
      </div>

    </div>
  `;
}



function _updateUrl() {
  /** @type {Record<string, string|number|null>} */
  const params = {
    tab: _activeTab,
    page: _page > 1 ? _page : null,
    q: _query || null,
  };
  for (const [filterId, state] of Object.entries(_filters)) {
    params['f_' + filterId] = JSON.stringify(state);
  }
  replaceState(params);
}


function _updateBreadcrumb() {
  const crumbs = [{ label: t('source.nav.sources'), href: '/sources' }];
  if (_query) {
    crumbs.push({ label: _sourceName || t('source.nav.source'), href: `/source/${_sourceId}` });
    crumbs.push({ label: t('source.nav.search', { query: _query }) });
  } else {
    crumbs.push({ label: _sourceName || t('source.nav.source') });
  }
  setPageHeader({ crumbs, actions: _headerActions });
}


/**
 * @param {HTMLElement} container
 * @param {{ id: string }} params
 */
export async function init(container, { id }) {
  _sourceId = Number(id);
  _page = 1;
  const _urlParams = new URLSearchParams(location.search);
  _query = _urlParams.get('q') ?? '';
  _tileSize = getTileSize();
  _sourceName = '';
  _sourceEnabled = true;
  _settingsMountEl = null;
  _prefsMountEl = null;
  _popularPanelEl = null;
  _searchPanelEl = null;
  _tabsUpdateFn = null;
  _settingsMounted = false;
  _prefsMounted = false;
  _hasPrefs = false;
  _filterDefs = [];
  _filterModalDestroy?.();
  _filterModalDestroy = null;

  const _preFilterName  = _urlParams.get('filter_name');
  const _preFilterValue = _urlParams.get('filter_value');
  _filters = (_preFilterName && _preFilterValue)
    ? { [_preFilterName]: _preFilterValue }
    : {};

  // Restore tab from URL (takes precedence over query-based heuristic)
  const _tabParam = _urlParams.get('tab');
  if (_tabParam === 'library') {
    navigate(`/?source_id=${_sourceId}`, { replace: true });
    return;
  }
  if (_tabParam && ['popular', 'search', 'prefs', 'settings'].includes(_tabParam)) {
    _activeTab = _tabParam;
  } else {
    _activeTab = (_query || _preFilterName) ? 'search' : 'popular';
  }

  // Restore page numbers from URL
  const _pageParam = _urlParams.get('page');
  if (_pageParam) _page = Math.max(1, parseInt(_pageParam, 10) || 1);

  // Stash f_* filter params for async restoration after filter defs load
  _pendingFilterParams = {};
  for (const [key, value] of _urlParams.entries()) {
    if (key.startsWith('f_')) _pendingFilterParams[key.slice(2)] = value;
  }

  if (!hasPermission('source:browse')) {
    container.innerHTML = '';
    container.appendChild(createErrorState({ message: t('source.error.no_permission') }));
    return;
  }

  const header = createSourcesHeaderActions({
    canInstall: hasPermission('source:install'),
    onTab: (tab) => navigate(tab === 'repos' ? '/sources?tab=repos' : '/sources'),
  });
  header.setActive('extensions');
  _headerActions = header.actions;
  _addSourceBtn = header.addSourceBtn;

  container.classList.add('page-fixed');
  container.innerHTML = `
    <div class="flex page-body-host">

      <!-- Sidebar (lg+) — SourcesSidebar mounts here -->
      <aside
        class="hidden lg:flex flex-col w-72 shrink-0 border-r border-border-subtle sticky overflow-y-auto"
        style="top:0;height:calc(100dvh - var(--header-h));"
        aria-label="${t('source.nav.sources')}"
      ></aside>

      <!-- Main panel -->
      <div class="flex-1 min-w-0 flex flex-col page-col">

        <div class="flex-1 max-w-page mx-auto w-full px-4 md:px-6 py-4 md:pt-6 md:pb-0 flex flex-col gap-4 page-col">
          <!-- Tab bar -->
          <div class="js-tabs"></div>

          <!-- Popular panel -->
          <div class="js-panel page-panel" data-panel="popular">
            <div class="flex flex-col gap-4 page-col page-fill">
              <div class="flex items-end justify-end gap-2">
                <div class="js-popular-tile-size-mount w-28"></div>
              </div>
              <div class="js-popular-grid page-body" aria-live="polite" aria-busy="false"><div class="manga-grid"></div></div>
              <div class="js-popular-pagination"></div>
            </div>
          </div>

          <!-- Search panel (hidden initially) -->
          <div class="js-panel page-panel hidden" data-panel="search">
            <div class="flex flex-col gap-4 page-col page-fill">
              <div class="flex items-center gap-3 flex-wrap">
                <div class="relative flex-1 min-w-48 max-w-sm">
                  <span class="absolute left-3 top-1/2 -translate-y-1/2 text-text-muted pointer-events-none icon-sm" aria-hidden="true">${iconSearch}</span>
                  <input
                    type="search"
                    class="input w-full pl-9 js-search"
                    placeholder="${t('source.search.placeholder')}"
                    aria-label="${t('source.search.aria')}"
                  />
                </div>
                <div class="js-tile-size-mount w-28 shrink-0"></div>
                <button type="button" class="js-filter-btn input flex items-center justify-center gap-1.5 w-full sm:w-auto shrink-0" aria-label="${t('source.filters.open')}" style="display:none">${t('library.filters')}</button>
              </div>
              <div class="js-search-grid page-body" aria-live="polite" aria-busy="false"><div class="manga-grid"></div></div>
              <div class="js-search-pagination"></div>
            </div>
          </div>


          <!-- Preferences panel (hidden initially) -->
          <div class="js-panel page-panel hidden" data-panel="prefs">
            <div class="js-prefs-mount flex flex-col gap-4 page-body"></div>
          </div>

          <!-- Settings panel (hidden initially) -->
          <div class="js-panel page-panel hidden" data-panel="settings">
            <div class="js-settings-mount flex flex-col gap-4 page-body"></div>
          </div>
        </div>
      </div>
    </div>
  `;

  _asideEl = /** @type {HTMLElement} */ (container.querySelector('aside'));
  _settingsMountEl = /** @type {HTMLElement} */ (container.querySelector('.js-settings-mount'));
  _prefsMountEl = /** @type {HTMLElement} */ (container.querySelector('.js-prefs-mount'));
  _popularPanelEl  = /** @type {HTMLElement} */ (container.querySelector('[data-panel="popular"]'));
  _searchPanelEl   = /** @type {HTMLElement} */ (container.querySelector('[data-panel="search"]'));

  const popularGridEl  = /** @type {HTMLElement} */ (container.querySelector('.js-popular-grid'));
  const popularPaginEl = /** @type {HTMLElement} */ (container.querySelector('.js-popular-pagination'));
  const popularSizeMountEl = /** @type {HTMLElement} */ (container.querySelector('.js-popular-tile-size-mount'));
  const searchGridEl   = /** @type {HTMLElement} */ (container.querySelector('.js-search-grid'));
  const searchPaginEl  = /** @type {HTMLElement} */ (container.querySelector('.js-search-pagination'));
  const filterBtnEl    = /** @type {HTMLButtonElement} */ (container.querySelector('.js-filter-btn'));
  const searchEl       = /** @type {HTMLInputElement} */ (container.querySelector('.js-search'));
  const searchSizeMountEl = /** @type {HTMLElement} */ (container.querySelector('.js-tile-size-mount'));

  _updateBreadcrumb();

  // Wire add source button. Mount once per click and close via the returned
  // cleanup — mountIntoModalRoot gives each call its own container, so
  // re-mounting with open=false would stack an empty modal instead of closing.
  if (_addSourceBtn) {
    _addSourceBtn.addEventListener('click', () => {
      let cleanup = () => {};
      cleanup = mountIntoModalRoot(html`
        <${AddSourceModal}
          open=${true}
          onClose=${() => cleanup()}
          onCreated=${() => { cleanup(); _refreshSidebar(); }}
        />
      `);
    });
  }

  if (_query) searchEl.value = _query;

  const panels = /** @type {NodeListOf<HTMLElement>} */ (container.querySelectorAll('.js-panel'));

  let _popularFetched = false;

  const _switchTab = (/** @type {string} */ tab) => {
    _activeTab = tab;
    _tabsUpdateFn?.(tab);
    for (const panel of panels) {
      panel.classList.toggle('hidden', panel.dataset.panel !== tab);
    }
    if (tab === 'settings') _mountSettings();
    if (tab === 'prefs') _mountPrefs();
    if (tab === 'popular' && !_popularFetched) {
      _popularFetched = true;
      if (!_sourceEnabled) {
        _showDisabledPanel(/** @type {HTMLElement} */ (container.querySelector('[data-panel="popular"]')));
      } else {
        _fetch(popularGridEl, popularPaginEl, false);
      }
    }
    if (tab === 'search' && !_sourceEnabled) {
      _showDisabledPanel(/** @type {HTMLElement} */ (container.querySelector('[data-panel="search"]')));
    }
  };

  const _tabDefs = () => [
    { id: 'popular', name: t('source.tab.popular') },
    { id: 'search', name: t('source.tab.search') },
    { id: 'prefs', name: t('source.tab.preferences'), disabled: !_hasPrefs },
    { id: 'settings', name: t('source.tab.settings') },
  ];
  const tabsEl = /** @type {HTMLElement} */ (container.querySelector('.js-tabs'));
  const { update: tabsUpdate } = renderTabs(tabsEl, {
    tabs: _tabDefs(),
    activeId: _activeTab,
    onSelect: (tab) => { _switchTab(tab); _updateUrl(); },
  });
  _tabsUpdateFn = tabsUpdate;

  api.getPreferenceSchema(_sourceId).then(schema => {
    _hasPrefs = Array.isArray(schema) && schema.length > 0;
    if (_hasPrefs) _tabsUpdateFn?.(_activeTab, _tabDefs());
  }).catch(() => {
    _hasPrefs = true;
    _tabsUpdateFn?.(_activeTab, _tabDefs());
  });

  _switchTab(_activeTab);

  // If search tab is initial tab (arriving from URL with query/filter), fetch immediately
  if (_activeTab === 'search') {
    _fetch(searchGridEl, searchPaginEl, true);
  }

  searchEl.addEventListener('input', debounce(() => {
    _query = searchEl.value.trim();
    _page = 1;
    _updateBreadcrumb();
    _updateUrl();
    _fetch(searchGridEl, searchPaginEl, true);
  }, 600));

  const _renderTileSize = () => {
    for (const [mountEl, gridEl, paginEl, isSearch] of /** @type {Array<[HTMLElement, HTMLElement, HTMLElement, boolean]>} */ ([
      [searchSizeMountEl, searchGridEl, searchPaginEl, true],
      [popularSizeMountEl, popularGridEl, popularPaginEl, false],
    ])) {
      render(html`<${TileSizeSelect}
        value=${_tileSize}
        onChange=${(/** @type {import('../tile-size.js').TileSize} */ size) => {
          _tileSize = size;
          setTileSize(size);
          _page = 1;
          _updateUrl();
          // The new track width only exists after the browser has laid the grid
          // out again, so refetch on the next frame rather than this one.
          requestAnimationFrame(() => {
            _renderTileSize();
            if (!isSearch) _popularFetched = false;
            _fetch(gridEl, paginEl, isSearch);
            if (!isSearch) _popularFetched = true;
          });
        }}
      />`, mountEl);
    }
  };
  _renderTileSize();

  // Infinite mode does not fit: its batch is fixed, and refetching on resize
  // would discard every batch already appended.
  if (!_isInfinite()) {
    for (const [gridEl, paginEl, isSearch] of /** @type {Array<[HTMLElement, HTMLElement, boolean]>} */ ([
      [popularGridEl, popularPaginEl, false],
      [searchGridEl, searchPaginEl, true],
    ])) {
      _stopColumnWatches.push(observeCapacity(gridEl, () => _mangaGridIn(gridEl), () => gridEl, () => {
        // Only the visible tab refetches; the other measures again when shown.
        if ((_activeTab === 'search') !== isSearch) return;
        _page = 1;
        if (!isSearch) _popularFetched = false;
        _fetch(gridEl, paginEl, isSearch);
        if (!isSearch) _popularFetched = true;
      }, _pageSize));
    }
  }

  _filterDefs = [];
  _filterModalDestroy?.();
  _filterModalDestroy = null;
  api.getSourceFilters(_sourceId).then(fl => {
    const defs = Array.isArray(fl?.filters) ? fl.filters : [];
    if (defs.length === 0) return;
    _filterDefs = defs;

    // Apply pending URL filter params (from back/forward navigation)
    if (Object.keys(_pendingFilterParams).length > 0) {
      const merged = _buildDefaultFilters(_filterDefs);
      for (const [filterId, value] of Object.entries(_pendingFilterParams)) {
        try { merged[filterId] = JSON.parse(value); } catch { }
      }
      _filters = merged;
      _pendingFilterParams = {};
      if (_activeTab === 'search') _fetch(searchGridEl, searchPaginEl, true);
    }

    filterBtnEl.style.display = '';
    _filterModalDestroy = mountFilterModal(filterBtnEl, document.body, {
      filterDefs: _filterDefs,
      activeFilters: _filters,
      onChange: (updated) => {
        _filters = updated;
        _page = 1;
        _updateUrl();
        _fetch(searchGridEl, searchPaginEl, true);
      },
    });
  }).catch(() => {});

  document.title = t('source.title');
  api.getSource(_sourceId).then(src => {
    if (src?.name) {
      _sourceName = src.name;
      document.title = t('source.title.named', { name: src.name });
      _updateBreadcrumb();
    }
    if (src && src.enabled === false) {
      _sourceEnabled = false;
      if (_activeTab === 'popular') {
        _popularFetched = false;
        _showDisabledPanel(/** @type {HTMLElement} */ (container.querySelector('[data-panel="popular"]')));
        _popularFetched = true;
      } else if (_activeTab === 'search') {
        _showDisabledPanel(/** @type {HTMLElement} */ (container.querySelector('[data-panel="search"]')));
      }
    }
  }).catch(() => {});

  let _sidebarSources = /** @type {any[]} */ ([]);

  function _mountSidebar() {
    render(html`<${SourcesSidebar}
      sources=${_sidebarSources}
      activeSourceId=${_sourceId}
      canInstall=${hasPermission('source:install')}
      onCreated=${_refreshSidebar}
    />`, _asideEl);
  }

  async function _refreshSidebar() {
    try {
      const updated = await api.getSources();
      if (Array.isArray(updated)) {
        _sidebarSources = updated;
        _mountSidebar();
      }
    } catch { }
  }

  api.getSources().then(sources => {
    _sidebarSources = Array.isArray(sources) ? sources : [];
    _mountSidebar();
  }).catch(() => { _mountSidebar(); });

  _unsubSourcesInvalidation = subscribeCache('sourcesInvalidation', _refreshSidebar);

  let _prevPrefVersion = /** @type {number | undefined} */ (undefined);
  _unsubPrefVersion = subscribeUiState('sourcePreferenceVersion', (/** @type {Map<number, number>} */ map) => {
    const v = map.get(_sourceId);
    if (v === undefined || v === _prevPrefVersion) return;
    _prevPrefVersion = v;
    const popularGridEl = _popularPanelEl?.querySelector('.js-popular-grid');
    const popularPaginEl = _popularPanelEl?.querySelector('.js-popular-pagination');
    const searchGridEl = _searchPanelEl?.querySelector('.js-search-grid');
    const searchPaginEl = _searchPanelEl?.querySelector('.js-search-pagination');
    if (_activeTab === 'popular' && popularGridEl && popularPaginEl) {
      _fetch(/** @type {HTMLElement} */ (popularGridEl), /** @type {HTMLElement} */ (popularPaginEl), false);
    } else if (_activeTab === 'search' && searchGridEl && searchPaginEl) {
      _fetch(/** @type {HTMLElement} */ (searchGridEl), /** @type {HTMLElement} */ (searchPaginEl), true);
    }
  });
}


function _mountPrefs() {
  if (_prefsMounted || !_prefsMountEl) return;
  _prefsMounted = true;
  render(html`<${SourcePreferencesPanel} sourceId=${_sourceId} />`, _prefsMountEl);
}


async function _mountSettings() {
  if (_settingsMounted || !_settingsMountEl) return;
  _settingsMounted = true;

  const [sourceRes, activeIdsRes] = await Promise.allSettled([
    api.getSource(_sourceId),
    api.getActiveSourceIds(),
  ]);

  const source    = sourceRes.status === 'fulfilled' ? sourceRes.value : null;
  const activeIds = new Set(activeIdsRes.status === 'fulfilled' && Array.isArray(activeIdsRes.value)
    ? activeIdsRes.value
    : []);

  if (!source) {
    _settingsMountEl.appendChild(createErrorState({ message: t('source.error.settings') }));
    return;
  }

  render(
    html`<${SourceSettingsPage}
      source=${source}
      activeIds=${activeIds}
      onDeleted=${() => navigate('/sources')}
      onEnabledChange=${(enabled) => {
        _sourceEnabled = enabled;
        // Notify sidebar to re-fetch so the "Off" badge updates immediately
        updateCacheState('sourcesInvalidation', n => n + 1);
        // Refresh popular/search panels immediately
        if (_popularPanelEl) {
          const inner = _popularPanelEl.querySelector('.flex.flex-col');
          if (inner) {
            if (enabled) {
              inner.innerHTML = '';
              const gridEl = document.createElement('div');
              gridEl.className = 'js-popular-grid-dyn';
              gridEl.setAttribute('aria-live', 'polite');
              gridEl.setAttribute('aria-busy', 'false');
              const paginEl = document.createElement('div');
              paginEl.className = 'js-popular-pagination-dyn';
              inner.appendChild(gridEl);
              inner.appendChild(paginEl);
              _fetch(gridEl, paginEl, false);
            } else {
              inner.innerHTML = '';
              inner.appendChild(_disabledStateEl());
            }
          }
        }
        if (_searchPanelEl) {
          const inner = _searchPanelEl.querySelector('.flex.flex-col');
          if (inner && !enabled) {
            // Clear grid content but keep search bar; show disabled message in grid area
            const gridEl = inner.querySelector('.js-search-grid');
            if (gridEl) {
              gridEl.innerHTML = '';
              gridEl.appendChild(_disabledStateEl());
            }
          }
        }
      }}
    />`,
    _settingsMountEl,
  );
}


/**
 * @param {HTMLElement} gridEl
 * @param {HTMLElement} paginEl
 * @param {boolean} isSearch - true = search_manga (query+filters), false = get_popular_manga
 */
async function _fetch(gridEl, paginEl, isSearch) {
  _syncPageSize(gridEl);
  const infinite = _isInfinite();
  const isAppend = infinite && _page > 1;

  _abort?.abort();
  _abort = new AbortController();

  _sentinelObserver?.disconnect();
  _sentinelObserver = null;
  if (isSearch) {
    _destroyPaginationSearch?.();
    _destroyPaginationSearch = null;
  } else {
    _destroyPaginationPopular?.();
    _destroyPaginationPopular = null;
  }
  paginEl.innerHTML = '';

  const filtersJson = Object.keys(_filters).length > 0
    ? JSON.stringify(
        Object.entries(_filters).map(([id, stateObj]) => ({
        filter_name: id,
        state: stateObj
        }))
    )
    : undefined;

  const outcome = await fetchPagedGrid({
    gridEl,
    pageSize: _pageSize,
    append: isAppend,
    fetchPage: () => isSearch
      ? api.searchManga(_sourceId, _query, _page, _pageSize, filtersJson, /** @type {AbortController} */ (_abort).signal)
      : api.getPopularManga(_sourceId, _page, _pageSize, undefined, /** @type {AbortController} */ (_abort).signal),
    mapItems: (result) => Array.isArray(result?.manga) ? result.manga
      : Array.isArray(result)                          ? result
      : [],
    renderCard: (m) => createMangaCard({
      manga: { id: m.db_id ?? m.id, title: m.title, cover_image_url: m.cover_url ?? null },
      href: `/source/${_sourceId}/manga/${encodeURIComponent(m.source_manga_id ?? m.id)}`,
    }),
    emptyIcon: iconSearch,
    emptyTitle: isSearch ? t('source.search.empty') : t('source.popular.empty'),
    errorMessage: t('source.error.load_manga'),
    onRetry: () => _fetch(gridEl, paginEl, isSearch),
    onError: (e, gridEl) => {
      const err = /** @type {any} */ (e);
      if (err?.code === 'flaresolverr_required') {
        gridEl.appendChild(createErrorState({
          message: err?.hint ?? t('source.error.solver_required'),
          onRetry: () => _fetch(gridEl, paginEl, isSearch),
        }));
        return true;
      }
      if (err?.code !== 'source_disabled') return false;
      const panel = /** @type {HTMLElement | null} */ (gridEl.closest('[data-panel]'));
      if (panel) _showDisabledPanel(panel);
      else gridEl.appendChild(createErrorState({ message: t('source.disabled.title') }));
      return true;
    },
  });
  if (!outcome) return;
  if ('error' in outcome) { paginEl.innerHTML = ''; return; }

  const { result, items } = outcome;
  paginEl.innerHTML = '';
  const hasNext = hasNextPage(result, items.length, _pageSize);
  if (infinite) {
    _setupSourceSentinel(gridEl, paginEl, hasNext, isSearch);
  } else {
    if (_page > 1 || hasNext) {
      const { destroy } = renderPagination(paginEl, {
        page: _page,
        hasNext,
        total: result?.total_pages ?? undefined,
        onPageChange: (p) => { _page = p; _updateUrl(); _fetch(gridEl, paginEl, isSearch); scrollPageTop(); },
      });
      if (isSearch) {
        _destroyPaginationSearch = destroy;
      } else {
        _destroyPaginationPopular = destroy;
      }
    }
  }
}

/** Sets up (or clears) the IntersectionObserver sentinel for source-details infinite scroll. */
function _setupSourceSentinel(gridEl, paginEl, hasNext, isSearch) {
  _sentinelObserver?.disconnect();
  _sentinelObserver = null;
  if (!hasNext) return;

  const sentinel = document.createElement('div');
  sentinel.className = 'js-sentinel h-px';
  paginEl.appendChild(sentinel);

  _sentinelObserver = new IntersectionObserver((entries) => {
    if (entries[0].isIntersecting) {
      _sentinelObserver?.disconnect();
      _sentinelObserver = null;
      _page++;
      _updateUrl();
      _fetch(gridEl, paginEl, isSearch);
    }
  }, { rootMargin: '200px' });
  _sentinelObserver.observe(sentinel);
}


/** @param {HTMLElement} container */
export function destroy(container) {
  for (const stop of _stopColumnWatches) stop();
  _stopColumnWatches = [];
  _abort?.abort();
  _abort = null;
  _destroyPaginationSearch?.();
  _destroyPaginationSearch = null;
  _destroyPaginationPopular?.();
  _destroyPaginationPopular = null;
  _sentinelObserver?.disconnect();
  _sentinelObserver = null;
  if (_settingsMountEl) render(null, _settingsMountEl);
  _settingsMountEl = null;
  _settingsMounted = false;
  if (_prefsMountEl) render(null, _prefsMountEl);
  _prefsMountEl = null;
  _prefsMounted = false;
  _hasPrefs = false;
  _popularPanelEl = null;
  _searchPanelEl = null;
  _tabsUpdateFn = null;
  if (_asideEl) render(null, _asideEl);
  _asideEl = null;
  _filterModalDestroy?.();
  _filterModalDestroy = null;
  _unsubSourcesInvalidation?.();
  _unsubSourcesInvalidation = null;
  _unsubPrefVersion?.();
  _unsubPrefVersion = null;
  mountIntoModalRoot(null);
  _addSourceBtn = null;
  _headerActions = null;
  clearPageHeader();
  container.innerHTML = '';
}