// @ts-check

/**
 * Shows independently scrolling panes side by side on desktop and exposes
 * `setView` to select the single visible pane on mobile.
 *
 * @param {HTMLElement} el
 * @param {{ listWidth?: number }} [opts]
 * @returns {{ listEl: HTMLElement, detailEl: HTMLElement, setView: (v: 'list'|'detail') => void, destroy: () => void }}
 */
export function mountMasterDetail(el, { listWidth = 300 } = {}) {
  const wrap = document.createElement('div');
  wrap.className = 'master-detail';
  wrap.style.setProperty('--md-list-w', `${listWidth}px`);
  wrap.dataset.view = 'list';

  const listEl = document.createElement('div');
  listEl.className = 'list-pane master-detail__list';

  const detailEl = document.createElement('div');
  detailEl.className = 'master-detail__detail';

  wrap.appendChild(listEl);
  wrap.appendChild(detailEl);
  el.appendChild(wrap);

  return {
    listEl,
    detailEl,
    setView(v) { wrap.dataset.view = v; },
    destroy() { wrap.remove(); },
  };
}
