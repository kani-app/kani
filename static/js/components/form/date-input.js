// @ts-check

import { h } from 'preact';
import htm from 'htm';

const html = htm.bind(h);

/**
 * @param {{
 *   label: string,
 *   value: string,
 *   onChange: (isoDate: string) => void,
 *   disabled?: boolean,
 *   class?: string,
 * }} props
 */
export function DateInput({ label, value, onChange, disabled = false, class: klass = '' }) {
  return html`
    <label class=${'flex items-center gap-2 text-sm text-text-muted ' + klass}>
      <span class="shrink-0">${label}</span>
      <input
        type="date"
        class="input input-sm w-auto"
        value=${value}
        disabled=${disabled}
        onChange=${(/** @type {any} */ e) => onChange(e.target.value)}
      />
    </label>
  `;
}
