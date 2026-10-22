// @ts-check

import { escapeHtml } from '../utils.js';

/**
 * @param {{
 *   tags: string[],
 *   getHref?: (tag: string) => string | null,
 * }} props
 * @returns {HTMLElement}
 */
export function createTagList({ tags, getHref }) {
  const wrap = document.createElement('div');
  wrap.className = 'flex flex-wrap gap-1.5';

  for (const tag of tags) {
    const el = document.createElement('span');
    el.className = 'inline-flex items-center px-2 py-0.5 text-xs rounded-sm border border-border text-text-muted';

    const href = getHref?.(tag);
    if (href) {
      el.innerHTML = `<a class="hover:text-text transition-colors" href="${escapeHtml(href)}">${escapeHtml(tag)}</a>`;
    } else {
      el.textContent = tag;
    }

    wrap.appendChild(el);
  }

  return wrap;
}
