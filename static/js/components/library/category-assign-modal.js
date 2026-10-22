// @ts-check

import { h } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { Modal, mountIntoModalRoot } from '../modal.js';
import { Checkbox } from '../form/checkbox.js';
import { EmptyState } from '../empty-state.js';
import { showToast, showApiError } from '../toast.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/**
 * @param {{
 *   mangaIds: number[],
 *   onApplied: (appliedCount: number) => void,
 *   onClose: () => void,
 * }} props
 */
function CategoryAssignModal({ mangaIds, onApplied, onClose }) {
  const [cats, setCats] = useState(/** @type {Array<{id:number,name:string}>|null} */ (null));
  const [selected, setSelected] = useState(/** @type {Set<number>} */ (new Set()));
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState(false);

  useEffect(() => {
    api.getCategories()
      .then(list => setCats(Array.isArray(list) ? list : []))
      .catch(() => { setLoadError(true); setCats([]); });
  }, []);

  function _toggle(/** @type {number} */ id, /** @type {boolean} */ on) {
    setSelected(prev => {
      const next = new Set(prev);
      if (on) next.add(id); else next.delete(id);
      return next;
    });
  }

  async function _apply() {
    setBusy(true);
    const catIds = [...selected];
    let done = 0;
    try {
      for (const id of mangaIds) {
        try { await api.setMangaCategories(id, catIds); done++; } catch { }
      }
      showToast(t('library.categories.toast', { count: done }));
      onApplied(done);
      onClose();
    } catch (e) {
      showApiError(e);
    } finally {
      setBusy(false);
    }
  }

  const hasCats = (cats?.length ?? 0) > 0;

  return html`
    <${Modal}
      open=${true}
      title=${t('library.categories.title')}
      onClose=${onClose}
      footer=${hasCats && html`
        <button type="button" class="btn-ghost btn-sm" disabled=${busy} onClick=${onClose}>${t('common.cancel')}</button>
        <button type="button" class="btn-primary btn-sm" disabled=${busy} onClick=${_apply}>
          ${busy ? t('library.categories.applying') : t('library.categories.apply')}
        </button>
      `}
    >
      ${cats === null && html`<p class="text-sm text-text-muted">${t('common.loading')}</p>`}
      ${cats !== null && !hasCats && html`
        <${EmptyState} compact=${true}
          title=${loadError ? t('library.categories.load_failed') : t('library.categories.empty')} />
      `}
      ${hasCats && html`
        <div class="flex flex-col gap-1">
          <p class="text-xs text-text-muted mb-2">${t('library.categories.applies_to', { count: mangaIds.length })}</p>
          ${/** @type {Array<{id:number,name:string}>} */ (cats).map(c => html`
            <${Checkbox}
              key=${c.id}
              label=${c.name}
              checked=${selected.has(c.id)}
              disabled=${busy}
              onChange=${(/** @type {boolean} */ on) => _toggle(c.id, on)}
              class="py-1"
            />
          `)}
        </div>
      `}
    </${Modal}>
  `;
}

/**
 * @param {number[]} mangaIds
 * @param {{ onApplied: (appliedCount: number) => void }} opts
 */
export function showCategoryAssignModal(mangaIds, { onApplied }) {
  let cleanup = () => {};
  cleanup = mountIntoModalRoot(html`
    <${CategoryAssignModal}
      mangaIds=${mangaIds}
      onApplied=${onApplied}
      onClose=${() => cleanup()}
    />
  `);
}
