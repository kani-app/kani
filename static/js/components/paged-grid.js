
import { skeletonGrid, skeletonGridItems } from './skeletons.js';
import { createEmptyState } from './empty-state.js';
import { createErrorState } from './error-state.js';
import { startLoading, finishLoading } from './page-loading-bar.js';

/**
 * @typedef {{
 *   gridEl: HTMLElement,
 *   pageSize: number,
 *   append?: boolean,
 *   fetchPage: () => Promise<any>,
 *   mapItems: (result: any) => any[],
 *   renderCard: (item: any) => HTMLElement,
 *   emptyIcon?: string,
 *   emptyTitle: string,
 *   errorMessage: string,
 *   onError?: (error: any, gridEl: HTMLElement) => boolean,
 *   onRetry?: () => void,
 * }} PagedGridOptions
 */

/**
 * Runs one fetch-render cycle for a manga grid. `opts.fetchPage` must close
 * over the caller's own AbortSignal so a stale response throws AbortError
 * the same way it always has.
 *
 * Returns null on AbortError — the caller must not touch its pagination
 * element in that case, since a newer fetch for the same grid is already
 * in flight and may have already started painting its own loading state;
 * clearing it here would race and wipe that out.
 *
 * Returns `{ error: true }` when a non-abort error was caught and handled
 * (an error state was rendered into gridEl, or onError handled it) — the
 * caller should still reset its own pagination UI in this case.
 *
 * Otherwise returns the fetch result plus the mapped item list so the
 * caller can wire pagination or an infinite-scroll sentinel.
 * @param {PagedGridOptions} opts
 * @returns {Promise<{ result: any, items: any[] } | { error: true } | null>}
 */
export async function fetchPagedGrid(opts) {
  const { gridEl, pageSize, append = false, fetchPage, mapItems, renderCard, emptyIcon, emptyTitle, errorMessage, onError, onRetry } = opts;

  /** @type {Element[]} */
  let placeholders = [];
  if (append) {
    const grid = gridEl.querySelector('.manga-grid');
    if (grid) {
      const holder = document.createElement('div');
      holder.innerHTML = skeletonGridItems(pageSize);
      placeholders = [...holder.children];
      grid.append(...placeholders);
    }
    gridEl.setAttribute('aria-busy', 'true');
  } else {
    gridEl.innerHTML = skeletonGrid(pageSize);
    gridEl.setAttribute('aria-busy', 'true');
    gridEl.classList.add('opacity-50', 'pointer-events-none');
  }
  startLoading();

  let result;
  try {
    result = await fetchPage();
  } catch (e) {
    for (const p of placeholders) p.remove();
    if (/** @type {any} */ (e)?.name === 'AbortError') return null;
    gridEl.setAttribute('aria-busy', 'false');
    if (!append) {
      gridEl.innerHTML = '';
      gridEl.classList.remove('opacity-50', 'pointer-events-none');
    }
    finishLoading();
    if (!onError?.(e, gridEl)) {
      gridEl.appendChild(createErrorState({ message: errorMessage, onRetry }));
    }
    return { error: true };
  }

  finishLoading();
  for (const p of placeholders) p.remove();
  gridEl.setAttribute('aria-busy', 'false');
  if (!append) {
    gridEl.innerHTML = '';
    gridEl.classList.remove('opacity-50', 'pointer-events-none');
  }

  const items = mapItems(result);

  if (items.length === 0 && !append) {
    gridEl.appendChild(createEmptyState({ icon: emptyIcon, title: emptyTitle }));
  } else if (items.length > 0) {
    let grid = append ? /** @type {HTMLElement|null} */ (gridEl.querySelector('.manga-grid')) : null;
    if (!grid) {
      grid = document.createElement('div');
      grid.className = 'manga-grid';
      gridEl.appendChild(grid);
    }
    for (const item of items) grid.appendChild(renderCard(item));
  }

  return { result, items };
}
