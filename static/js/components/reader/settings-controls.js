// @ts-check
import { h } from 'preact';
import htm from 'htm';
import { Icon } from '../icon.js';

const html = htm.bind(h);

const BTN_ACTIVE   = 'bg-surface-2 text-text';
const BTN_INACTIVE = 'text-muted hover:bg-surface-2 hover:text-text';

/** A labelled sub-group within a settings tab; divider above unless `first`. @param {{ title?: string, first?: boolean, children?: any }} props */
export function Group({ title, first = false, children }) {
  return html`
    <div class=${'flex flex-col gap-3 ' + (first ? '' : 'border-t border-border-subtle pt-4')}>
      ${title ? html`<p class="text-xs font-medium text-muted uppercase tracking-wide">${title}</p>` : null}
      ${children}
    </div>`;
}

/** @param {{ label: string, checked: boolean, onChange: (v: boolean) => void }} props */
export function ToggleRow({ label, checked, onChange }) {
  return html`
    <label class="flex items-center justify-between gap-3 cursor-pointer">
      <span class="text-sm text-text">${label}</span>
      <label class="kani-toggle" aria-label=${label}>
        <input type="checkbox" class="kani-toggle__input" checked=${checked}
               onChange=${(/** @type {any} */ e) => onChange(e.currentTarget.checked)} />
        <span class="kani-toggle__track"></span>
      </label>
    </label>`;
}

/** @param {{ label: string, min: number, max: number, value: number, step?: number, unit?: string, onChange: (v: number) => void }} props */
export function SliderRow({ label, min, max, value, step = 1, unit = '', onChange }) {
  const commit = (/** @type {string} */ raw) => onChange(Math.max(min, Math.min(max, Number(raw))));
  return html`
    <div class="flex flex-col gap-1.5">
      <div class="flex items-center justify-between gap-2">
        <span class="text-sm text-text min-w-0 truncate">${label}</span>
        <input type="number" min=${min} max=${max} step=${step} value=${value} title=${unit}
               aria-label=${label}
               class="no-spinners text-xs text-muted tabular-nums text-right w-14 bg-transparent border border-border rounded px-1 py-0.5 shrink-0 focus:outline-none focus:border-accent"
               onChange=${(/** @type {any} */ e) => commit(e.currentTarget.value)}
               onKeyDown=${(/** @type {any} */ e) => e.stopPropagation()} />
      </div>
      <input type="range" min=${min} max=${max} step=${step} value=${value}
             aria-label=${label}
             class="w-full accent-accent h-1.5 cursor-pointer"
             onInput=${(/** @type {any} */ e) => onChange(Number(e.currentTarget.value))} />
    </div>`;
}

/** @param {{ label?: string, options: {value: string, label: string, icon?: string}[], selected: string, onSelect: (v: string) => void }} props */
export function SegmentedRow({ label, options, selected, onSelect }) {
  const hasIcons = options.some(o => o.icon);
  return html`
    <div class="flex flex-col gap-1.5">
      ${label ? html`<p class="text-xs text-muted">${label}</p>` : null}
      <div class="flex gap-2">
        ${options.map(opt => html`
          <button aria-pressed=${opt.value === selected}
                  class=${'flex-1 rounded-md transition-colors ' + (hasIcons
                    ? 'flex flex-col items-center gap-1 px-1 py-2 text-xs leading-none '
                    : 'text-sm px-2 py-1.5 ') + (opt.value === selected ? BTN_ACTIVE : BTN_INACTIVE)}
                  onClick=${() => onSelect(opt.value)}>
            ${opt.icon ? html`<${Icon} svg=${opt.icon} />` : null}
            <span>${opt.label}</span>
          </button>`)}
      </div>
    </div>`;
}

/** @param {{ label?: string, options: {value: string, label: string}[], selected: string, disabled?: boolean, onChange: (v: string) => void }} props */
export function SelectRow({ label, options, selected, disabled = false, onChange }) {
  return html`
    <div class="flex flex-col gap-1.5">
      ${label ? html`<p class="text-xs text-muted">${label}</p>` : null}
      <select class="input w-full text-sm" disabled=${disabled}
              onChange=${(/** @type {any} */ e) => onChange(e.currentTarget.value)}>
        ${options.map(opt => html`<option value=${opt.value} selected=${opt.value === selected}>${opt.label}</option>`)}
      </select>
    </div>`;
}

/** @param {{ label: string, onClick: () => void }} props */
export function ActionBtn({ label, onClick }) {
  return html`
    <button class="btn-ghost w-full flex items-center justify-center gap-1 text-sm" onClick=${onClick}>${label}</button>`;
}
