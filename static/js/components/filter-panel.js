// @ts-check

import { h, render } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import { Combobox } from './combobox.js';
import { Modal } from './modal.js';
import { t } from '../i18n.js';
const html = htm.bind(h);

/**
 * @typedef { { kind: 'Selection', data: { name: string, value: string } }
 * | { kind: 'Checkbox', data: boolean }
 * | { kind: 'TextInput', data: string }
 * | { kind: 'Multiselect', data: string[] } } FilterState
 */

/**
 * @typedef {{
 * id: string,
 * name: string,
 * tag?: 'select'|'checkbox'|'text-input'|'sort'|'multiselect',
 * filter_type?: 'Select'|'Checkbox'|'TextInput'|'Sort'|'Multiselect',
 * options: {filter_name?: string, name: string, value: string}[],
 * default_value?: FilterState | null,
 * semantic?: 'Author'|'Artist'|'Tag'|null,
 * }} FilterDef
 */

/**
 * Mounts a Filters button that opens a modal with an explicit Apply action.
 * Returns a destroy function.
 *
 * @param {HTMLElement} triggerBtn
 * @param {HTMLElement} modalRoot
 * @param {{
 * filterDefs: FilterDef[],
 * activeFilters: Record<string, FilterState>,
 * onChange: (filters: Record<string, FilterState>) => void,
 * }} props
 * @returns {() => void} destroy function
 */
export function mountFilterModal(triggerBtn, modalRoot, { filterDefs, activeFilters, onChange }) {
  let committed = { ...activeFilters };
  /** @type {HTMLDivElement | null} */
  let _mount = null;
  /** @type {Element | null} */
  let _prevFocus = null;

  function _updateBadge() {
    const count = Object.keys(committed).length;
    let badge = /** @type {HTMLElement | null} */ (triggerBtn.querySelector('.js-filter-badge'));
    if (count > 0) {
      if (!badge) {
        badge = document.createElement('span');
        badge.className = 'js-filter-badge inline-flex items-center justify-center w-4 h-4 text-2xs font-bold rounded-full bg-accent text-on-accent ml-1.5';
        triggerBtn.appendChild(badge);
      }
      badge.textContent = String(count);
    } else {
      badge?.remove();
    }
  }

  function _close() {
    if (_mount) {
      render(null, _mount);
      _mount.remove();
      _mount = null;
    }
    /** @type {HTMLElement | null} */ (_prevFocus)?.focus();
  }

  function _openModal() {
    if (_mount) return;
    _prevFocus = document.activeElement;
    _mount = document.createElement('div');
    modalRoot.appendChild(_mount);
    render(html`<${FilterModal}
      filterDefs=${filterDefs}
      committed=${committed}
      onApply=${(/** @type {Record<string, FilterState>} */ newFilters) => {
        committed = newFilters;
        _updateBadge();
        onChange({ ...committed });
        _close();
      }}
      onClose=${_close}
    />`, _mount);
  }

  triggerBtn.addEventListener('click', () => {
    if (_mount) return;
    _openModal();
  });

  _updateBadge();

  return () => {
    if (_mount) { render(null, _mount); _mount.remove(); _mount = null; }
  };
}


/**
 * @param {{
 *   filterDefs: FilterDef[],
 *   committed: Record<string, FilterState>,
 *   onApply: (filters: Record<string, FilterState>) => void,
 *   onClose: () => void,
 * }} props
 */
function FilterModal({ filterDefs, committed, onApply, onClose }) {
  const [draft, setDraft] = useState(() => ({ ...committed }));
  const applyRef = useRef(/** @type {HTMLButtonElement | null} */ (null));

  useEffect(() => {
    const id = setTimeout(() => applyRef.current?.focus(), 50);
    return () => clearTimeout(id);
  }, []);

  /** @param {string} id @param {FilterState | null} stateObj */
  function handleChange(id, stateObj) {
    setDraft(prev => {
      const next = { ...prev };
      if (!stateObj) delete next[id];
      else next[id] = stateObj;
      return next;
    });
  }

  function handleReset() {
    setDraft(_buildDefaultFilters(filterDefs));
  }

  function handleApply() {
    onApply({ ...draft });
  }

  return html`
    <${Modal}
      open=${true}
      title=${t('filter.panel.title')}
      onClose=${onClose}
      footer=${html`
        <div class="w-full flex items-center justify-between gap-2">
          <button type="button" class="btn-ghost btn-sm" onClick=${handleReset}>${t('filter.reset')}</button>
          <button type="button" class="btn-primary btn-sm" ref=${applyRef} onClick=${handleApply}>${t('filter.apply')}</button>
        </div>
      `}
    >
      <${FilterControls} filterDefs=${filterDefs} draft=${draft} onChange=${handleChange} />
    </${Modal}>
  `;
}

/**
 * @param {{
 *   filterDefs: FilterDef[],
 *   draft: Record<string, FilterState>,
 *   onChange: (id: string, stateObj: FilterState | null) => void,
 * }} props
 */
function FilterControls({ filterDefs, draft, onChange }) {
  return html`
    <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
      ${filterDefs.map(filter => html`<${FilterControl}
        key=${filter.id}
        filter=${filter}
        value=${draft[filter.id]}
        onChange=${onChange}
      />`)}
    </div>
  `;
}

/**
 * @param {{
 *   filter: FilterDef,
 *   value: FilterState | undefined,
 *   onChange: (id: string, stateObj: FilterState | null) => void,
 * }} props
 */
function FilterControl({ filter, value: curState, onChange }) {
  const tag = _filterTag(filter);
  const isCheckbox = tag === 'checkbox';
  const isMultiselect = tag === 'multiselect';
  const displayState = curState ?? _normalizeFilterState(filter.default_value ?? null);

  const wrapClass = isCheckbox
    ? 'flex items-center justify-between gap-3 py-1'
    : isMultiselect
      ? 'flex flex-col gap-1 col-span-2'
      : 'flex flex-col gap-1';

  const labelEl = isCheckbox
    ? html`<span class="text-xs font-medium text-text-muted uppercase tracking-wider">${filter.name}</span>`
    : html`<span class="text-xs font-medium text-text-muted uppercase tracking-wider">${filter.name}</span>`;

  let controlEl;

  if (tag === 'select') {
    const curVal = displayState?.kind === 'Selection' ? displayState.data.value : '';
    const hasDefault = !!filter.default_value;
    controlEl = html`
      <select class="input text-sm" value=${curVal} onChange=${(/** @type {Event} */ e) => {
        const val = /** @type {HTMLSelectElement} */ (e.target).value;
        if (!val) { onChange(filter.id, null); return; }
        const opt = filter.options.find(o => o.value === val);
        if (opt) onChange(filter.id, { kind: 'Selection', data: { name: opt.name, value: opt.value } });
      }}>
        ${!hasDefault && html`<option value="">${t('filter.any_option')}</option>`}
        ${filter.options.map(o => html`<option key=${o.value} value=${o.value}>${o.name}</option>`)}
      </select>
    `;

  } else if (tag === 'checkbox') {
    const checked = displayState?.kind === 'Checkbox' ? displayState.data : false;
    controlEl = html`
      <label class="kani-toggle shrink-0 cursor-pointer">
        <input type="checkbox" class="kani-toggle__input" checked=${checked} onChange=${(/** @type {Event} */ e) => {
          onChange(filter.id, /** @type {HTMLInputElement} */ (e.target).checked ? { kind: 'Checkbox', data: true } : null);
        }} />
        <span class="kani-toggle__track"></span>
      </label>
    `;

  } else if (tag === 'text-input') {
    const textVal = curState?.kind === 'TextInput' ? curState.data : '';
    controlEl = html`
      <input
        type="text"
        class="input text-sm"
        value=${textVal}
        placeholder=${filter.name}
        onInput=${(/** @type {Event} */ e) => {
          const trimmed = /** @type {HTMLInputElement} */ (e.target).value.trim();
          onChange(filter.id, trimmed ? { kind: 'TextInput', data: trimmed } : null);
        }}
      />
    `;

  } else if (tag === 'sort') {
    const curVal = displayState?.kind === 'Selection' ? displayState.data.value : '';
    const hasDefault = !!filter.default_value;
    controlEl = html`
      <select class="input text-sm" value=${curVal} onChange=${(/** @type {Event} */ e) => {
        const val = /** @type {HTMLSelectElement} */ (e.target).value;
        if (!val) { onChange(filter.id, null); return; }
        const [baseVal, dir] = val.split(':');
        const opt = filter.options.find(o => o.value === baseVal);
        if (opt) onChange(filter.id, {
          kind: 'Selection',
          data: { name: opt.name + ' ' + (dir === 'asc' ? '↑' : '↓'), value: val }
        });
      }}>
        ${!hasDefault && html`<option value="">${t('filter.default_option')}</option>`}
        ${filter.options.map(o => html`
          <option key=${o.value + ':asc'} value=${o.value + ':asc'}>${o.name} ↑</option>
          <option key=${o.value + ':desc'} value=${o.value + ':desc'}>${o.name} ↓</option>
        `)}
      </select>
    `;

  } else if (tag === 'multiselect') {
    const comboOptions = filter.options.map((o, i) => ({ id: i, name: o.name }));
    const optionValues = filter.options.map(o => o.value || o.name);
    const selectedVals = curState?.kind === 'Multiselect' ? curState.data : [];
    const selectedIds = selectedVals.map(v => optionValues.indexOf(v)).filter(i => i !== -1);

    controlEl = html`
      <${Combobox}
        multiple=${true}
        options=${comboOptions}
        value=${selectedIds}
        placeholder=${'Select ' + filter.name.toLowerCase() + '…'}
        onChange=${(/** @type {number[]} */ newIds) => {
          const newVals = newIds.map(i => optionValues[i]).filter(v => v != null);
          onChange(filter.id, newVals.length > 0 ? { kind: 'Multiselect', data: newVals } : null);
        }}
      />
    `;
  }

  return html`<div class=${wrapClass}>${labelEl}${controlEl}</div>`;
}


/**
 * Normalize a filter state value from either adjacently-tagged ({kind, data}) or
 * externally-tagged ({Selection: data}) serde format.
 * @param {any} raw
 * @returns {FilterState|null}
 */
function _normalizeFilterState(raw) {
  if (!raw || typeof raw !== 'object') return /** @type {any} */ (raw);
  if (typeof raw.kind === 'string') return /** @type {FilterState} */ (raw);
  const entries = Object.entries(raw);
  if (entries.length === 1) return /** @type {FilterState} */ ({ kind: entries[0][0], data: entries[0][1] });
  return null;
}

/**
 * Build an initial filter state from filter defaults.
 * @param {FilterDef[]} filterDefs
 * @returns {Record<string, FilterState>}
 */
function _buildDefaultFilters(filterDefs) {
  /** @type {Record<string, FilterState>} */
  const defaults = {};
  for (const f of filterDefs) {
    if (f.default_value) {
      const normalized = _normalizeFilterState(f.default_value);
      if (normalized) defaults[f.id] = normalized;
    }
  }
  return defaults;
}

/**
 * Normalise filter type tag to a lowercase internal identifier.
 * @param {FilterDef} filter
 * @returns {string}
 */
function _filterTag(filter) {
  const rawType = filter.tag ?? filter.filter_type ?? '';
  return rawType === 'TextInput'    ? 'text-input'
    : rawType === 'Select'          ? 'select'
    : rawType === 'Checkbox'        ? 'checkbox'
    : rawType === 'Sort'            ? 'sort'
    : rawType === 'Multiselect'     ? 'multiselect'
    : rawType.toLowerCase();
}
