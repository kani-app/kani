// @ts-check

import { h } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import { Icon } from '../icon.js';
import { iconChevronDown } from '../../icons.js';
import { renderPopover, useOutsideClose } from '../popover.js';

const html = htm.bind(h);

const ITEM_H = 34;

/**
 * @param {{
 *   options: Array<{ value: string, label: string, group?: string }>,
 *   value: string,
 *   onChange: (value: string) => void,
 *   disabled?: boolean,
 *   ariaLabel?: string,
 *   class?: string,
 * }} props
 */
export function Select({ options, value, onChange, disabled = false, ariaLabel, class: klass = '' }) {
  const [open, setOpen] = useState(false);
  const [highlighted, setHighlighted] = useState(0);
  const btnRef = useRef(/** @type {HTMLButtonElement|null} */ (null));
  const listRef = useRef(/** @type {HTMLDivElement|null} */ (null));

  const current = options.find(o => o.value === value);

  useEffect(() => {
    if (open) setHighlighted(Math.max(0, options.findIndex(o => o.value === value)));
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const btn = btnRef.current;
    if (!btn) return;
    const rect = btn.getBoundingClientRect();
    /** @type {Array<{ heading: string } | { opt: { value: string, label: string, group?: string }, i: number }>} */
    const items = [];
    let lastGroup;
    options.forEach((opt, i) => {
      if (opt.group && opt.group !== lastGroup) {
        items.push({ heading: opt.group });
        lastGroup = opt.group;
      }
      items.push({ opt, i });
    });
    const dropH = Math.min(items.length, 10) * ITEM_H + 8;
    const spaceBelow = window.innerHeight - rect.bottom - 4;
    const openUp = spaceBelow < dropH && rect.top > dropH;
    const top = openUp ? rect.top - dropH - 4 : rect.bottom + 4;
    // Size to the widest option, not to the trigger: a narrow trigger ("All job
    // types") next to long labels ("Pending delete retry") would otherwise wrap
    // them into the next row's fixed height and overlap.
    const minWidth = Math.max(rect.width, 140);
    const left = Math.max(4, Math.min(rect.left, window.innerWidth - minWidth - 4));
    const maxWidth = Math.max(minWidth, window.innerWidth - left - 8);

    renderPopover(html`
      <div
        role="listbox"
        ref=${listRef}
        style=${{
          position: 'fixed', top: top + 'px', left: left + 'px',
          width: 'max-content', minWidth: minWidth + 'px', maxWidth: maxWidth + 'px',
          maxHeight: dropH + 'px', overflowY: 'auto',
          background: 'var(--color-surface)',
          border: '1px solid var(--color-border)',
          borderRadius: 'var(--radius-lg)',
          boxShadow: 'var(--shadow-popover)',
          zIndex: 'var(--z-popover)',
          padding: '4px',
          pointerEvents: 'auto',
        }}
      >
        ${items.map((item) => 'heading' in item
          ? html`
            <div
              key=${'h-' + item.heading}
              class="px-2.5 pt-2 pb-1 text-2xs uppercase tracking-wider text-text-faint select-none whitespace-nowrap"
              aria-hidden="true"
            >${item.heading}</div>`
          : html`
            <div
              key=${item.opt.value}
              role="option"
              aria-selected=${item.opt.value === value}
              class=${'flex items-center px-2.5 rounded-md text-sm cursor-pointer select-none whitespace-nowrap truncate '
                + (item.i === highlighted ? 'bg-surface-2 text-text' : 'text-text')
                + (item.opt.value === value ? ' font-medium' : '')}
              style=${{ height: ITEM_H - 4 + 'px' }}
              onMouseDown=${(/** @type {MouseEvent} */ e) => { e.preventDefault(); _pick(item.opt); }}
              onMouseEnter=${() => setHighlighted(item.i)}
            >${item.opt.label}</div>`
        )}
      </div>
    `);

    return () => renderPopover(null);
  }, [open, highlighted, options, value]);

  useOutsideClose(open, [btnRef, listRef], () => setOpen(false));

  useEffect(() => () => renderPopover(null), []);

  function _pick(/** @type {{value: string}} */ opt) {
    setOpen(false);
    if (opt.value !== value) onChange(opt.value);
  }

  function _onKeyDown(/** @type {KeyboardEvent} */ e) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (!open) { setOpen(true); return; }
      setHighlighted(h => Math.min(h + 1, options.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlighted(h => Math.max(h - 1, 0));
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      if (open && options[highlighted]) _pick(options[highlighted]);
      else setOpen(true);
    } else if (e.key === 'Escape') {
      setOpen(false);
    } else if (e.key.length === 1 && /\S/.test(e.key)) {
      const q = e.key.toLowerCase();
      const idx = options.findIndex(o => o.label.toLowerCase().startsWith(q));
      if (idx >= 0) { setHighlighted(idx); if (!open) _pick(options[idx]); }
    }
  }

  return html`
    <button
      type="button"
      ref=${btnRef}
      class=${'input flex items-center justify-between gap-2 text-left w-auto ' + klass}
      disabled=${disabled}
      aria-haspopup="listbox"
      aria-expanded=${open}
      aria-label=${ariaLabel}
      onClick=${() => { if (!disabled) setOpen(o => !o); }}
      onKeyDown=${_onKeyDown}
    >
      <span class="truncate text-sm">${current?.label ?? ''}</span>
      <span class="icon-sm text-text-muted shrink-0" aria-hidden="true"><${Icon} svg=${iconChevronDown} /></span>
    </button>
  `;
}
