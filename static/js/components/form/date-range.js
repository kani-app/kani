// @ts-check

import { h } from 'preact';
import htm from 'htm';
import { DateInput } from './date-input.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/**
 * @param {{
 *   from: string,
 *   to: string,
 *   onChange: (range: { from: string, to: string }) => void,
 * }} props
 */
export function DateRange({ from, to, onChange }) {
  return html`
    <${DateInput} label=${t('daterange.from')} value=${from}
      onChange=${(/** @type {string} */ v) => onChange({ from: v, to })} />
    <${DateInput} label=${t('daterange.to')} value=${to}
      onChange=${(/** @type {string} */ v) => onChange({ from, to: v })} />
  `;
}
