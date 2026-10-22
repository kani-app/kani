// @ts-check
import { h } from 'preact';
import htm from 'htm';
const html = htm.bind(h);

/** @param {'open' | 'half_open' | 'closed'} state */
function dotClass(state) {
  if (state === 'open') return 'bg-danger';
  if (state === 'closed') return 'bg-success';
  return 'bg-warn';
}

/**
 * A colour-coded status dot with an accessible label so colour is not the sole signal.
 * @param {{ state: 'open' | 'half_open' | 'closed', label: string }} props
 */
export function StatusDot({ state, label }) {
  return html`<span
    role="img"
    aria-label=${'Status: ' + label}
    class=${'w-2 h-2 rounded-full shrink-0 ' + dotClass(state)}
  ></span>`;
}

