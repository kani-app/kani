// @ts-check
import { h } from 'preact';
import htm from 'htm';
const html = htm.bind(h);

/**
 * @param {{
 *   options: number[],
 *   value: number,
 *   onChange: (n: number) => void,
 *   ariaLabel: string,
 *   class?: string,
 * }} props
 */
export function PageSizeSelect({ options, value, onChange, ariaLabel, class: className = '' }) {
  return html`
    <select
      class="input ${className}"
      aria-label=${ariaLabel}
      value=${value}
      onChange=${(/** @type {Event} */ e) => onChange(Number(/** @type {HTMLSelectElement} */ (e.target).value))}
    >
      ${options.map(n => html`<option value=${n}>${n}</option>`)}
    </select>
  `;
}
