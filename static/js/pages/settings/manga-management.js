// @ts-check

import { h } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { showToast, showApiError } from '../../components/toast.js';
import { showConfirm, Modal } from '../../components/modal.js';
import { useBusy } from '../../hooks/use-busy.js';
import { navigate } from '../../router.js';
import { skeletonSettingsCards } from '../../components/skeletons.js';
import { EmptyState } from '../../components/empty-state.js';
import { ErrorState } from '../../components/error-state.js';
import { Tabs } from '../../components/tabs.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/** @param {() => Promise<any>} fetcher */
function useLoader(fetcher) {
  const [state, setState] = useState(
    /** @type {{ status: string, data: any[], error: string }} */ ({
      status: 'loading',
      data: [],
      error: '',
    }),
  );
  const load = useCallback(async () => {
    setState((s) => ({ ...s, status: 'loading' }));
    try {
      const data = await fetcher();
      setState({ status: 'ready', data: Array.isArray(data) ? data : [], error: '' });
    } catch (e) {
      setState({ status: 'error', data: [], error: e?.message ?? '' });
    }
    // eslint-disable-next-line
  }, []);
  useEffect(() => {
    load();
  }, [load]);
  return /** @type {[typeof state, () => Promise<void>]} */ ([state, load]);
}

const Skeleton = () => html`${html([skeletonSettingsCards(3)])}`;

/** @param {string} href @param {string} cls @param {string} label */
function NavLink({ href, cls, label }) {
  return html`<a
    href=${href}
    class=${cls}
    onClick=${(/** @type {Event} */ e) => {
      e.preventDefault();
      navigate(href);
    }}
    >${label}</a
  >`;
}

/**
 * Picks the real series a pending import refers to, and links them.
 *
 * Resolution links a pending import to the selected existing series.
 */
function ResolveDialog({ item, onClose, onResolved }) {
  const [query, setQuery] = useState(item?.title ?? '');
  const [results, setResults] = useState(/** @type {any[]|null} */ (null));
  const { busy, run } = useBusy();

  const search = () =>
    run(async () => {
      setResults(null);
      try {
        const res = await api.globalSearch(query, 'all', 1, 10);
        const flat = [];
        for (const group of res ?? []) {
          for (const m of group.manga ?? []) {
            flat.push({ ...m, source_id: group.source_id, source_name: group.source_name });
          }
        }
        setResults(flat);
      } catch (e) {
        showApiError(e);
        setResults([]);
      }
    });

  const pick = (/** @type {any} */ m) =>
    run(async () => {
      try {
        await api.resolvePendingImport(item.id, m.source_id, m.id);
        showToast(t('settings.manga_mgmt.pending.resolved'), { type: 'success' });
        onResolved();
        onClose();
      } catch (e) {
        showApiError(e);
      }
    });

  return html`
    <${Modal} open=${!!item} title=${t('settings.manga_mgmt.pending.resolve')} onClose=${onClose}>
      <div class="flex flex-col gap-3 px-1">
        <p class="text-xs text-text-muted">${t('settings.manga_mgmt.pending.resolve.desc')}</p>
        <div class="flex gap-2">
          <input
            class="input flex-1"
            value=${query}
            onInput=${(/** @type {any} */ e) => setQuery(e.currentTarget.value)}
            onKeyDown=${(/** @type {any} */ e) => { if (e.key === 'Enter') search(); }}
            aria-label=${t('settings.manga_mgmt.pending.resolve.search')}
          />
          <button type="button" class="btn-secondary btn-sm" disabled=${busy} onClick=${search}>
            ${t('settings.manga_mgmt.pending.resolve.search')}
          </button>
        </div>
        ${results === null
          ? null
          : results.length === 0
            ? html`<p class="text-sm text-text-muted">${t('settings.manga_mgmt.pending.resolve.none')}</p>`
            : html`<div class="flex flex-col">
                ${results.map(
                  (m) => html`
                    <button
                      type="button"
                      key=${`${m.source_id}:${m.id}`}
                      class="text-left px-1 py-2 border-b border-border-subtle hover:bg-surface-hover"
                      disabled=${busy}
                      onClick=${() => pick(m)}
                    >
                      <span class="block text-sm text-text truncate">${m.title}</span>
                      <span class="block text-xs text-text-muted">${m.source_name}</span>
                    </button>
                  `,
                )}
              </div>`}
      </div>
    <//>
  `;
}

function PendingPanel() {
  const [state, load] = useLoader(() => api.getPendingImports());
  const [resolving, setResolving] = useState(/** @type {any} */ (null));

  const dismiss = async (/** @type {any} */ item) => {
    try {
      await api.deletePendingImport(item.id);
      await load();
    } catch (e) {
      showApiError(e);
    }
  };

  if (state.status === 'loading') return html`<${Skeleton} />`;
  if (state.status === 'error') {
    return html`<${ErrorState}
      message=${t('settings.manga_mgmt.load_failed', { msg: state.error })}
      onRetry=${load}
    />`;
  }
  if (state.data.length === 0) {
    return html`<${EmptyState}
      title=${t('settings.manga_mgmt.pending.empty.title')}
      subtitle=${t('settings.manga_mgmt.pending.empty.desc')}
    />`;
  }

  return html`
    <div class="flex flex-col gap-2">
      ${state.data.map(
        (item) => html`
          <div class="bg-surface-2 rounded-xl p-4 flex flex-col gap-2" key=${item.id}>
            <div class="flex items-start justify-between gap-2">
              <div class="flex flex-col gap-0.5 min-w-0">
                <p class="font-medium text-sm truncate">${item.title}</p>
                <p class="text-xs text-text-muted">
                  ${item.origin === 'tachiyomi' ? 'Tachiyomi' : 'Kani Backup'}${item.source_hint
                    ? ' · ' + item.source_hint
                    : ''}
                </p>
                ${item.possible_duplicate_of &&
                html`<p class="text-xs text-warn">
                  ${t('settings.manga_mgmt.pending.dup.prefix')}${' '}
                  <${NavLink}
                    href=${`/manga/${item.possible_duplicate_of}`}
                    cls="underline font-medium"
                    label=${item.possible_duplicate_title ?? '#' + item.possible_duplicate_of}
                  />${item.duplicate_similarity
                    ? ' ' +
                      t('settings.manga_mgmt.pending.dup.match', {
                        pct: Math.round(item.duplicate_similarity * 100),
                      })
                    : ''}
                </p>`}
              </div>
              <div class="flex gap-2 shrink-0">
                <${NavLink}
                  href=${`/sources?search=${encodeURIComponent(item.title)}`}
                  cls="btn-secondary btn-sm"
                  label=${t('settings.manga_mgmt.pending.find_btn')}
                />
                <button
                  type="button"
                  class="btn-secondary btn-sm"
                  onClick=${() => setResolving(item)}
                >
                  ${t('settings.manga_mgmt.pending.resolve')}
                </button>
                <button
                  type="button"
                  class="btn-secondary btn-sm"
                  onClick=${() => dismiss(item)}
                >
                  ${t('settings.manga_mgmt.pending.dismiss_btn')}
                </button>
              </div>
            </div>
          </div>
        `,
      )}
      ${resolving &&
      html`<${ResolveDialog}
        item=${resolving}
        onClose=${() => setResolving(null)}
        onResolved=${load}
      />`}
    </div>
  `;
}

function DuplicatesPanel() {
  const [state, load] = useLoader(() => api.getDuplicates());

  const merge = async (/** @type {any} */ keep, /** @type {any} */ discard) => {
    if (
      !(await showConfirm(
        t('settings.manga_mgmt.dupes.merge.confirm', { keep: keep.name, discard: discard.name }),
        {
          title: t('settings.manga_mgmt.dupes.merge.title'),
          confirmLabel: t('settings.manga_mgmt.dupes.merge.btn'),
          danger: true,
        },
      ))
    )
      return;
    try {
      await api.mergeDuplicate(keep.id, discard.id);
      showToast(t('settings.manga_mgmt.dupes.merge.success', { name: keep.name }), {
        type: 'success',
      });
      await load();
    } catch (e) {
      showApiError(e);
    }
  };

  const notDup = async (/** @type {any} */ pair) => {
    try {
      await api.dismissDuplicate(pair.manga_a.id, pair.manga_b.id);
      await load();
    } catch (e) {
      showApiError(e);
    }
  };

  if (state.status === 'loading') return html`<${Skeleton} />`;
  if (state.status === 'error') {
    return html`<${ErrorState}
      message=${t('settings.manga_mgmt.load_failed', { msg: state.error })}
      onRetry=${load}
    />`;
  }
  if (state.data.length === 0) {
    return html`<${EmptyState}
      title=${t('settings.manga_mgmt.dupes.empty.title')}
      subtitle=${t('settings.manga_mgmt.dupes.empty.desc')}
    />`;
  }

  const mangaCol = (/** @type {any} */ m) => html`<div class="flex-1 min-w-0">
    <${NavLink}
      href=${`/manga/${m.id}`}
      cls="font-medium text-sm text-text hover:underline truncate block"
      label=${m.name}
    />
  </div>`;

  return html`
    <div class="flex flex-col gap-2">
      ${state.data.map(
        (pair, i) => html`
          <div class="bg-surface-2 rounded-xl p-4 flex flex-col gap-2" key=${i}>
            <div class="flex items-start gap-4">
              ${mangaCol(pair.manga_a)}
              <div class="text-xs text-text-muted shrink-0 pt-0.5">
                ${Math.round(pair.similarity * 100)}%${pair.author_match ? ' · author' : ''}
              </div>
              ${mangaCol(pair.manga_b)}
            </div>
            <div class="flex gap-2 flex-wrap">
              <button type="button" class="btn-danger btn-sm" onClick=${() => merge(pair.manga_a, pair.manga_b)}>
                ${t('settings.manga_mgmt.dupes.keep_btn', { name: pair.manga_a.name })}
              </button>
              <button type="button" class="btn-danger btn-sm" onClick=${() => merge(pair.manga_b, pair.manga_a)}>
                ${t('settings.manga_mgmt.dupes.keep_btn', { name: pair.manga_b.name })}
              </button>
              <button type="button" class="btn-secondary btn-sm" onClick=${() => notDup(pair)}>
                ${t('settings.manga_mgmt.dupes.not_dup')}
              </button>
            </div>
          </div>
        `,
      )}
    </div>
  `;
}

function OrphanedPanel() {
  const [state, load] = useLoader(() => api.getOrphanedManga());

  const del = async (/** @type {any} */ item) => {
    if (
      !(await showConfirm(t('settings.manga_mgmt.orphaned.delete.confirm', { name: item.name }), {
        title: t('settings.manga_mgmt.orphaned.delete.title'),
        confirmLabel: t('common.delete'),
      }))
    )
      return;
    try {
      await api.deleteManga(item.id);
      await load();
    } catch (e) {
      showApiError(e);
    }
  };

  if (state.status === 'loading') return html`<${Skeleton} />`;
  if (state.status === 'error') {
    return html`<${ErrorState}
      message=${t('settings.manga_mgmt.load_failed', { msg: state.error })}
      onRetry=${load}
    />`;
  }
  if (state.data.length === 0) {
    return html`<${EmptyState}
      title=${t('settings.manga_mgmt.orphaned.empty.title')}
      subtitle=${t('settings.manga_mgmt.orphaned.empty.desc')}
    />`;
  }

  return html`
    <div class="flex flex-col gap-2">
      ${state.data.map(
        (item) => html`
          <div
            class="bg-surface-2 rounded-xl p-4 flex items-center justify-between gap-4"
            key=${item.id}
          >
            <div class="flex flex-col gap-0.5 min-w-0">
              <p class="font-medium text-sm truncate">${item.name}</p>
              <p class="text-xs text-text-muted">
                ${t('settings.manga_mgmt.orphaned.from', { source: item.source_name })}
              </p>
            </div>
            <div class="flex gap-2 shrink-0">
              <${NavLink}
                href=${`/manga/${item.id}`}
                cls="btn-secondary btn-sm"
                label=${t('settings.manga_mgmt.orphaned.migrate')}
              />
              <button type="button" class="btn-danger btn-sm" onClick=${() => del(item)}>
                ${t('common.delete')}
              </button>
            </div>
          </div>
        `,
      )}
    </div>
  `;
}

export function MangaManagementSection() {
  const [active, setActive] = useState('pending');
  const tabs = [
    { id: 'pending', name: t('settings.manga_mgmt.tab.pending') },
    { id: 'dupes', name: t('settings.manga_mgmt.tab.dupes') },
    { id: 'orphaned', name: t('settings.manga_mgmt.tab.orphaned') },
  ];

  return html`
    <div>
      <div class="mb-4">
        <${Tabs} tabs=${tabs} activeId=${active} onSelect=${setActive} />
      </div>
      ${active === 'pending'
        ? html`<${PendingPanel} />`
        : active === 'dupes'
        ? html`<${DuplicatesPanel} />`
        : html`<${OrphanedPanel} />`}
    </div>
  `;
}
