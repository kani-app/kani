// @ts-check
// Settings page — Preact host. Nav, search, save bar, and section content are
// one component tree; sections are ordinary child components. The router's
// init/destroy render/unmount the tree.

import { h, render } from 'preact';
import { useState, useEffect, useMemo, useRef } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { hasPermission } from '../../session.js';
import { deferredSkeleton } from '../../utils.js';
import { iconLock } from '../../icons.js';
import { t } from '../../i18n.js';
import { buildSettingsSearchIndex } from '../../settings-search-index.js';
import { isPhoneLayout, PAGINATION_SETTINGS_PREFIX } from '../../pagination-mode.js';
import { skeletonSettingsCards } from '../../components/skeletons.js';
import { EmptyState } from '../../components/empty-state.js';
import { RestartTray } from '../../components/restart-tray.js';
import { setPageHeader, clearPageHeader } from '../../components/app-header.js';
import { setBeforeNavigate, clearBeforeNavigate } from '../../router.js';
import { showConfirm } from '../../components/modal.js';
import { showApiError } from '../../components/toast.js';
import { pushState as pushUrlState } from '../../url-params.js';
import { useBusy } from '../../hooks/use-busy.js';
import { formDirty, runSave, runReset } from './form-bus.js';

import { GeneralSection } from './general.js';
import { LibrarySection } from './library.js';
import { ScanlatorsSection } from './scanlators.js';
import { CollectionsSection } from './collections.js';
import { MangaManagementSection } from './manga-management.js';
import { TrashSection } from './trash.js';
import { DownloadsSection } from './downloads.js';
import { OfflineSection } from './offline.js';
import { ScanSection } from './scan.js';
import { TrackersSection } from './trackers.js';
import { EmailSection } from './email.js';
import { WebhooksSection } from './webhooks.js';
import { AdvancedSection } from './advanced.js';
import { StorageSection } from './storage.js';
import { MaintenanceSection } from './maintenance.js';
import { ServerSection } from './server.js';
import { SourcesHealthSection } from './sources-health.js';
import { AccountSection } from './account.js';
import { ClientsSection } from './clients.js';
import { SecuritySection } from './security.js';
import { DiagnosticsSection } from './diagnostics.js';

const html = htm.bind(h);

/** @param {string} message */
function confirmDiscard(message) {
  return showConfirm(message, {
    title: t('settings.unsaved.title'),
    confirmLabel: t('settings.unsaved.leave'),
    cancelLabel: t('settings.unsaved.stay'),
  });
}

/** @param {any} settings @param {any[]} categories @param {string} bootId */
function buildSections(settings, categories, bootId) {
  const g = { server: t('settings.group.server'), account: t('settings.group.account') };
  return [
    { id: 'general', perm: null, C: GeneralSection, props: {} },
    { id: 'library', perm: 'library:manage', C: LibrarySection, props: { categories } },
    { id: 'collections', perm: 'library:manage', C: CollectionsSection, props: {} },
    { id: 'manga-management', perm: 'library:manage', C: MangaManagementSection, props: {} },
    { id: 'scanlators', perm: 'library:manage', C: ScanlatorsSection, props: {} },
    { id: 'trash', perm: 'library:view', C: TrashSection, props: {} },
    { id: 'downloads', perm: 'settings:edit_download', C: DownloadsSection, props: { settings } },
    { id: 'offline', perm: null, C: OfflineSection, props: {} },
    { id: 'scan', perm: 'settings:edit_scan', C: ScanSection, props: { settings } },
    { id: 'trackers', perm: null, C: TrackersSection, props: { settings } },
    { id: 'email', perm: 'settings:edit_advanced', group: g.server, C: EmailSection, props: { settings } },
    { id: 'webhooks', perm: 'settings:edit_advanced', group: g.server, C: WebhooksSection, props: {} },
    { id: 'advanced', perm: 'settings:edit_advanced', group: g.server, C: AdvancedSection, props: { settings, bootId } },
    { id: 'storage', perm: 'admin:manage', group: g.server, C: StorageSection, props: {} },
    { id: 'maintenance', perm: 'settings:edit_advanced', group: g.server, C: MaintenanceSection, props: { settings } },
    { id: 'server', perm: 'server:manage', group: g.server, C: ServerSection, props: {} },
    {
      id: 'sources-health',
      perm: 'source:browse',
      group: g.server,
      C: SourcesHealthSection,
      props: {},
    },
    {
      id: 'diagnostics',
      perm: 'server:manage',
      group: g.server,
      C: DiagnosticsSection,
      props: {},
    },
    { id: 'account', perm: null, group: g.account, C: AccountSection, props: {} },
    { id: 'clients', perm: null, group: g.account, C: ClientsSection, props: {} },
    { id: 'security', perm: null, group: g.account, C: SecuritySection, props: {} },
  ].map((s) => ({
    ...s,
    label: t(`settings.section.${s.id.replace(/-/g, '_')}.label`),
    description: t(`settings.section.${s.id.replace(/-/g, '_')}.desc`),
  }));
}

function SaveBar() {
  const dirty = formDirty.value;
  const { busy, run } = useBusy();
  if (!dirty) return null;
  return html`
    <div class="sticky bottom-0 max-w-4xl w-full px-4 md:px-8 pb-4 pt-2">
      <div class="flex items-center gap-3 bg-surface border border-border rounded-xl px-4 py-3 shadow-lg">
        <span class="dirty-dot shrink-0" aria-hidden="true"></span>
        <span class="text-sm text-text flex-1">${t('settings.savebar.unsaved')}</span>
        <button type="button" class="btn-ghost btn-sm" disabled=${busy} onClick=${runReset}>
          ${t('settings.savebar.discard')}
        </button>
        <button
          type="button"
          class="btn-primary btn-sm"
          disabled=${busy}
          onClick=${() => run(async () => {
            try {
              await runSave();
            } catch (e) {
              showApiError(e);
            }
          })}
        >
          ${t('common.save')}
        </button>
      </div>
    </div>
  `;
}

function AccessDenied() {
  return html`
    <div class="flex flex-col items-center justify-center gap-3 py-20 text-text-muted">
      <span class="icon-xl opacity-40" aria-hidden="true">${html([iconLock])}</span>
      <p class="text-base font-medium text-text">${t('settings.access_denied.title')}</p>
      <p class="text-sm">${t('settings.access_denied.desc')}</p>
    </div>
  `;
}

/** Scans rendered rows for text matching `q`, marks the best row and scrolls to it. */
function highlightMatches(root, q, single) {
  if (!root) return;
  for (const el of root.querySelectorAll('.search-hit')) el.classList.remove('search-hit');
  if (!q) return;
  /** @type {HTMLElement|null} */ let exact = null;
  /** @type {Set<HTMLElement>} */ const partial = new Set();
  for (const el of root.querySelectorAll('span, p, label, div')) {
    const ownText = Array.from(el.childNodes)
      .filter((n) => n.nodeType === Node.TEXT_NODE)
      .map((n) => n.textContent)
      .join('')
      .trim();
    if (!ownText.toLowerCase().includes(q)) continue;
    const row = el.closest('[data-settings-row]') ?? el.closest('.pref-row');
    if (!row) continue;
    if (ownText.toLowerCase() === q) exact ??= row;
    partial.add(/** @type {HTMLElement} */ (row));
  }
  let rows = [...partial];
  if (single) rows = exact ? [exact] : rows.slice(0, 1);
  for (const row of rows) row.classList.add('search-hit');
  const first = exact ?? rows[0] ?? null;
  if (first) first.scrollIntoView({ block: 'center', behavior: 'smooth' });
}

/** @param {{ settings: any, categories: any[], bootId: string }} props */
function SettingsPage({ settings, categories, bootId }) {
  const allSections = useMemo(() => buildSections(settings, categories, bootId), [settings, categories, bootId]);
  const sections = useMemo(() => allSections.filter((s) => !s.perm || hasPermission(s.perm)), [allSections]);
  const searchIndex = useMemo(() => buildSettingsSearchIndex(sections, { exclude: isPhoneLayout() ? [PAGINATION_SETTINGS_PREFIX] : [] }), [sections]);

  const initialFromUrl = () => {
    const p = new URLSearchParams(location.search);
    return { section: p.get('section'), q: p.get('q') };
  };
  const url = initialFromUrl();
  const validInitial = sections.find((s) => s.id === url.section)?.id;
  const [active, setActive] = useState(
    validInitial ?? (window.innerWidth >= 1024 ? sections[0]?.id ?? null : null),
  );
  const [query, setQuery] = useState(url.q ?? '');
  const [highlight, setHighlight] = useState(/** @type {string|null} */ (null));
  const contentRef = useRef(/** @type {HTMLElement|null} */ (null));

  const dirty = formDirty.value;
  const q = query.trim().toLowerCase();

  const sectionHits = useMemo(() => {
    /** @type {Map<string, any[]>} */
    const m = new Map();
    if (!q) return m;
    for (const s of sections) {
      const hits = (searchIndex.get(s.id) ?? []).filter(
        (it) => it.label.toLowerCase().includes(q) || it.desc.toLowerCase().includes(q),
      );
      if (hits.length) m.set(s.id, hits);
    }
    return m;
  }, [q, sections, searchIndex]);

  const filteredSections = useMemo(() => {
    if (!q) return sections;
    return sections.filter(
      (s) => (s.label + ' ' + s.description).toLowerCase().includes(q) || sectionHits.has(s.id),
    );
  }, [q, sections, sectionHits]);

  const activeSection = sections.find((s) => s.id === active) ?? null;

  useEffect(() => {
    if (q) {
      setPageHeader({ crumbs: [{ label: t('settings.crumb') }] });
    } else if (activeSection) {
      setPageHeader({
        crumbs: [{ label: t('settings.crumb'), href: '/settings' }, { label: activeSection.label }],
      });
    } else {
      setPageHeader({ crumbs: [{ label: t('settings.crumb') }] });
    }
  }, [active, q, activeSection]);

  useEffect(() => {
    setBeforeNavigate(async () => {
      if (formDirty.value && !(await confirmDiscard(t('settings.unsaved.page.message')))) return false;
      return true;
    });
    return () => clearBeforeNavigate();
  }, []);

  useEffect(() => {
    if (!highlight && !q) return;
    const term = (highlight ?? q).toLowerCase();
    const single = highlight != null;
    const timers = [50, 400, 1200].map((d) =>
      setTimeout(() => highlightMatches(contentRef.current, term, single), d),
    );
    return () => timers.forEach(clearTimeout);
  }, [active, highlight, q]);

  const go = async (/** @type {string} */ id, push = true) => {
    if (dirty && !(await confirmDiscard(t('settings.unsaved.section.message')))) return;
    setQuery('');
    setHighlight(null);
    setActive(id);
    if (push) pushUrlState({ section: id });
  };

  const goHitSection = async (/** @type {string} */ id, /** @type {string} */ label) => {
    if (dirty && !(await confirmDiscard(t('settings.unsaved.section.message')))) return;
    setQuery('');
    setActive(id);
    setHighlight(label);
    pushUrlState({ section: id });
  };

  const backToList = async () => {
    if (dirty && !(await confirmDiscard(t('settings.unsaved.section.message')))) return;
    setActive(null);
    pushUrlState({ section: null });
  };

  const applySearch = (/** @type {string} */ v) => {
    setQuery(v);
    setHighlight(null);
  };

  const desktopNav = () => {
    if (filteredSections.length === 0) {
      return html`<div class="px-2 py-2 text-xs text-text-faint">${t('settings.search.empty')}</div>`;
    }
    let lastGroup = '';
    /** @type {any[]} */
    const out = [];
    for (const s of filteredSections) {
      if (s.group && s.group !== lastGroup) {
        out.push(html`<div class="nav-section" key=${'g' + s.group}>${s.group}</div>`);
        lastGroup = s.group;
      }
      const hits = sectionHits.get(s.id);
      out.push(html`
        <button
          type="button"
          class=${'nav-item w-full text-left' + (s.id === active && !q ? ' active' : '')}
          data-section=${s.id}
          aria-current=${s.id === active && !q ? 'page' : 'false'}
          onClick=${() => go(s.id)}
          key=${s.id}
        >
          ${s.label}
          ${q && hits ? html`<span class="nav-badge ml-auto">${hits.length}</span>` : null}
          ${!q && dirty && s.id === active ? html`<span class="dirty-dot ml-auto"></span>` : null}
        </button>
      `);
    }
    return out;
  };

  const mobileNav = () =>
    filteredSections.map((s) => {
      const hits = sectionHits.get(s.id);
      return html`
        <button
          type="button"
          class="w-full text-left px-4 py-3.5 text-sm text-text hover:bg-surface-2 transition-colors flex items-center justify-between"
          onClick=${() => go(s.id)}
          key=${s.id}
        >
          <span>${s.label}</span>
          ${q && hits
            ? html`<span class="nav-badge">${hits.length}</span>`
            : html`<span class="text-text-muted text-xs">›</span>`}
        </button>
      `;
    });

  const content = () => {
    if (q) {
      if (sectionHits.size === 0) {
        return html`
          <div class="section-card-header border border-border rounded-xl mb-2 bg-surface">
            <div><h2>${t('settings.search.results.title')}</h2></div>
          </div>
          <${EmptyState}
            title=${t('settings.search.results.empty.title')}
            subtitle=${t('settings.search.results.empty.subtitle')}
          />
        `;
      }
      return html`
        <div class="section-card-header border border-border rounded-xl mb-2 bg-surface">
          <div><h2>${t('settings.search.results.title')}</h2></div>
        </div>
        ${sections
          .filter((s) => sectionHits.has(s.id))
          .map(
            (s) => html`
              <div class="flex flex-col gap-2" key=${s.id}>
                <h3 class="font-display text-base font-bold text-text px-1">${s.label}</h3>
                <div class="bg-surface border border-border-subtle rounded-xl divide-y divide-border-subtle overflow-hidden">
                  ${(sectionHits.get(s.id) ?? []).map(
                    (hit) => html`
                      <button
                        type="button"
                        class="w-full flex items-center justify-between gap-3 px-4 py-3 text-left hover:bg-surface-2 transition-colors"
                        onClick=${() => goHitSection(s.id, hit.label)}
                        key=${hit.key}
                      >
                        <span class="flex flex-col gap-0.5 min-w-0">
                          <span class="text-sm font-medium text-text truncate">${hit.label}</span>
                          ${hit.desc
                            ? html`<span class="text-xs text-text-muted truncate">${hit.desc}</span>`
                            : null}
                        </span>
                        <span class="text-xs text-text-faint shrink-0">${s.label}</span>
                      </button>
                    `,
                  )}
                </div>
              </div>
            `,
          )}
      `;
    }
    if (activeSection) {
      const Section = activeSection.C;
      return html`
        <div class="section-card-header border border-border rounded-xl mb-2 bg-surface">
          <div>
            <h2>${activeSection.label}</h2>
            ${activeSection.description ? html`<p>${activeSection.description}</p>` : null}
          </div>
        </div>
        <div class="flex flex-col gap-5">
          <${Section} ...${activeSection.props} />
        </div>
      `;
    }
    return null;
  };

  const showMobileBack = activeSection && !q;

  return html`
    <div class="flex h-full min-h-0 flex-1">
      <aside
        class="hidden lg:flex flex-col w-52 shrink-0 border-r border-border-subtle overflow-y-auto"
        aria-label="Settings sections"
      >
        <div class="p-2 flex flex-col gap-0.5 pt-4">
          <div class="px-2 pb-1">
            <input
              type="search"
              placeholder=${t('settings.search.placeholder')}
              autocomplete="off"
              class="touch-min-h w-full text-xs bg-surface-2 border border-border-subtle rounded-lg px-2.5 py-1.5 outline-none focus:ring-1 focus:ring-accent/50 placeholder:text-text-faint text-text"
              aria-label=${t('settings.search.placeholder')}
              value=${query}
              onInput=${(e) => applySearch(e.target.value)}
            />
          </div>
          <div class="nav-section">${t('settings.crumb')}</div>
          <div>${desktopNav()}</div>
        </div>
      </aside>
      <div class="flex-1 min-w-0 flex flex-col overflow-y-auto pb-nav-safe">
        <div class="px-4 md:px-8 pt-4">
          <${RestartTray}
            currentBootId=${bootId}
            onRestart=${async () => {
              try {
                await api.serverRestart();
              } catch {
                /* handled in server section */
              }
            }}
          />
        </div>
        <div class=${'lg:hidden flex flex-col gap-0 px-0 py-2' + (showMobileBack ? ' hidden' : '')}>
          <div class="px-4 pt-2 pb-1 lg:hidden">
            <input
              type="search"
              placeholder=${t('settings.search.placeholder_mobile')}
              autocomplete="off"
              class="touch-min-h w-full text-sm bg-surface-2 border border-border-subtle rounded-lg px-3 py-2 outline-none focus:ring-1 focus:ring-accent/50 placeholder:text-text-faint text-text"
              aria-label=${t('settings.search.placeholder_mobile')}
              value=${query}
              onInput=${(e) => applySearch(e.target.value)}
            />
          </div>
          <div class="flex flex-col divide-y divide-border-subtle border-t border-border-subtle">
            ${mobileNav()}
          </div>
        </div>
        ${showMobileBack
          ? html`<button
              type="button"
              class="lg:hidden flex items-center gap-2 px-4 py-3 text-sm text-accent hover:text-accent/80 transition-colors"
              onClick=${backToList}
            >
              <span aria-hidden="true">‹</span> ${t('settings.mobile_back')}
            </button>`
          : null}
        <div
          ref=${contentRef}
          class=${'max-w-4xl w-full px-4 md:px-8 py-4 md:py-6 flex flex-col gap-6' +
          (!q && !activeSection ? ' hidden lg:flex' : '')}
        >
          ${content()}
        </div>
        <${SaveBar} />
      </div>
    </div>
  `;
}

/** @param {HTMLElement} container */
export async function init(container) {
  document.title = t('settings.page_title');
  setPageHeader({ crumbs: [{ label: t('settings.crumb') }] });

  if (!hasPermission('settings:view')) {
    render(html`<${AccessDenied} />`, container);
    return;
  }

  const cancelSkeleton = deferredSkeleton(() => {
    container.innerHTML = `<div class="max-w-page mx-auto px-4 md:px-6 py-8">${skeletonSettingsCards(5)}</div>`;
  });

  const [settings, categories, bootData] = await Promise.allSettled([
    api.getSettings(),
    api.getCategories(),
    api.getBootId(),
  ]).then((r) => r.map((s) => (s.status === 'fulfilled' ? s.value : null)));

  cancelSkeleton();

  const bootId = bootData?.boot_id ?? bootData ?? '';
  const catList = Array.isArray(categories) ? categories : [];

  render(
    html`<${SettingsPage} settings=${settings} categories=${catList} bootId=${bootId} />`,
    container,
  );
}

/** @param {HTMLElement} container */
export function destroy(container) {
  clearPageHeader();
  clearBeforeNavigate();
  render(null, container);
}
