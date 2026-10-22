// @ts-check

import { h, render } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { Modal } from './modal.js';
import { EmptyState } from './empty-state.js';
import { Callout } from './form/callout.js';
import { iconChevronRight } from '../icons.js';
import { Icon } from './icon.js';
import { t } from '../i18n.js';
import { subscribeJob } from '../sse.js';
const html = htm.bind(h);

/** @typedef {'search'|'previewing'|'preview'|'confirming'|'done'} MigrationStep */

/**
 * @param {{
 *   dbId: number,
 *   currentSourceId: number,
 *   currentSourceName: string,
 *   currentTitle: string,
 *   currentCoverUrl?: string | null,
 *   onComplete: (newSourceId: number, newMangaId: string) => void,
 *   onClose: () => void,
 * }} props
 */
function MigrationDialogue({
  dbId, currentSourceId, currentSourceName, currentTitle, currentCoverUrl,
  onComplete, onClose,
}) {
  /** @type {[MigrationStep, (s: MigrationStep) => void]} */
  const [step, setStep] = useState(/** @type {MigrationStep} */ ('search'));
  const [scope, setScope] = useState(/** @type {'FavouritedOnly'|'AllEnabled'} */ ('FavouritedOnly'));
  const [query, setQuery] = useState(currentTitle);
  const [searchResults, setSearchResults] = useState(/** @type {any[]} */ ([]));
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState(/** @type {string|null} */ (null));

  const [targetSid, setTargetSid] = useState(0);
  const [targetMid, setTargetMid] = useState('');
  const [targetTitle, setTargetTitle] = useState('');
  const [targetCoverUrl, setTargetCoverUrl] = useState(/** @type {string|null} */ (null));

  const [preview, setPreview] = useState(/** @type {any|null} */ (null));
  const [keepOrphaned, setKeepOrphaned] = useState(true);
  const [error, setError] = useState(/** @type {string|null} */ (null));
  const [result, setResult] = useState(/** @type {any|null} */ (null));

  const debounceRef = useRef(/** @type {ReturnType<typeof setTimeout>|null} */ (null));
  const abortRef = useRef(/** @type {AbortController|null} */ (null));
  const unsubscribeRef = useRef(/** @type {(() => void)|null} */ (null));

  // A migration outlives this dialogue: closing it must not leave a listener
  // holding a setState on an unmounted tree.
  useEffect(() => () => unsubscribeRef.current?.(), []);

  useEffect(() => {
    if (!query.trim()) { setSearchResults([]); return; }
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => _doSearch(), 400);
    return () => { if (debounceRef.current) clearTimeout(debounceRef.current); };
  }, [query, scope]);

  async function _doSearch() {
    abortRef.current?.abort();
    abortRef.current = new AbortController();
    setSearching(true);
    setSearchError(null);
    try {
      const res = await api.globalSearch(query, scope, 1, 20, abortRef.current.signal);
      const grouped = Array.isArray(res?.results) ? res.results
        : Array.isArray(res)                       ? res
        : [];
      setSearchResults(grouped);
    } catch (e) {
      if (e?.name !== 'AbortError') setSearchError(t('migration.error.search_failed'));
    } finally {
      setSearching(false);
    }
  }

  async function _selectTarget(sid, mid, title, coverUrl) {
    setTargetSid(sid);
    setTargetMid(mid);
    setTargetTitle(title);
    setTargetCoverUrl(coverUrl ?? null);
    setStep('previewing');
    setError(null);
    try {
      const res = await api.previewMigration(dbId, sid, mid);
      setPreview(res);
      setStep('preview');
    } catch {
      setError(t('migration.error.preview_failed'));
      setStep('search');
    }
  }

  async function _confirmMigration() {
    setStep('confirming');
    setError(null);
    try {
      const { job_id: jobId } = await api.migrateManga(dbId, targetSid, targetMid, keepOrphaned);
      if (!jobId) {
        setError(t('migration.error.migrate_failed'));
        setStep('preview');
        return;
      }
      unsubscribeRef.current = subscribeJob(jobId, {
        onComplete: async () => {
          try {
            const [job] = await api.getMigrationStatuses([jobId]);
            setResult(job?.result ?? null);
            setStep('done');
          } catch {
            setError(t('migration.error.migrate_failed'));
            setStep('preview');
          }
        },
        onFailed: (/** @type {any} */ e) => {
          setError(e?.message || t('migration.error.migrate_failed'));
          setStep('preview');
        },
        onCancelled: () => {
          setError(t('migration.error.migrate_cancelled'));
          setStep('preview');
        },
      });
    } catch (/** @type {any} */ e) {
      setError(e?.status === 409
        ? t('migration.error.already_running')
        : t('migration.error.migrate_failed'));
      setStep('preview');
    }
  }

  /** @type {Map<string, { sourceName: string, sourceId: number, items: any[] }>} */
  const bySource = new Map();
  for (const sourceResult of searchResults) {
    bySource.set(String(sourceResult.source_id), {
      sourceName: sourceResult.source_name ?? String(sourceResult.source_id),
      sourceId: sourceResult.source_id,
      items: sourceResult.manga ?? [],
    });
  }

  const footer = step === 'preview' && html`
    <div class="flex gap-3 justify-end">
      <button class="btn-ghost" onClick=${() => setStep('search')}>${t('migration.action.back')}</button>
      <button class="btn-primary" onClick=${_confirmMigration}>${t('migration.action.migrate')}</button>
    </div>
  `;

  const footerDone = step === 'done' && html`
    <div class="flex justify-end">
      <button class="btn-primary" onClick=${() => onComplete(targetSid, targetMid)}>
        ${t('migration.action.go_to_new')}
      </button>
    </div>
  `;

  const footerSearch = step === 'search' && html`
    <button type="button" class="btn-ghost btn-sm" onClick=${onClose}>${t('common.cancel')}</button>
  `;

  return html`
    <${Modal} open=${true} onClose=${onClose} title=${t('migration.title')} wide=${true}
      footer=${footer || footerDone || footerSearch || undefined}>

      ${step === 'search' && html`
        <div class="flex flex-col gap-3 mb-4">
          <p class="text-sm text-text-muted">
            ${t('migration.intro', { title: currentTitle, source: currentSourceName })}
          </p>
          <label class="flex flex-col gap-1.5">
            <span class="text-sm font-medium text-text">${t('migration.search.label')}</span>
            <input
              type="search"
              class="input"
              placeholder=${t('migration.search.placeholder')}
              value=${query}
              onInput=${(e) => setQuery(/** @type {HTMLInputElement} */ (e.target).value)}
            />
          </label>
          <div class="flex flex-wrap gap-2">
            ${(['FavouritedOnly', 'AllEnabled']).map(s => html`
              <button
                key=${s}
                type="button"
                class=${scope === s ? 'chip chip-active' : 'chip'}
                onClick=${() => setScope(/** @type {any} */ (s))}
              >${s === 'FavouritedOnly' ? t('migration.scope.favourites') : t('migration.scope.all_enabled')}</button>
            `)}
          </div>
        </div>

        ${searching && html`<p class="text-sm text-text-muted py-2">${t('migration.search.searching')}</p>`}
        ${searchError && html`
          <${Callout} tone="danger">${searchError}</${Callout}>
        `}
        ${!searching && !query.trim() && html`
          <${EmptyState} compact=${true}
            title=${t('migration.search.prequery')}
            subtitle=${t('migration.search.prequery.desc')} />
        `}
        ${!searching && !searchError && searchResults.length === 0 && query.trim() && html`
          <${EmptyState} compact=${true}
            title=${t('migration.search.no_results')}
            subtitle=${t('migration.search.no_results.desc')} />
        `}

        ${[...bySource.entries()].map(([sid, { sourceName, sourceId, items }]) => html`
          <div key=${sid} class="flex items-center gap-2 py-2 text-sm font-semibold text-text-muted border-t border-border-subtle mt-2">
            <span>${sourceName}</span>
          </div>
          ${items.length === 0
            ? html`<p class="text-sm text-text-muted px-1">${t('migration.search.no_source_results')}</p>`
            : html`
              <div class="manga-row" role="list">
                ${items.map(item => html`
                  <div
                    key=${item.id}
                    role="button"
                    tabindex="0"
                    class=${'manga-card manga-row__item cursor-pointer focus-visible:ring-2 focus-visible:ring-accent focus-visible:outline-none' + (sourceId === currentSourceId ? ' ring-2 ring-accent' : '')}
                    onClick=${() => _selectTarget(sourceId, item.id, item.title, item.cover_url ?? null)}
                    onKeyDown=${(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); _selectTarget(sourceId, item.id, item.title, item.cover_url ?? null); } }}
                  >
                    <div class="cover">
                      ${item.cover_url
                        ? html`<img src=${item.cover_url} alt=${item.title} loading="lazy" />`
                        : html`<div class="no-cover">${t('migration.preview.no_cover')}</div>`
                      }
                    </div>
                    <p class="title"><span>${item.title}</span></p>
                  </div>
                `)}
              </div>
            `}
        `)}
      `}

      ${step === 'previewing' && html`
        <div class="flex items-center justify-center gap-6 py-4">
          <div class="flex flex-col items-center gap-2 w-32">
            <strong class="text-xs font-semibold text-text-muted text-center">${currentSourceName}</strong>
            <div class="cover">
              ${currentCoverUrl
                ? html`<img src=${currentCoverUrl} alt=${currentTitle} />`
                : html`<div class="no-cover">${t('common.no_cover')}</div>`}
            </div>
            <span class="text-xs text-text text-center line-clamp-2">${currentTitle}</span>
          </div>
          <span class="text-text-muted icon-lg shrink-0"><${Icon} svg=${iconChevronRight} /></span>
          <div class="flex flex-col items-center gap-2 w-32">
            <strong class="text-xs font-semibold text-text-muted text-center">${t('migration.preview.loading')}</strong>
            <div class="skeleton h-40 w-full rounded-md"></div>
            <div class="skeleton h-3 w-24 rounded"></div>
          </div>
        </div>
      `}

      ${step === 'preview' && preview && html`
        <div class="flex items-center justify-center gap-6 py-4">
          <div class="flex flex-col items-center gap-2 w-32">
            <strong class="text-xs font-semibold text-text-muted text-center">${currentSourceName}</strong>
            <div class="cover">
              ${currentCoverUrl
                ? html`<img src=${currentCoverUrl} alt=${currentTitle} />`
                : html`<div class="no-cover">${t('common.no_cover')}</div>`}
            </div>
            <span class="text-xs text-text text-center line-clamp-2">${currentTitle}</span>
          </div>
          <span class="text-text-muted icon-lg shrink-0"><${Icon} svg=${iconChevronRight} /></span>
          <div class="flex flex-col items-center gap-2 w-32">
            <strong class="text-xs font-semibold text-text-muted text-center">${targetTitle}</strong>
            <div class="cover">
              ${targetCoverUrl
                ? html`<img src=${targetCoverUrl} alt=${targetTitle} />`
                : html`<div class="no-cover">${t('common.no_cover')}</div>`}
            </div>
            <span class="text-xs text-text text-center line-clamp-2">${targetTitle}</span>
          </div>
        </div>

        <div class="flex flex-col border border-border rounded-lg overflow-hidden mt-4">
          ${[
            { get label() { return t('migration.preview.chapters_matched'); }, value: preview.chapters_matched, cls: '' },
            { get label() { return t('migration.preview.chapters_new'); }, value: preview.chapters_new, cls: preview.chapters_new > 0 ? 'text-success bg-success/5' : '' },
            { get label() { return t('migration.preview.chapters_orphaned'); }, value: preview.chapters_orphaned, cls: '' },
            { get label() { return t('migration.preview.downloaded_at_risk'); }, value: preview.downloaded_chapters_at_risk, cls: preview.downloaded_chapters_at_risk > 0 ? 'text-warn bg-warn/5' : '' },
          ].map(({ label, value, cls }) => html`
            <div key=${label} class=${'flex items-center justify-between px-4 py-2 border-b border-border-subtle last:border-b-0 text-sm ' + cls}>
              <span class="text-text-muted">${label}</span>
              <span class="font-semibold text-text">${value ?? 0}</span>
            </div>
          `)}
        </div>

        ${preview.downloaded_chapters_at_risk > 0 && html`
          <div class="mt-4 p-3 rounded-lg bg-warn/10 border border-warn/30 text-sm text-warn">
            ${t('migration.preview.orphan_warning', { count: preview.downloaded_chapters_at_risk })}
            <div class="flex items-center gap-2 mt-2">
              <label class="kani-toggle">
                <input
                    type="checkbox"
                    class="kani-toggle__input"
                    checked=${keepOrphaned}
                    onChange=${(e) => setKeepOrphaned(/** @type {HTMLInputElement} */ (e.target).checked)}
                />
                <span class="kani-toggle__track"></span>
              </label>
              ${t('migration.preview.keep_downloaded')}
            </div>
          </div>
        `}
        ${error && html`<p class="text-sm text-danger mt-2">${error}</p>`}
      `}

      ${step === 'confirming' && html`<p class="text-sm text-text-muted py-2">${t('migration.step.confirming')}</p>`}

      ${step === 'done' && result && html`
        <div class="flex flex-col gap-4">
          <p class="text-sm text-success font-medium">${t('migration.done.success')}</p>
          <div class="flex flex-col border border-border rounded-lg overflow-hidden">
            <div class="flex items-center justify-between px-4 py-2 border-b border-border-subtle text-sm">
              <span class="text-text-muted">${t('migration.done.chapters_migrated')}</span>
              <span class="font-semibold text-text">${result.chapters_matched ?? 0}</span>
            </div>
            <div class="flex items-center justify-between px-4 py-2 text-sm">
              <span class="text-text-muted">${t('migration.done.orphaned')}</span>
              <span class="font-semibold text-text">${result.chapters_orphaned ?? 0}</span>
            </div>
          </div>
        </div>
      `}

    <//>
  `;
}

/**
 * Mount the migration dialogue into #modal-root and return an unmount function.
 * @param {ConstructorParameters<typeof MigrationDialogue>[0]} props
 * @returns {() => void}
 */
export function mountMigrationDialogue(props) {
  const root = document.getElementById('modal-root');
  if (!root) return () => {};
  render(html`<${MigrationDialogue} ...${props} onClose=${() => { render(null, root); props.onClose?.(); }} />`, root);
  return () => render(null, root);
}
