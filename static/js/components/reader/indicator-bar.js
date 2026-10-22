// @ts-check
import { h, render, Fragment } from 'preact';
import { signal, computed, effect } from '@preact/signals';

function segColor(i, currentPage, loaded, failed) {
  if (failed.has(i))     return 'bg-danger/70';
  if (i === currentPage) return 'bg-accent';
  if (i < currentPage)   return 'bg-accent/40';
  if (loaded.has(i))     return 'seg-loaded';
  return 'seg-unloaded';
}

/**
 * @param {{
 *   miniStrip: HTMLElement,
 *   segsEl: HTMLElement,
 *   segLeft: HTMLElement,
 *   segRight: HTMLElement,
 *   onSegClick: (i: number, e: MouseEvent) => void,
 * }} deps
 */
export function createIndicatorBar({ miniStrip, segsEl, segLeft, segRight, onSegClick }) {
  const currentPage = signal(0);
  const total       = signal(0);
  const loaded      = signal(/** @type {Set<number>} */ (new Set()));
  const failed      = signal(/** @type {Set<number>} */ (new Set()));

  /** @type {Array<() => void>} */
  const _disposers = [];

  _disposers.push(effect(() => {
    if (total.value === 0) {
      segLeft.textContent  = '—';
      segRight.textContent = '—';
    } else {
      segLeft.textContent  = String(currentPage.value + 1);
      segRight.textContent = String(total.value);
    }
  }));

  function _rebuild() {
    const t = total.peek();
    if (t === 0) {
      render(null, miniStrip);
      render(null, segsEl);
      return;
    }
    const miniSegs = [];
    const fullSegs = [];
    for (let i = 0; i < t; i++) {
      const idx = i;
      const miniClass = computed(() => `flex-1 h-full ${segColor(idx, currentPage.value, loaded.value, failed.value)}`);
      const fullClass = computed(() => `flex-1 h-full rounded cursor-pointer ${segColor(idx, currentPage.value, loaded.value, failed.value)}`);
      miniSegs.push(h('div', { class: miniClass }));
      fullSegs.push(h('div', { class: fullClass, onClick: (/** @type {MouseEvent} */ e) => onSegClick(idx, e) }));
    }
    render(h(Fragment, null, miniSegs), miniStrip);
    render(h(Fragment, null, fullSegs), segsEl);
  }

  /** @param {{ total: number, currentPage: number, loaded: Set<number>, failed: Set<number> }} props */
  function update({ total: t, currentPage: cp, loaded: l, failed: f }) {
    currentPage.value = cp;
    loaded.value = l instanceof Set ? new Set(l) : new Set();
    failed.value = f instanceof Set ? new Set(f) : new Set();
    if (t !== total.peek()) {
      total.value = t;
      _rebuild();
    }
  }

  function destroy() {
    for (const dispose of _disposers) dispose();
    _disposers.length = 0;
    render(null, miniStrip);
    render(null, segsEl);
  }

  return { update, destroy };
}
