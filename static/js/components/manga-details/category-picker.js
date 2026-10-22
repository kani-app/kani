// @ts-check

import { h, render } from 'preact';
import { useState } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { t } from '../../i18n.js';
import { EmptyState } from '../empty-state.js';
import { showApiError } from '../toast.js';
const html = htm.bind(h);

/**
 * @param {HTMLElement} bodyEl
 * @param {any[]} allCats
 * @param {any[]} mangaCats
 * @param {number} dbId
 */
export function mountCategoryPicker(bodyEl, allCats, mangaCats, dbId) {
  const mount = document.createElement('div');
  bodyEl.appendChild(mount);
  render(html`<${CategoryPicker} allCats=${allCats} mangaCats=${mangaCats} dbId=${dbId} />`, mount);
}

function CategoryPicker({ allCats, mangaCats, dbId }) {
  const all = Array.isArray(allCats) ? allCats : [];
  const [memberIds, setMemberIds] = useState(
    () => new Set((Array.isArray(mangaCats) ? mangaCats : []).map(c => c.id ?? c))
  );

  if (all.length === 0) {
    return html`<${EmptyState} title=${t('manga.categories.empty')} />`;
  }

  async function handleToggle(catId) {
    const wasIn = memberIds.has(catId);
    const next = new Set(memberIds);
    if (wasIn) next.delete(catId); else next.add(catId);
    setMemberIds(next);
    try {
      await api.setMangaCategories(dbId, [...next]);
    } catch (err) {
      setMemberIds(memberIds);
      showApiError(err);
    }
  }

  return html`
    <div class="flex flex-wrap gap-2 p-1">
      ${all.map(cat => html`
        <button
          key=${cat.id}
          type="button"
          class=${memberIds.has(cat.id) ? 'chip chip-active' : 'chip'}
          aria-pressed=${memberIds.has(cat.id)}
          onClick=${() => handleToggle(cat.id)}
        >${cat.name}</button>
      `)}
    </div>
  `;
}
