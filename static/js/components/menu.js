// @ts-check

import { h, render } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
const html = htm.bind(h);

/**
 * @typedef {{ label: string, action: () => void, danger?: boolean, disabled?: boolean } | { divider: true }} MenuItem
 */

/**
 * Dropdown menu anchored to a button element, or a context menu at fixed coordinates.
 *
 * @param {{
 *   items: MenuItem[],
 *   trigger: { current: HTMLElement | null } | { x: number, y: number },
 *   onClose: () => void,
 *   id?: string,
 * }} props
 */
export function ContextMenu({ items, trigger, onClose, id }) {
  const menuRef = useRef(/** @type {HTMLDivElement|null} */(null));
  const [pos, setPos] = useState(/** @type {{ top: number|null, bottom: number|null, left: number|null, right: number|null }} */({ top: 0, bottom: null, left: 0, right: null }));
  const [visible, setVisible] = useState(false);

  // Keep a stable ref to onClose so event listeners don't need it in their deps
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;

    const menuW = menu.offsetWidth;
    const menuH = menu.offsetHeight;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const MARGIN = 8;

    let top = /** @type {number|null} */ (null);
    let bottom = /** @type {number|null} */ (null);
    let left = /** @type {number|null} */ (null);
    let right = /** @type {number|null} */ (null);

    if ('current' in trigger) {
      const el = trigger.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      const spaceBelow = vh - rect.bottom;
      if (spaceBelow < menuH + MARGIN && rect.top > menuH + MARGIN) {
        bottom = vh - rect.top + 4;
      } else {
        top = rect.bottom + 4;
      }
      // Align right edge to button's right edge, but keep within viewport
      right = vw - rect.right;
      if (vw - right - menuW < MARGIN) right = vw - menuW - MARGIN;
    } else {
      left = trigger.x;
      top = trigger.y;
      if (left + menuW > vw - MARGIN) left = vw - menuW - MARGIN;
      if (top + menuH > vh - MARGIN) top = vh - menuH - MARGIN;
      if (left < MARGIN) left = MARGIN;
      if (top < MARGIN) top = MARGIN;
    }

    setPos({ top, bottom, left, right });
    setVisible(true);
  }, []);

  useEffect(() => {
    function handleMouseDown(/** @type {MouseEvent} */ e) {
      if (menuRef.current?.contains(/** @type {Node} */(e.target))) return;
      onCloseRef.current();
    }
    function handleScroll() { onCloseRef.current(); }
    function handleKey(/** @type {KeyboardEvent} */ e) { if (e.key === 'Escape') onCloseRef.current(); }

    document.addEventListener('mousedown', handleMouseDown);
    document.addEventListener('scroll', handleScroll, true);
    document.addEventListener('keydown', handleKey);
    return () => {
      document.removeEventListener('mousedown', handleMouseDown);
      document.removeEventListener('scroll', handleScroll, true);
      document.removeEventListener('keydown', handleKey);
    };
  }, []);

  return html`
    <div
      ref=${menuRef}
      id=${id}
      class="min-w-40 max-w-64 rounded-lg bg-surface shadow-lg py-1"
      role="menu"
      style=${{
        position: 'fixed',
        zIndex: 'var(--z-popover)',
        visibility: visible ? 'visible' : 'hidden',
        top: pos.top != null ? pos.top + 'px' : 'auto',
        bottom: pos.bottom != null ? pos.bottom + 'px' : 'auto',
        left: pos.left != null ? pos.left + 'px' : 'auto',
        right: pos.right != null ? pos.right + 'px' : 'auto',
        border: '1px solid var(--color-border)',
      }}
    >
      ${items.map((item, i) => {
        if ('divider' in item) {
          return html`<div key=${'d' + i} class="my-1 border-t border-border-subtle" role="separator" />`;
        }
        return html`
          <button
            key=${i}
            type="button"
            role="menuitem"
            disabled=${!!item.disabled}
            aria-disabled=${item.disabled ? 'true' : undefined}
            class=${[
              'w-full text-left px-4 py-2 text-sm transition-colors',
              item.danger ? 'text-danger hover:bg-danger/10' : 'text-text hover:bg-surface-2',
              item.disabled ? 'opacity-40 cursor-default pointer-events-none' : '',
            ].filter(Boolean).join(' ')}
            onClick=${() => { onCloseRef.current(); item.action(); }}
          >${item.label}</button>
        `;
      })}
    </div>
  `;
}

/**
 * Imperatively show a ContextMenu in a body-appended container, owning the
 * mount/unmount ritual. Returns a close function (idempotent).
 *
 * @param {MenuItem[]} items
 * @param {{ current: HTMLElement | null } | { x: number, y: number }} trigger
 * @returns {() => void}
 */
export function showContextMenu(items, trigger) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  let closed = false;
  const close = () => {
    if (closed) return;
    closed = true;
    render(null, container);
    container.remove();
  };
  render(html`<${ContextMenu} items=${items} trigger=${trigger} onClose=${close} />`, container);
  return close;
}
