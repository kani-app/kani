
import { h, render } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { t } from '../../i18n.js';
import { showToast, showApiError } from '../toast.js';
import { Modal, mountIntoModalRoot, showConfirm } from '../modal.js';
import { Select } from '../form/select.js';
import { Icon } from '../icon.js';
import { iconSettings, iconTrash } from '../../icons.js';

const html = htm.bind(h);

/**
 * @typedef {{ getCurrentFilters: () => Record<string, any>, onApply: (queryJson: string) => void }} SavedSearchesOpts
 */

/** @param {{ opts: SavedSearchesOpts }} props */
function SavedSearches({ opts }) {
  const [searches, setSearches] = useState(/** @type {any[]} */ ([]));

  const reload = useCallback(() => {
    api.listSavedSearches().then(list => setSearches(Array.isArray(list) ? list : [])).catch(() => {});
  }, []);
  useEffect(reload, [reload]);

  function _openSave() {
    let cleanup = () => {};
    cleanup = mountIntoModalRoot(html`
      <${SaveSearchModal} opts=${opts} onSaved=${reload} onClose=${() => cleanup()} />
    `);
  }

  function _openManage() {
    let cleanup = () => {};
    cleanup = mountIntoModalRoot(html`
      <${ManageSearchesModal} searches=${searches} onChanged=${reload} onClose=${() => cleanup()} />
    `);
  }

  return html`
    <button type="button" class="btn-secondary btn-sm whitespace-nowrap" onClick=${_openSave}>
      ${t('saved_searches.save')}
    </button>
    ${searches.length > 0 && html`
      <${Select}
        options=${[{ value: '', label: t('saved_searches.label') },
          ...searches.map(s => ({ value: String(s.id), label: s.name }))]}
        value=${''}
        ariaLabel=${t('saved_searches.label')}
        onChange=${(/** @type {string} */ v) => {
          const search = searches.find(s => String(s.id) === v);
          if (search) opts.onApply(search.query_json);
        }}
      />
      <button type="button" class="btn-icon text-text-muted shrink-0"
        aria-label=${t('saved_searches.manage')} data-tooltip=${t('saved_searches.manage')}
        onClick=${_openManage}>
        <${Icon} svg=${iconSettings} />
      </button>
    `}
  `;
}

/** @param {{ opts: SavedSearchesOpts, onSaved: () => void, onClose: () => void }} props */
function SaveSearchModal({ opts, onSaved, onClose }) {
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);

  async function _save() {
    const trimmed = name.trim();
    if (!trimmed) return;
    setBusy(true);
    try {
      await api.createSavedSearch({ name: trimmed, query_json: JSON.stringify(opts.getCurrentFilters()) });
      showToast(t('saved_searches.toast.saved'), { type: 'success' });
      onSaved();
      onClose();
    } catch (e) {
      showApiError(e);
      setBusy(false);
    }
  }

  return html`
    <${Modal}
      open=${true}
      title=${t('saved_searches.save')}
      onClose=${onClose}
      footer=${html`
        <button type="button" class="btn-ghost btn-sm" disabled=${busy} onClick=${onClose}>${t('common.cancel')}</button>
        <button type="button" class="btn-primary btn-sm" disabled=${busy || !name.trim()} onClick=${_save}>
          ${t('common.save')}
        </button>
      `}
    >
      <input
        type="text"
        class="input w-full text-sm"
        placeholder=${t('saved_searches.name.placeholder')}
        value=${name}
        onInput=${(/** @type {any} */ e) => setName(e.target.value)}
        onKeyDown=${(/** @type {KeyboardEvent} */ e) => { if (e.key === 'Enter') _save(); }}
      />
    </${Modal}>
  `;
}

/** @param {{ searches: any[], onChanged: () => void, onClose: () => void }} props */
function ManageSearchesModal({ searches, onChanged, onClose }) {
  const [items, setItems] = useState(searches);
  const [busyId, setBusyId] = useState(/** @type {number|null} */ (null));

  async function _rename(/** @type {any} */ s) {
    const name = prompt(t('saved_searches.rename.prompt'), s.name);
    const trimmed = name?.trim();
    if (!trimmed || trimmed === s.name) return;
    setBusyId(s.id);
    try {
      // The endpoint takes the whole body, so the query has to be resent
      // unchanged or a rename would blank it.
      await api.updateSavedSearch(s.id, { name: trimmed, query_json: s.query_json });
      showToast(t('saved_searches.toast.renamed'), { type: 'success' });
      setItems(prev => prev.map(x => (x.id === s.id ? { ...x, name: trimmed } : x)));
      onChanged();
    } catch (e) {
      showApiError(e);
    } finally {
      setBusyId(null);
    }
  }

  async function _delete(/** @type {any} */ s) {
    const ok = await showConfirm(t('saved_searches.delete.confirm', { name: s.name }),
      { title: t('common.delete'), confirmLabel: t('common.delete'), danger: true });
    if (!ok) return;
    setBusyId(s.id);
    try {
      await api.deleteSavedSearch(s.id);
      showToast(t('saved_searches.toast.deleted'), { type: 'success' });
      setItems(prev => prev.filter(x => x.id !== s.id));
      onChanged();
    } catch (e) {
      showApiError(e);
    } finally {
      setBusyId(null);
    }
  }

  return html`
    <${Modal} open=${true} title=${t('saved_searches.manage.title')} onClose=${onClose}>
      ${items.length === 0
        ? html`<p class="text-sm text-text-muted text-center py-2">${t('saved_searches.empty.title')}</p>`
        : html`
          <div class="divide-y divide-border-subtle -my-2">
            ${items.map(s => html`
              <div key=${s.id} class="flex items-center gap-3 py-2.5">
                <span class="flex-1 text-sm text-text truncate">${s.name}</span>
                <button type="button" class="btn-ghost btn-sm shrink-0"
                  disabled=${busyId === s.id} onClick=${() => _rename(s)}>
                  ${t('saved_searches.rename')}
                </button>
                <button type="button" class="btn-icon text-danger shrink-0"
                  aria-label=${t('common.delete')} disabled=${busyId === s.id}
                  onClick=${() => _delete(s)}>
                  <${Icon} svg=${iconTrash} />
                </button>
              </div>
            `)}
          </div>
        `}
    </${Modal}>
  `;
}

/**
 * @param {HTMLElement} el
 * @param {SavedSearchesOpts} opts
 */
export function mountSavedSearches(el, opts) {
  render(html`<${SavedSearches} opts=${opts} />`, el);
}
