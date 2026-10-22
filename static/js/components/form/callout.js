// @ts-check
// Callout — inline emphasis block for warnings and notices that must not read
// as body copy (ink-and-stamp: real warnings get real visual weight).

import { h, render } from 'preact';
import htm from 'htm';
import { Icon } from '../icon.js';
import { iconInfo, iconWarning } from '../../icons.js';

const html = htm.bind(h);

const TONES = {
  info:   { icon: iconInfo,    classes: 'border-border text-text-muted bg-surface-2' },
  warn:   { icon: iconWarning, classes: 'border-warn/40 text-warn bg-warn/10' },
  danger: { icon: iconWarning, classes: 'border-danger/40 text-danger bg-danger/10' },
};

/**
 * @param {{ tone?: 'info'|'warn'|'danger', children: any }} props
 */
export function Callout({ tone = 'info', children }) {
  const t = TONES[tone] ?? TONES.info;
  return html`
    <div class=${'flex items-start gap-2.5 px-3 py-2.5 rounded-lg border text-sm ' + t.classes} role=${tone === 'info' ? undefined : 'alert'}>
      <span class="icon-sm shrink-0 mt-0.5" aria-hidden="true"><${Icon} svg=${t.icon} /></span>
      <div class="min-w-0">${children}</div>
    </div>
  `;
}

/**
 * Imperative helper for vanilla call sites.
 * @param {{ tone?: 'info'|'warn'|'danger', text: string }} opts
 */
export function createCallout({ tone = 'info', text }) {
  const host = document.createElement('div');
  render(html`<${Callout} tone=${tone}>${text}<//>`, host);
  return host;
}
