// @ts-check
// Preact/htm row primitives for settings sections. The canonical building
// blocks every settings section is composed from.

import { h } from 'preact';
import { useState } from 'preact/hooks';
import htm from 'htm';
import { NumberInput } from '../../components/form/number-input.js';

const html = htm.bind(h);

/**
 * Titled card group.
 * @param {{ label?: string, cardClass?: string, children?: any }} props
 */
export function SettingsGroup({ label, cardClass = '', children }) {
  return html`
    <div class="flex flex-col gap-2">
      ${label
        ? html`<h3 class="font-display text-base font-bold text-text px-1">${label}</h3>`
        : null}
      <div
        class=${'bg-surface border border-border-subtle rounded-xl divide-y divide-border-subtle overflow-hidden ' +
        cardClass}
      >
        ${children}
      </div>
    </div>
  `;
}

/**
 * Row: label + optional description on the left, control (children) on the right.
 * @param {{ label: string, description?: string, badge?: string, tooltip?: string, children?: any }} props
 */
export function SettingsRow({ label, description, badge, tooltip, children }) {
  return html`
    <div
      class="flex flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:justify-between sm:gap-4 px-4"
      data-settings-row
      ...${tooltip ? { 'data-tooltip': tooltip } : {}}
    >
      <div class="flex flex-col gap-0.5 min-w-0">
        <div class="flex items-center gap-2">
          <span class="text-sm font-medium text-text">${label}</span>
          ${badge
            ? html`<span
                class="text-xs px-1.5 py-0.5 rounded bg-warn/20 text-warn font-medium"
                >${badge}</span
              >`
            : null}
        </div>
        ${description
          ? html`<span class="text-xs text-text-muted">${description}</span>`
          : null}
      </div>
      ${children
        ? html`<div class="shrink-0 self-start sm:self-center">${children}</div>`
        : null}
    </div>
  `;
}

/**
 * Toggle row.
 * @param {{ label: string, description?: string, tooltip?: string, checked: boolean, onChange: (v: boolean) => void }} props
 */
export function ToggleRow({ label, description, tooltip, checked, onChange }) {
  return html`
    <${SettingsRow} label=${label} description=${description} tooltip=${tooltip}>
      <label class="kani-toggle">
        <input
          type="checkbox"
          class="kani-toggle__input"
          checked=${checked}
          onChange=${(/** @type {Event} */ e) =>
            onChange(/** @type {HTMLInputElement} */ (e.target).checked)}
        />
        <span class="kani-toggle__track"></span>
      </label>
    <//>
  `;
}

/**
 * Number input row. Renders hold-to-repeat stepper buttons (the `NumberInput`
 * component) when the range is small and bounded, or `stepper` forces it;
 * otherwise a plain numeric field (nobody holds a button to reach 2592000).
 * @param {{ label: string, description?: string, badge?: string, tooltip?: string, id?: string, value: any, min?: number, max?: number, stepper?: boolean, onChange: (v: number) => void }} props
 */
export function NumberRow({ label, description, badge, tooltip, id, value, min, max, stepper, onChange }) {
  const useStepper = stepper ?? (max != null && max - (min ?? 0) <= 100);
  const control = useStepper
    ? html`<${NumberInput}
        value=${value ?? 0}
        min=${min}
        max=${max}
        ariaLabel=${label}
        onChange=${onChange}
      />`
    : html`<input
        type="number"
        inputMode="numeric"
        id=${id}
        aria-label=${label}
        class="input w-24 text-sm"
        value=${value ?? ''}
        min=${min ?? undefined}
        max=${max ?? undefined}
        onChange=${(/** @type {Event} */ e) =>
          onChange(Number(/** @type {HTMLInputElement} */ (e.target).value))}
      />`;
  return html`
    <${SettingsRow} label=${label} description=${description} badge=${badge} tooltip=${tooltip}>
      ${control}
    <//>
  `;
}

/**
 * Single-select row rendered as an accessible segmented control (radiogroup
 * with roving tabindex + arrow-key navigation).
 * @param {{ label: string, description?: string, tooltip?: string, options: { value: string, label: string }[], value: string, onChange: (v: string) => void }} props
 */
export function SelectRow({ label, description, tooltip, options, value, onChange }) {
  const [current, setCurrent] = useState(value);

  const pick = (/** @type {string} */ val) => {
    if (val === current) return;
    setCurrent(val);
    onChange(val);
  };

  const onKeyDown = (/** @type {KeyboardEvent} */ e, /** @type {number} */ i) => {
    let idx = -1;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') idx = (i + 1) % options.length;
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') idx = (i - 1 + options.length) % options.length;
    else if (e.key === 'Home') idx = 0;
    else if (e.key === 'End') idx = options.length - 1;
    else return;
    e.preventDefault();
    pick(options[idx].value);
  };

  return html`
    <${SettingsRow} label=${label} description=${description} tooltip=${tooltip}>
      <div class="flex gap-1.5 shrink-0 flex-wrap" role="radiogroup" aria-label=${label}>
        ${options.map((opt, i) => {
          const on = opt.value === current;
          return html`
            <button
              type="button"
              role="radio"
              class=${on ? 'chip chip-active' : 'chip'}
              aria-checked=${String(on)}
              tabindex=${on ? 0 : -1}
              onClick=${() => pick(opt.value)}
              onKeyDown=${(/** @type {KeyboardEvent} */ e) => onKeyDown(e, i)}
            >
              ${opt.label}
            </button>
          `;
        })}
      </div>
    <//>
  `;
}
