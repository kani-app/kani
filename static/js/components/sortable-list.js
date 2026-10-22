// @ts-check

import { iconDragHandle } from '../icons.js';

/**
 * @template T
 * @param {HTMLElement} container
 * @param {{
 *   items: T[],
 *   getId: (item: T) => string | number,
 *   renderItem: (item: T, index: number) => HTMLElement,
 *   onReorder: (ids: Array<string | number>, items: T[]) => void | Promise<void>,
 *   className?: string,
 * }} opts
 * @returns {{ update: (items: T[]) => void, destroy: () => void }}
 */
export function mountSortableList(container, opts) {
  let items = [...opts.items];

  /** @type {HTMLElement | null} */
  let dragSrc = null;
  /** @type {HTMLUListElement | null} */
  let ul = null;
  /** @type {number | null} — index of the item grabbed by keyboard */
  let _kbGrabIdx = null;

  /** Announce a message to screen readers via aria-live. */
  function _announce(msg) {
    const el = document.createElement('span');
    el.className = 'sr-only';
    el.setAttribute('aria-live', 'assertive');
    el.setAttribute('aria-atomic', 'true');
    el.textContent = msg;
    container.appendChild(el);
    setTimeout(() => el.remove(), 1000);
  }

  function _commit() {
    const ids = items.map(it => opts.getId(it));
    opts.onReorder(ids, [...items]);
  }

  function _render() {
    container.innerHTML = '';
    ul = document.createElement('ul');
    ul.className = opts.className ?? 'flex flex-col divide-y divide-border-subtle';
    ul.setAttribute('role', 'listbox');
    ul.setAttribute('aria-label', 'Reorderable list');

    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      const id = String(opts.getId(item));
      const isGrabbed = _kbGrabIdx === i;

      const li = document.createElement('li');
      li.className = 'flex items-center gap-3 py-2 px-2 hover:bg-surface-2 cursor-grab active:cursor-grabbing transition-colors'
        + (isGrabbed ? ' ring-2 ring-accent' : '');
      li.draggable = true;
      li.dataset.sortId = id;
      li.dataset.idx = String(i);
      li.setAttribute('role', 'option');
      li.setAttribute('aria-selected', 'false');

      const grip = document.createElement('span');
      grip.className = 'text-text-muted shrink-0 cursor-grab select-none icon-sm';
      grip.setAttribute('tabindex', '0');
      grip.setAttribute('aria-label', 'Drag to reorder');
      grip.setAttribute('role', 'button');
      grip.innerHTML = iconDragHandle;

      grip.addEventListener('keydown', (e) => {
        const idx = Number(li.dataset.idx);
        if (e.key === ' ' || e.key === 'Enter') {
          e.preventDefault();
          if (_kbGrabIdx === null) {
            _kbGrabIdx = idx;
            _announce('Grabbed. Use arrow keys to move, Space or Enter to drop, Escape to cancel.');
          } else {
            _kbGrabIdx = null;
            _announce('Dropped.');
            _commit();
          }
          _render();
          const grips = ul?.querySelectorAll('[role="button"]');
          /** @type {HTMLElement|undefined} */ (grips?.[Math.min(idx, items.length - 1)])?.focus();
        } else if (e.key === 'Escape') {
          if (_kbGrabIdx !== null) {
            e.preventDefault();
            _kbGrabIdx = null;
            _announce('Cancelled.');
            _render();
          }
        } else if (_kbGrabIdx !== null && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
          e.preventDefault();
          const delta = e.key === 'ArrowUp' ? -1 : 1;
          const from = _kbGrabIdx;
          const to = from + delta;
          if (to < 0 || to >= items.length) return;
          const [moved] = items.splice(from, 1);
          items.splice(to, 0, moved);
          _kbGrabIdx = to;
          _announce(`Moved to position ${to + 1} of ${items.length}.`);
          _render();
          const grips = ul?.querySelectorAll('[role="button"]');
          /** @type {HTMLElement|undefined} */ (grips?.[to])?.focus();
        }
      });

      const content = opts.renderItem(item, i);
      li.appendChild(grip);
      li.appendChild(content);

      li.addEventListener('dragstart', (e) => {
        dragSrc = li;
        e.dataTransfer?.setData('text/plain', id);
        li.classList.add('opacity-50');
      });
      li.addEventListener('dragend', () => {
        dragSrc = null;
        li.classList.remove('opacity-50');
        ul?.querySelectorAll('li[data-sort-id]').forEach(el => el.classList.remove('border-t-2', 'border-t-accent'));
      });
      li.addEventListener('dragover', (e) => {
        e.preventDefault();
        if (dragSrc && dragSrc !== li) {
          ul?.querySelectorAll('li[data-sort-id]').forEach(el => el.classList.remove('border-t-2', 'border-t-accent'));
          li.classList.add('border-t-2', 'border-t-accent');
        }
      });
      li.addEventListener('drop', (e) => {
        e.preventDefault();
        if (!dragSrc || dragSrc === li) return;
        const srcId = dragSrc.dataset.sortId;
        const tgtId = li.dataset.sortId;
        const srcIdx = items.findIndex(it => String(opts.getId(it)) === srcId);
        const tgtIdx = items.findIndex(it => String(opts.getId(it)) === tgtId);
        if (srcIdx < 0 || tgtIdx < 0) return;
        const [moved] = items.splice(srcIdx, 1);
        items.splice(tgtIdx, 0, moved);
        _render();
        _commit();
      });

      ul.appendChild(li);
    }

    container.appendChild(ul);
  }

  _render();

  return {
    update(newItems) {
      items = [...newItems];
      _render();
    },
    destroy() {
      container.innerHTML = '';
      ul = null;
    },
  };
}
