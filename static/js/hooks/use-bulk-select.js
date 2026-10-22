// @ts-check
import { useState, useCallback } from 'preact/hooks';

/**
 * Set-backed bulk-selection state for list UIs.
 * @template {string|number} T
 * @param {T[]} allIds - all selectable ids; used for toggleAll and headerState
 * @returns {{
 *   selected: Set<T>,
 *   toggle: (id: T) => void,
 *   toggleAll: () => void,
 *   clear: () => void,
 *   isSelected: (id: T) => boolean,
 *   count: number,
 *   headerState: 'unchecked' | 'indeterminate' | 'checked',
 * }}
 */
export function useBulkSelect(allIds) {
  const [selected, setSelected] = useState(/** @type {Set<T>} */ (new Set()));

  const toggle = useCallback((/** @type {T} */ id) => {
    setSelected(prev => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  }, []);

  const toggleAll = useCallback(() => {
    setSelected(prev =>
      prev.size === allIds.length ? new Set() : new Set(allIds)
    );
  }, [allIds]);

  const clear = useCallback(() => setSelected(new Set()), []);

  const isSelected = useCallback((/** @type {T} */ id) => selected.has(id), [selected]);

  const headerState =
    allIds.length === 0 || selected.size === 0 ? 'unchecked' :
    selected.size === allIds.length ? 'checked' :
    'indeterminate';

  return { selected, toggle, toggleAll, clear, isSelected, count: selected.size, headerState };
}
