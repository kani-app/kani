// @ts-check
// Display menu — a small popover housing view-only toggles (hide read, hide
// completed) that don't need the same visual weight as the primary filters.

import { h, render } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import { Icon } from '../icon.js';
import { iconEye, iconChevronDown } from '../../icons.js';
import { t } from '../../i18n.js';
import { renderPopover, useOutsideClose } from '../popover.js';

const html = htm.bind(h);

/**
 * @param {{
 *   hideRead: boolean,
 *   hideCompleted: boolean,
 *   onChangeHideRead: (v: boolean) => void,
 *   onChangeHideCompleted: (v: boolean) => void,
 * }} props
 */
function DisplayMenu({ hideRead, hideCompleted, onChangeHideRead, onChangeHideCompleted }) {
  const [open, setOpen] = useState(false);
  const btnRef = useRef(/** @type {HTMLButtonElement|null} */ (null));
  const panelRef = useRef(/** @type {HTMLDivElement|null} */ (null));
  const activeCount = (hideRead ? 1 : 0) + (hideCompleted ? 1 : 0);

  useEffect(() => {
    if (!open || !btnRef.current) { renderPopover(null); return; }
    const rect = btnRef.current.getBoundingClientRect();
    const width = 220;
    const left = Math.max(4, Math.min(rect.right - width, window.innerWidth - width - 4));
    const top = rect.bottom + 4;

    renderPopover(html`
      <div
        ref=${panelRef}
        class="flex flex-col gap-1 p-2"
        style=${{
          position: 'fixed', top: top + 'px', left: left + 'px', width: width + 'px',
          background: 'var(--color-surface)',
          border: '1px solid var(--color-border)',
          borderRadius: 'var(--radius-lg)',
          boxShadow: 'var(--shadow-popover)',
          zIndex: 'var(--z-popover)',
        }}
      >
        <label class="flex items-center justify-between gap-3 px-2 py-1.5 text-sm text-text cursor-pointer select-none rounded-md hover:bg-surface-2">
          <span>${t('library.hide_read')}</span>
          <label class="kani-toggle">
            <input type="checkbox" class="kani-toggle__input" checked=${hideRead}
              onChange=${(/** @type {any} */ e) => onChangeHideRead(e.target.checked)} />
            <span class="kani-toggle__track"></span>
          </label>
        </label>
        <label class="flex items-center justify-between gap-3 px-2 py-1.5 text-sm text-text cursor-pointer select-none rounded-md hover:bg-surface-2">
          <span>${t('library.hide_completed')}</span>
          <label class="kani-toggle">
            <input type="checkbox" class="kani-toggle__input" checked=${hideCompleted}
              onChange=${(/** @type {any} */ e) => onChangeHideCompleted(e.target.checked)} />
            <span class="kani-toggle__track"></span>
          </label>
        </label>
      </div>
    `);
  }, [open, hideRead, hideCompleted]);

  useOutsideClose(open, [btnRef, panelRef], () => setOpen(false));

  useEffect(() => () => renderPopover(null), []);

  return html`
    <button
      type="button"
      ref=${btnRef}
      class="btn-secondary btn-sm flex items-center gap-1.5 shrink-0"
      aria-haspopup="true"
      aria-expanded=${open}
      onClick=${() => setOpen(o => !o)}
    >
      <span class="icon-sm" aria-hidden="true"><${Icon} svg=${iconEye} /></span>
      <span>${t('library.display')}</span>
      ${activeCount > 0 && html`<span class="nav-badge">${activeCount}</span>`}
      <span class="icon-sm" aria-hidden="true"><${Icon} svg=${iconChevronDown} /></span>
    </button>
  `;
}

/**
 * @param {HTMLElement} container
 * @param {{
 *   hideRead: boolean,
 *   hideCompleted: boolean,
 *   onChangeHideRead: (v: boolean) => void,
 *   onChangeHideCompleted: (v: boolean) => void,
 * }} props
 */
export function mountDisplayMenu(container, props) {
  render(html`<${DisplayMenu} ...${props} />`, container);
}
