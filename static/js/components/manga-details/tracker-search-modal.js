// @ts-check
// Tracker search dialog — find the manga on an external tracker and link it.
// Replaces two chained window.prompt() calls (type a query into a browser
// prompt, then type the number of the result you wanted).

import { h } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { Modal, mountIntoModalRoot } from '../modal.js';
import { SearchInput } from '../form/search-input.js';
import { EmptyState } from '../empty-state.js';
import { Callout } from '../form/callout.js';
import { showApiError } from '../toast.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/**
 * @param {{
 *   tracker: { id: number, name: string },
 *   initialQuery?: string,
 *   onLink: (trackerMangaId: string) => Promise<void>,
 *   onClose: () => void,
 * }} props
 */
function TrackerSearchModal({ tracker, initialQuery = '', onLink, onClose }) {
  const [query, setQuery] = useState(initialQuery);
  const [results, setResults] = useState(/** @type {any[]|null} */ (null));
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState(/** @type {string|null} */ (null));
  const [linkingId, setLinkingId] = useState(/** @type {string|null} */ (null));
  const debounceRef = useRef(/** @type {ReturnType<typeof setTimeout>|null} */ (null));

  useEffect(() => {
    if (!query.trim()) { setResults(null); setError(null); return; }
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      setSearching(true);
      setError(null);
      try {
        const res = await api.searchTrackerManga(tracker.id, query.trim());
        setResults(Array.isArray(res) ? res : []);
      } catch (e) {
        setError(/** @type {any} */ (e)?.message ?? String(e));
        setResults([]);
      } finally {
        setSearching(false);
      }
    }, 450);
    return () => { if (debounceRef.current) clearTimeout(debounceRef.current); };
  }, [query, tracker.id]);

  async function _link(/** @type {any} */ r) {
    setLinkingId(r.tracker_manga_id);
    try {
      await onLink(r.tracker_manga_id);
      onClose();
    } catch (e) {
      showApiError(e);
      setLinkingId(null);
    }
  }

  return html`
    <${Modal}
      open=${true}
      title=${t('manga.tracker.search.modal_title', { name: tracker.name })}
      onClose=${onClose}
      footer=${html`<button type="button" class="btn-ghost btn-sm" onClick=${onClose}>${t('common.cancel')}</button>`}
    >
      <div class="flex flex-col gap-3">
        <${SearchInput}
          value=${query}
          onInput=${setQuery}
          placeholder=${t('manga.tracker.search.placeholder')}
          ariaLabel=${t('manga.tracker.search.placeholder')}
        />

        ${searching && html`<p class="text-sm text-text-muted">${t('migration.search.searching')}</p>`}
        ${error && html`<${Callout} tone="danger">${error}</${Callout}>`}
        ${!searching && !error && results === null && html`
          <${EmptyState} compact=${true} title=${t('manga.tracker.search.prequery')} />
        `}
        ${!searching && !error && results !== null && results.length === 0 && html`
          <${EmptyState} compact=${true} title=${t('manga.tracker.no_results')} />
        `}
        ${!searching && results !== null && results.length > 0 && html`
          <div class="divide-y divide-border-subtle border border-border-subtle rounded-lg overflow-y-auto max-h-72">
            ${results.map(r => html`
              <button
                key=${r.tracker_manga_id}
                type="button"
                class="w-full flex items-center gap-3 px-3 py-2.5 text-left hover:bg-surface-2 focus-visible:outline-none focus-visible:bg-surface-2 disabled:opacity-50"
                disabled=${linkingId !== null}
                onClick=${() => _link(r)}
              >
                <span class="flex-1 min-w-0 text-sm text-text truncate">${r.title}</span>
                <span class="text-xs text-text-faint font-mono shrink-0">
                  ${linkingId === r.tracker_manga_id ? t('manga.tracker.linking') : r.tracker_manga_id}
                </span>
              </button>
            `)}
          </div>
        `}
      </div>
    </${Modal}>
  `;
}

/**
 * @param {{ id: number, name: string }} tracker
 * @param {{ initialQuery?: string, onLink: (trackerMangaId: string) => Promise<void> }} opts
 */
export function showTrackerSearchModal(tracker, { initialQuery, onLink }) {
  let cleanup = () => {};
  cleanup = mountIntoModalRoot(html`
    <${TrackerSearchModal}
      tracker=${tracker}
      initialQuery=${initialQuery}
      onLink=${onLink}
      onClose=${() => cleanup()}
    />
  `);
}
