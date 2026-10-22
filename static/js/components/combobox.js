// @ts-check

import { h, render } from 'preact';
import { useState, useEffect, useRef, useMemo } from 'preact/hooks';
import htm from 'htm';
import { iconX, iconChevronDown, iconCheck } from '../icons.js';
import { Icon } from './icon.js';
import { Pill } from './pill.js';
import { renderPopover, useOutsideClose } from './popover.js';
import { t } from '../i18n.js';
const html = htm.bind(h);

const ITEM_H = 36;
const VISIBLE = 8;

/**
 * Single-select mode (default):
 *   value: number | null, onChange: (id: number | null) => void
 *
 * Multi-select mode (multiple={true}):
 *   value: number[], onChange: (ids: number[]) => void
 *
 * Creatable multi-select (creatable={true}):
 *   value: string[], onChange: (names: string[]) => void
 *   Options are suggestions only — new values can be typed and added freely.
 *
 * @param {{
 *   options: Array<{ id: number, name: string }>,
 *   value: number | null | number[] | string[],
 *   onChange: ((id: number | null) => void) | ((ids: number[]) => void) | ((names: string[]) => void),
 *   placeholder?: string,
 *   disabled?: boolean,
 *   multiple?: boolean,
 *   creatable?: boolean,
 * }} props
 */
export function Combobox({ options, value, onChange, placeholder = 'Select…', disabled = false, multiple = false, creatable = false }) {
  if (creatable) {
    return html`<${CreatableMultiCombobox}
      options=${options}
      value=${/** @type {string[]} */ (Array.isArray(value) ? value : [])}
      onChange=${/** @type {(names: string[]) => void} */ (onChange)}
      placeholder=${placeholder}
      disabled=${disabled}
    />`;
  }
  if (multiple) {
    return html`<${MultiCombobox}
      options=${options}
      value=${/** @type {number[]} */ (Array.isArray(value) ? value : [])}
      onChange=${/** @type {(ids: number[]) => void} */ (onChange)}
      placeholder=${placeholder}
      disabled=${disabled}
    />`;
  }
  return html`<${SingleCombobox}
    options=${options}
    value=${/** @type {number | null} */ (value)}
    onChange=${/** @type {(id: number | null) => void} */ (onChange)}
    placeholder=${placeholder}
    disabled=${disabled}
  />`;
}


function SingleCombobox({ options, value, onChange, placeholder, disabled }) {
  const [inputText, setInputText] = useState(() => {
    const opt = options.find(o => o.id === value);
    return opt ? opt.name : '';
  });
  const [open, setOpen] = useState(false);
  const [highlighted, setHighlighted] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);
  const isTyping = useRef(false);
  // Tracks the last confirmed selection independently of the value prop, so _close()
  // doesn't depend on a stale closure if the parent re-renders asynchronously.
  const selectedIdRef = useRef(value);
  const dropdownRef = useRef(/** @type {HTMLDivElement | null} */(null));
  const wrapRef = useRef(/** @type {HTMLDivElement | null} */(null));

  useEffect(() => {
    selectedIdRef.current = value;
    if (isTyping.current) return;
    const opt = options.find(o => o.id === value);
    setInputText(opt ? opt.name : '');
  }, [value, options]);

  const filtered = useMemo(() => {
    const q = inputText.trim().toLowerCase();
    if (!q || !isTyping.current) return options;
    return options.filter(o => o.name.toLowerCase().includes(q));
  }, [inputText, options]);

  useEffect(() => { setHighlighted(0); }, [filtered]);

  useOutsideClose(open, [wrapRef, dropdownRef], _close);

  // Portal the dropdown into #popover-root so it escapes overflow:hidden in modals.
  useEffect(() => {
    if (!open || !filtered.length || !wrapRef.current) {
      renderPopover(null);
      return;
    }
    const rect = wrapRef.current.getBoundingClientRect();
    const localWinStart = Math.max(0, Math.floor(scrollTop / ITEM_H) - 2);
    const localWinItems = filtered.slice(localWinStart, localWinStart + VISIBLE + 4);
    const localTotalH = filtered.length * ITEM_H;
    const localDropH = Math.min(filtered.length, VISIBLE) * ITEM_H;
    const spaceBelow = window.innerHeight - rect.bottom - 4;
    const spaceAbove = rect.top - 4;
    const openUp = spaceBelow < localDropH && spaceAbove > localDropH;
    const dropTop = openUp ? rect.top - localDropH - 4 : rect.bottom + 4;
    // Clamp horizontally so the popover never clips off the viewport edge on narrow screens.
    const dropWidth = Math.min(rect.width, window.innerWidth - 8);
    const dropLeft  = Math.max(4, Math.min(rect.left, window.innerWidth - dropWidth - 4));

    renderPopover(html`
      <div
        role="listbox"
        style=${{
          position: 'fixed',
          top: dropTop + 'px',
          left: dropLeft + 'px',
          width: dropWidth + 'px',
          height: localDropH + 'px',
          background: 'var(--color-surface)',
          border: '1px solid var(--color-border)',
          borderRadius: 'var(--radius-lg)',
          boxShadow: 'var(--shadow-popover)',
          overflowY: 'auto',
          zIndex: 'var(--z-popover)',
          pointerEvents: 'auto',
        }}
        ref=${dropdownRef}
        onScroll=${(e) => setScrollTop(/** @type {HTMLElement} */(e.target).scrollTop)}
      >
        <div style=${{ height: localTotalH + 'px', position: 'relative' }}>
          ${localWinItems.map((opt, i) => {
            const idx = localWinStart + i;
            return html`
              <div
                id=${'combobox-opt-' + opt.id}
                key=${opt.id}
                role="option"
                aria-selected=${idx === highlighted}
                class=${'flex items-center px-3 text-sm cursor-pointer ' + (idx === highlighted ? 'bg-surface-2 text-text' : 'text-text hover:bg-surface-2')}
                style=${{ position: 'absolute', top: idx * ITEM_H + 'px', width: '100%', height: ITEM_H + 'px' }}
                onMouseDown=${(e) => { e.preventDefault(); _select(opt); }}
                onMouseEnter=${() => setHighlighted(idx)}
              >
                ${opt.name}
              </div>
            `;
          })}
        </div>
      </div>
    `);
  }, [open, filtered, highlighted, scrollTop]);

  useEffect(() => () => renderPopover(null), []);

  function _close() {
    isTyping.current = false;
    setOpen(false);
    // Use selectedIdRef rather than the value prop to avoid stale-closure issues
    // when the parent hasn't re-rendered yet with the new value.
    const opt = options.find(o => o.id === selectedIdRef.current);
    setInputText(opt ? opt.name : '');
  }

  function _select(opt) {
    selectedIdRef.current = opt.id;
    isTyping.current = false;
    onChange(opt.id);
    setInputText(opt.name);
    setOpen(false);
  }

  /** @param {MouseEvent} e */
  function _clear(e) {
    e.stopPropagation();
    selectedIdRef.current = null;
    isTyping.current = false;
    onChange(null);
    setInputText('');
    setOpen(false);
  }

  function _scrollTo(idx) {
    const dd = dropdownRef.current;
    if (!dd) return;
    const top = idx * ITEM_H;
    if (top < dd.scrollTop) dd.scrollTop = top;
    else if (top + ITEM_H > dd.scrollTop + dd.clientHeight) dd.scrollTop = top + ITEM_H - dd.clientHeight;
  }

  /** @param {KeyboardEvent} e */
  function _onKeyDown(e) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      const next = Math.min(highlighted + 1, filtered.length - 1);
      setHighlighted(next);
      _scrollTo(next);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      const prev = Math.max(highlighted - 1, 0);
      setHighlighted(prev);
      _scrollTo(prev);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (open && filtered[highlighted]) _select(filtered[highlighted]);
      else setOpen(true);
    } else if (e.key === 'Escape' || e.key === 'Tab') {
      _close();
    }
  }

  return html`
    <div class="relative" ref=${wrapRef}>
      <div class="relative flex items-center">
        <input
          type="text"
          role="combobox"
          class="input pr-8"
          value=${inputText}
          placeholder=${placeholder}
          disabled=${disabled}
          aria-expanded=${open}
          aria-autocomplete="list"
          aria-activedescendant=${open && filtered[highlighted] ? 'combobox-opt-' + filtered[highlighted].id : undefined}
          onInput=${(e) => {
            isTyping.current = true;
            setInputText(/** @type {HTMLInputElement} */(e.target).value);
            if (!open) setOpen(true);
          }}
          onFocus=${() => { if (!disabled) setOpen(true); }}
          onKeyDown=${_onKeyDown}
        />
        ${(value != null || inputText.trim() !== '')
          ? html`<button type="button" class="absolute right-1 top-1/2 -translate-y-1/2 btn-icon w-8 h-8 border-0" aria-label=${t('common.clear')} onClick=${_clear}><${Icon} svg=${iconX} /></button>`
          : html`<span class="absolute right-2 top-1/2 -translate-y-1/2 text-text-muted pointer-events-none icon-sm" aria-hidden="true"><${Icon} svg=${iconChevronDown} /></span>`
        }
      </div>
    </div>
  `;
}


function MultiCombobox({ options, value, onChange, placeholder, disabled }) {
  const [selectedIds, setSelectedIds] = useState(/** @type {number[]} */ (Array.isArray(value) ? value : []));

  useEffect(() => {
    setSelectedIds(Array.isArray(value) ? /** @type {number[]} */ (value) : []);
  }, [value]);

  const [inputText, setInputText] = useState('');
  const [open, setOpen] = useState(false);
  const [highlighted, setHighlighted] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);
  const dropdownRef = useRef(/** @type {HTMLDivElement | null} */(null));
  const wrapRef = useRef(/** @type {HTMLDivElement | null} */(null));
  const inputRef = useRef(/** @type {HTMLInputElement | null} */(null));

  const filtered = useMemo(() => {
    const q = inputText.trim().toLowerCase();
    if (!q) return options;
    return options.filter(o => o.name.toLowerCase().includes(q));
  }, [inputText, options]);

  useEffect(() => { setHighlighted(0); }, [filtered]);

  useOutsideClose(open, [wrapRef, dropdownRef], () => { setOpen(false); setInputText(''); });

  useEffect(() => {
    if (!open || !filtered.length || !wrapRef.current) {
      renderPopover(null);
      return;
    }
    const rect = wrapRef.current.getBoundingClientRect();
    const localWinStart = Math.max(0, Math.floor(scrollTop / ITEM_H) - 2);
    const localWinItems = filtered.slice(localWinStart, localWinStart + VISIBLE + 4);
    const localTotalH = filtered.length * ITEM_H;
    const localDropH = Math.min(filtered.length, VISIBLE) * ITEM_H;
    const spaceBelow = window.innerHeight - rect.bottom - 4;
    const spaceAbove = rect.top - 4;
    const openUp = spaceBelow < localDropH && spaceAbove > localDropH;
    const dropTop = openUp ? rect.top - localDropH - 4 : rect.bottom + 4;
    const dropWidth = Math.min(rect.width, window.innerWidth - 8);
    const dropLeft  = Math.max(4, Math.min(rect.left, window.innerWidth - dropWidth - 4));

    renderPopover(html`
      <div
        role="listbox"
        aria-multiselectable="true"
        style=${{
          position: 'fixed',
          top: dropTop + 'px',
          left: dropLeft + 'px',
          width: dropWidth + 'px',
          height: localDropH + 'px',
          background: 'var(--color-surface)',
          border: '1px solid var(--color-border)',
          borderRadius: 'var(--radius-lg)',
          boxShadow: 'var(--shadow-popover)',
          overflowY: 'auto',
          zIndex: 'var(--z-popover)',
          pointerEvents: 'auto',
        }}
        ref=${dropdownRef}
        onScroll=${(/** @type {any} */ e) => setScrollTop(/** @type {HTMLElement} */(e.target).scrollTop)}
      >
        <div style=${{ height: localTotalH + 'px', position: 'relative' }}>
          ${localWinItems.map((opt, i) => {
            const idx = localWinStart + i;
            const isSelected = selectedIds.includes(opt.id);
            const isHighlighted = idx === highlighted;
            return html`
              <div
                id=${'combobox-multi-opt-' + opt.id}
                key=${opt.id}
                role="option"
                aria-selected=${isSelected}
                class=${'flex items-center gap-2 px-3 text-sm cursor-pointer select-none '
                  + (isHighlighted ? 'bg-surface-2 text-text' : 'text-text hover:bg-surface-2')}
                style=${{ position: 'absolute', top: idx * ITEM_H + 'px', width: '100%', height: ITEM_H + 'px' }}
                onMouseDown=${(/** @type {MouseEvent} */ e) => { e.preventDefault(); _toggle(opt); }}
                onMouseEnter=${() => setHighlighted(idx)}
              >
                <span class=${'shrink-0 w-4 h-4 rounded border flex items-center justify-center text-2xs leading-none '
                  + (isSelected ? 'bg-accent border-accent text-on-accent' : 'border-border bg-surface')}>
                  ${isSelected ? html`<span class="icon-2xs"><${Icon} svg=${iconCheck} /></span>` : ''}
                </span>
                ${opt.name}
              </div>
            `;
          })}
        </div>
      </div>
    `);
  }, [open, filtered, highlighted, scrollTop, selectedIds]);

  useEffect(() => () => renderPopover(null), []);

  function _toggle(opt) {
    const next = selectedIds.indexOf(opt.id) === -1
      ? [...selectedIds, opt.id]
      : selectedIds.filter(id => id !== opt.id);
    setSelectedIds(next);
    onChange(/** @type {any} */ (next));
  }

  function _scrollTo(idx) {
    const dd = dropdownRef.current;
    if (!dd) return;
    const top = idx * ITEM_H;
    if (top < dd.scrollTop) dd.scrollTop = top;
    else if (top + ITEM_H > dd.scrollTop + dd.clientHeight) dd.scrollTop = top + ITEM_H - dd.clientHeight;
  }

  /** @param {KeyboardEvent} e */
  function _onKeyDown(e) {
    if (e.key === 'Backspace' && inputText === '' && selectedIds.length > 0) {
      const next = selectedIds.slice(0, -1);
      setSelectedIds(next);
      onChange(/** @type {any} */ (next));
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      const next = Math.min(highlighted + 1, filtered.length - 1);
      setHighlighted(next);
      _scrollTo(next);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      const prev = Math.max(highlighted - 1, 0);
      setHighlighted(prev);
      _scrollTo(prev);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (open && filtered[highlighted]) _toggle(filtered[highlighted]);
      else setOpen(true);
    } else if (e.key === 'Escape') {
      setOpen(false);
      setInputText('');
    }
  }

  return html`
    <div class="relative" ref=${wrapRef}>
      <div
        class="input-chips pr-8 relative"
        onClick=${() => { if (!disabled) inputRef.current?.focus(); }}
      >
        ${selectedIds.map(id => {
          const opt = options.find(o => o.id === id);
          if (!opt) return null;
          return html`<${Pill} key=${id} label=${opt.name} onDismiss=${() => _toggle(opt)} />`;
        })}
        <input
          ref=${inputRef}
          type="text"
          role="combobox"
          value=${inputText}
          placeholder=${selectedIds.length === 0 ? placeholder : ''}
          disabled=${disabled}
          aria-expanded=${open}
          aria-autocomplete="list"
          aria-multiselectable="true"
          aria-activedescendant=${open && filtered[highlighted] ? 'combobox-multi-opt-' + filtered[highlighted].id : undefined}
          onInput=${(/** @type {any} */ e) => {
            setInputText(/** @type {HTMLInputElement} */(e.target).value);
            if (!open) setOpen(true);
          }}
          onFocus=${() => { if (!disabled) setOpen(true); }}
          onKeyDown=${_onKeyDown}
        />
        <span class="absolute right-2 top-1/2 -translate-y-1/2 text-text-muted pointer-events-none icon-sm" aria-hidden="true">
          <${Icon} svg=${iconChevronDown} />
        </span>
      </div>
    </div>
  `;
}


const _CREATE_ID = '__create__';

function CreatableMultiCombobox({ options, value, onChange, placeholder, disabled }) {
  const selectedNames = /** @type {string[]} */ (Array.isArray(value) ? value : []);

  const [inputText, setInputText] = useState('');
  const [open, setOpen] = useState(false);
  const [highlighted, setHighlighted] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);
  const dropdownRef = useRef(/** @type {HTMLDivElement | null} */(null));
  const wrapRef = useRef(/** @type {HTMLDivElement | null} */(null));
  const inputRef = useRef(/** @type {HTMLInputElement | null} */(null));

  const dropItems = useMemo(() => {
    const q = inputText.trim().toLowerCase();
    const filtered = options.filter(o => {
      if (selectedNames.some(n => n.toLowerCase() === o.name.toLowerCase())) return false;
      return !q || o.name.toLowerCase().includes(q);
    });
    const trimmed = inputText.trim();
    const alreadyExact = trimmed
      && (options.some(o => o.name.toLowerCase() === trimmed.toLowerCase())
        || selectedNames.some(n => n.toLowerCase() === trimmed.toLowerCase()));
    if (trimmed && !alreadyExact) {
      return [...filtered, { id: _CREATE_ID, name: trimmed }];
    }
    return filtered;
  }, [inputText, options, selectedNames]);

  useEffect(() => { setHighlighted(0); }, [dropItems]);

  useOutsideClose(open, [wrapRef, dropdownRef], () => { setOpen(false); setInputText(''); });

  useEffect(() => {
    if (!open || !dropItems.length || !wrapRef.current) {
      renderPopover(null);
      return;
    }
    const rect = wrapRef.current.getBoundingClientRect();
    const localWinStart = Math.max(0, Math.floor(scrollTop / ITEM_H) - 2);
    const localWinItems = dropItems.slice(localWinStart, localWinStart + VISIBLE + 4);
    const localTotalH = dropItems.length * ITEM_H;
    const localDropH = Math.min(dropItems.length, VISIBLE) * ITEM_H;
    const spaceBelow = window.innerHeight - rect.bottom - 4;
    const spaceAbove = rect.top - 4;
    const openUp = spaceBelow < localDropH && spaceAbove > localDropH;
    const dropTop = openUp ? rect.top - localDropH - 4 : rect.bottom + 4;
    const dropWidth = Math.min(rect.width, window.innerWidth - 8);
    const dropLeft  = Math.max(4, Math.min(rect.left, window.innerWidth - dropWidth - 4));

    renderPopover(html`
      <div
        role="listbox"
        style=${{
          position: 'fixed',
          top: dropTop + 'px',
          left: dropLeft + 'px',
          width: dropWidth + 'px',
          height: localDropH + 'px',
          background: 'var(--color-surface)',
          border: '1px solid var(--color-border)',
          borderRadius: 'var(--radius-lg)',
          boxShadow: 'var(--shadow-popover)',
          overflowY: 'auto',
          zIndex: 'var(--z-popover)',
          pointerEvents: 'auto',
        }}
        ref=${dropdownRef}
        onScroll=${(/** @type {any} */ e) => setScrollTop(/** @type {HTMLElement} */(e.target).scrollTop)}
      >
        <div style=${{ height: localTotalH + 'px', position: 'relative' }}>
          ${localWinItems.map((opt, i) => {
            const idx = localWinStart + i;
            const isCreate = opt.id === _CREATE_ID;
            const isHighlighted = idx === highlighted;
            return html`
              <div
                id=${'combobox-create-opt-' + (isCreate ? _CREATE_ID : opt.id)}
                key=${isCreate ? _CREATE_ID : opt.id}
                role="option"
                class=${'flex items-center gap-2 px-3 text-sm cursor-pointer select-none '
                  + (isHighlighted ? 'bg-surface-2 text-text' : 'text-text hover:bg-surface-2')}
                style=${{ position: 'absolute', top: idx * ITEM_H + 'px', width: '100%', height: ITEM_H + 'px' }}
                onMouseDown=${(/** @type {MouseEvent} */ e) => { e.preventDefault(); _add(opt); }}
                onMouseEnter=${() => setHighlighted(idx)}
              >
                ${isCreate
                  ? html`<span class="text-accent font-medium">+</span><span>${t('combobox.add_new')} "<em>${opt.name}</em>"</span>`
                  : opt.name
                }
              </div>
            `;
          })}
        </div>
      </div>
    `);
  }, [open, dropItems, highlighted, scrollTop]);

  useEffect(() => () => renderPopover(null), []);

  /** @param {{ id: any, name: string }} opt */
  function _add(opt) {
    onChange([...selectedNames, opt.name]);
    setInputText('');
    inputRef.current?.focus();
  }

  function _scrollTo(idx) {
    const dd = dropdownRef.current;
    if (!dd) return;
    const top = idx * ITEM_H;
    if (top < dd.scrollTop) dd.scrollTop = top;
    else if (top + ITEM_H > dd.scrollTop + dd.clientHeight) dd.scrollTop = top + ITEM_H - dd.clientHeight;
  }

  /** @param {KeyboardEvent} e */
  function _onKeyDown(e) {
    if (e.key === 'Backspace' && inputText === '' && selectedNames.length > 0) {
      onChange(selectedNames.slice(0, -1));
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      const next = Math.min(highlighted + 1, dropItems.length - 1);
      setHighlighted(next);
      _scrollTo(next);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      const prev = Math.max(highlighted - 1, 0);
      setHighlighted(prev);
      _scrollTo(prev);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (open && dropItems[highlighted]) _add(dropItems[highlighted]);
      else setOpen(true);
    } else if (e.key === 'Escape') {
      setOpen(false);
      setInputText('');
    }
  }

  return html`
    <div class="relative" ref=${wrapRef}>
      <div
        class="input-chips pr-8 relative"
        onClick=${() => { if (!disabled) inputRef.current?.focus(); }}
      >
        ${selectedNames.map(name => html`
          <${Pill} key=${name} label=${name}
            onDismiss=${() => onChange(selectedNames.filter(n => n !== name))} />
        `)}
        <input
          ref=${inputRef}
          type="text"
          role="combobox"
          value=${inputText}
          placeholder=${selectedNames.length === 0 ? placeholder : ''}
          disabled=${disabled}
          aria-expanded=${open}
          aria-autocomplete="list"
          aria-activedescendant=${open && dropItems[highlighted] ? 'combobox-create-opt-' + (dropItems[highlighted].id === _CREATE_ID ? _CREATE_ID : dropItems[highlighted].id) : undefined}
          onInput=${(/** @type {any} */ e) => {
            setInputText(/** @type {HTMLInputElement} */(e.target).value);
            if (!open) setOpen(true);
          }}
          onFocus=${() => { if (!disabled) setOpen(true); }}
          onKeyDown=${_onKeyDown}
        />
        <span class="absolute right-2 top-1/2 -translate-y-1/2 text-text-muted pointer-events-none icon-sm" aria-hidden="true">
          <${Icon} svg=${iconChevronDown} />
        </span>
      </div>
    </div>
  `;
}
