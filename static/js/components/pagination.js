// @ts-check

import { h, render } from 'preact';
import htm from 'htm';
import { iconChevronLeft, iconChevronRight } from '../icons.js';

const html = htm.bind(h);

/**
 * @param {{
 *   page: number,
 *   hasNext: boolean,
 *   total?: number,
 *   onPageChange: (page: number) => void,
 * }} props
 */
export function Pagination({ page, hasNext, total, onPageChange }) {
  const prevDisabled = page <= 1;
  const nextDisabled = !hasNext;

  return html`
    <div class="flex flex-wrap items-center justify-center gap-1.5 py-4">
      <button
        type="button"
        class="tile-btn border-border bg-surface text-text-muted icon-sm transition-colors hover:bg-surface-2 disabled:opacity-35 disabled:cursor-not-allowed cursor-pointer"
        disabled=${prevDisabled}
        aria-label="Previous page"
        onClick=${() => onPageChange(page - 1)}
        dangerouslySetInnerHTML=${{ __html: iconChevronLeft }}
      />
      ${total != null && total > 0
        ? _pageWindow(page, total).map((entry, i) =>
            entry === null
              ? html`<span key=${'ellipsis-' + i} class="tile-btn border-0 text-text-muted select-none" aria-hidden="true">…</span>`
              : entry === page
                ? html`<span key=${entry} class="tile-btn border-text/40 bg-surface-2 text-text font-semibold" aria-current="page" aria-label=${'Page ' + entry}>${entry}</span>`
                : html`<button key=${entry} type="button" class="tile-btn border-border bg-surface text-text-muted transition-colors hover:bg-surface-2 cursor-pointer" aria-label=${'Page ' + entry} onClick=${() => onPageChange(entry)}>${entry}</button>`
          )
        : html`<span class="tile-btn border-text/40 bg-surface-2 text-text font-semibold" aria-current="page">${page}</span>`
      }
      <button
        type="button"
        class="tile-btn border-border bg-surface text-text-muted icon-sm transition-colors hover:bg-surface-2 disabled:opacity-35 disabled:cursor-not-allowed cursor-pointer"
        disabled=${nextDisabled}
        aria-label="Next page"
        onClick=${() => onPageChange(page + 1)}
        dangerouslySetInnerHTML=${{ __html: iconChevronRight }}
      />
    </div>
  `;
}

function _pageWindow(current, total) {
  const pinned = new Set(
    [1, total, current - 1, current, current + 1].filter(n => n >= 1 && n <= total),
  );
  const sorted = [...pinned].sort((a, b) => a - b);
  /** @type {Array<number | null>} */
  const result = [];
  for (let i = 0; i < sorted.length; i++) {
    if (i > 0 && sorted[i] - sorted[i - 1] > 1) result.push(null);
    result.push(sorted[i]);
  }
  return result;
}

/**
 * @param {HTMLElement} container
 * @param {{
 *   page: number,
 *   hasNext: boolean,
 *   total?: number,
 *   onPageChange: (page: number) => void,
 * }} props
 * @returns {{ update: (props: { page?: number, hasNext?: boolean, total?: number }) => void, destroy: () => void }}
 */
export function renderPagination(container, { page, hasNext, total, onPageChange }) {
  let _props = { page, hasNext, total, onPageChange };
  const _mount = document.createElement('div');
  _mount.style.display = 'contents';
  container.appendChild(_mount);

  function _render() {
    render(html`<${Pagination} ...${_props} />`, _mount);
  }

  _render();

  return {
    update(newProps) {
      _props = { ..._props, ...newProps };
      _render();
    },
    destroy() {
      render(null, _mount);
      _mount.remove();
    },
  };
}
