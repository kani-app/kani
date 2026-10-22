// @ts-check

import { h } from 'preact';
import { useRef } from 'preact/hooks';
import htm from 'htm';
import { t } from '../../i18n.js';

const html = htm.bind(h);

const REPEAT_DELAY = 400;
const REPEAT_INTERVAL = 70;

/**
 * @param {{
 *   value: number | string,
 *   onChange: (value: number) => void,
 *   min?: number,
 *   max?: number,
 *   step?: number,
 *   disabled?: boolean,
 *   ariaLabel?: string,
 *   class?: string,
 * }} props
 */
export function NumberInput({ value, onChange, min, max, step = 1, disabled = false, ariaLabel, class: klass = '' }) {
  const inputRef = useRef(/** @type {HTMLInputElement|null} */ (null));
  const timerRef = useRef(/** @type {any} */ (null));

  const clamp = (/** @type {number} */ n) => {
    if (min != null && n < min) n = min;
    if (max != null && n > max) n = max;
    return n;
  };

  const commit = (/** @type {number} */ n) => {
    if (Number.isNaN(n)) return;
    onChange(clamp(n));
  };

  const nudge = (/** @type {number} */ dir) => {
    const cur = Number(inputRef.current?.value ?? value) || 0;
    const next = clamp(cur + dir * step);
    if (inputRef.current) inputRef.current.value = String(next);
    onChange(next);
  };

  const startRepeat = (/** @type {number} */ dir) => {
    if (disabled) return;
    nudge(dir);
    const begin = Date.now();
    timerRef.current = setInterval(() => {
      if (Date.now() - begin >= REPEAT_DELAY) nudge(dir);
    }, REPEAT_INTERVAL);
  };

  const stopRepeat = () => {
    if (timerRef.current) { clearInterval(timerRef.current); timerRef.current = null; }
  };

  const stepBtn = (/** @type {number} */ dir, /** @type {string} */ glyph, /** @type {string} */ label) => html`
    <button
      type="button"
      tabindex="-1"
      class="num-step"
      aria-label=${label}
      disabled=${disabled}
      onPointerDown=${(/** @type {PointerEvent} */ e) => { e.preventDefault(); startRepeat(dir); }}
      onPointerUp=${stopRepeat}
      onPointerLeave=${stopRepeat}
      onPointerCancel=${stopRepeat}
    >${glyph}</button>
  `;

  return html`
    <div class=${'num-input ' + klass}>
      ${stepBtn(-1, '−', t('form.number.decrement'))}
      <input
        ref=${inputRef}
        type="number"
        class="input"
        value=${String(value)}
        min=${min}
        max=${max}
        step=${step}
        disabled=${disabled}
        aria-label=${ariaLabel}
        onChange=${(/** @type {any} */ e) => commit(Number(e.target.value))}
      />
      ${stepBtn(1, '+', t('form.number.increment'))}
    </div>
  `;
}
