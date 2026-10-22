// @ts-check
// Continue-reading shelf — horizontal snap-scroll row of in-progress chapters
// with a per-card read-progress bar and scroll-position-aware edge fades.
// Extracted from pages/library.js.

import { navigate } from '../../router.js';
import { formatChapterTitle } from '../../utils.js';
import { t } from '../../i18n.js';
import { applyCoverQuality } from '../../cover-quality.js';

/**
 * @typedef {{
 *   manga_id: number, manga_name: string,
 *   cover_url?: string | null, local_cover_path?: string | null,
 *   chapter_id: number, chapter_number: number,
 *   last_page: number, page_count: number,
 * }} ShelfItem
 */

/**
 * Mounts the shelf into `container` (initially hidden; shown only when
 * `loadItems` resolves with entries). Returns a destroy fn.
 *
 * @param {HTMLElement} container
 * @param {{ loadItems: () => Promise<ShelfItem[]> }} opts
 * @returns {{ destroy: () => void }}
 */
export function mountContinueShelf(container, { loadItems }) {
  container.classList.add('hidden', 'flex-col', 'gap-2');

  const heading = document.createElement('h2');
  heading.className = 'eyebrow';
  heading.textContent = t('library.continue_reading');
  container.appendChild(heading);

  const row = document.createElement('div');
  row.className = 'shelf-row flex gap-3 overflow-x-auto pb-1';
  container.appendChild(row);

  /** @type {(() => void) | null} */
  let removeFade = null;
  let destroyed = false;

  loadItems().then(items => {
    if (destroyed || !Array.isArray(items) || items.length === 0) return;
    container.classList.remove('hidden');
    container.classList.add('flex');

    for (const item of items) {
      row.appendChild(_mkShelfCard(item));
    }

    const updateFade = () => {
      const atStart = row.scrollLeft <= 2;
      const atEnd = row.scrollLeft + row.clientWidth >= row.scrollWidth - 2;
      row.classList.toggle('is-fade-start', !atStart);
      row.classList.toggle('is-fade-end', !atEnd);
    };
    row.addEventListener('scroll', updateFade, { passive: true });
    removeFade = () => row.removeEventListener('scroll', updateFade);
    requestAnimationFrame(updateFade);
  }).catch(() => { });

  return {
    destroy() {
      destroyed = true;
      removeFade?.();
      removeFade = null;
      container.innerHTML = '';
      container.classList.remove('flex', 'flex-col', 'gap-2');
      container.classList.add('hidden');
    },
  };
}

/** @param {ShelfItem} item */
function _mkShelfCard(item) {
  const card = document.createElement('a');
  card.className = 'shelf-card flex flex-col gap-1 shrink-0 w-24 cursor-pointer';
  card.href = `/reader/${item.chapter_id}`;
  card.addEventListener('click', e => { e.preventDefault(); navigate(`/reader/${item.chapter_id}`); });

  const cover = document.createElement('div');
  cover.className = 'relative w-full aspect-[2/3] rounded bg-surface-2 overflow-hidden';
  const coverSrc = applyCoverQuality(item.local_cover_path
    ? `/rest/manga/${item.manga_id}/cover?size=sm`
    : item.cover_url ?? null);
  if (coverSrc) {
    const img = document.createElement('img');
    img.src = coverSrc;
    img.alt = item.manga_name;
    img.className = 'w-full h-full object-cover';
    img.loading = 'lazy';
    cover.appendChild(img);
  }
  if (item.page_count > 0) {
    const pct = Math.min(100, Math.round(((item.last_page + 1) / item.page_count) * 100));
    const progress = document.createElement('div');
    progress.className = 'shelf-card__progress';
    const fill = document.createElement('div');
    fill.className = 'shelf-card__progress-fill';
    fill.style.width = `${pct}%`;
    progress.appendChild(fill);
    cover.appendChild(progress);
  }
  card.appendChild(cover);

  const title = document.createElement('p');
  title.className = 'text-xs text-text truncate';
  title.textContent = item.manga_name;
  card.appendChild(title);

  const ch = document.createElement('p');
  ch.className = 'text-xs text-text-muted';
  ch.textContent = formatChapterTitle({ chapter_number: item.chapter_number });
  card.appendChild(ch);

  return card;
}
