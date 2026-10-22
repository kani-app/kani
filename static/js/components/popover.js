
import { render } from 'preact';
import { useEffect } from 'preact/hooks';

/**
 * Render a vnode into #popover-root. Pass null to clear.
 * The root is shared: at most one popover is open at a time by design.
 * @param {any} vnode
 */
export function renderPopover(vnode) {
  const root = document.getElementById('popover-root');
  if (root) render(vnode, root);
}

/**
 * Closes an open popover on mousedown outside all of `refs` (typically the
 * trigger and the panel). Targets already detached from the document are
 * ignored — a re-render inside the panel must not count as "outside".
 *
 * @param {boolean} open
 * @param {Array<{ current: Node | null }>} refs
 * @param {() => void} onClose
 */
export function useOutsideClose(open, refs, onClose) {
  useEffect(() => {
    if (!open) return;
    const handler = (/** @type {MouseEvent} */ e) => {
      const target = /** @type {Node} */ (e.target);
      if (!document.contains(target)) return;
      if (refs.every(r => !r.current || !r.current.contains(target))) onClose();
    };
    const keyHandler = (/** @type {KeyboardEvent} */ e) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', handler);
    document.addEventListener('keydown', keyHandler);
    return () => {
      document.removeEventListener('mousedown', handler);
      document.removeEventListener('keydown', keyHandler);
    };
  }, [open]);
}
