// @ts-check
// Password strength indicator — debounced server-side zxcvbn + HIBP check.

import { h } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { t } from '../i18n.js';

const html = htm.bind(h);

const SCORE_LABEL_KEYS = [
  'pwstrength.score.very_weak',
  'pwstrength.score.weak',
  'pwstrength.score.fair',
  'pwstrength.score.strong',
  'pwstrength.score.very_strong',
];
const SCORE_COLORS = [
  'bg-danger',
  'bg-danger',
  'bg-warn',
  'bg-success',
  'bg-success',
];

/**
 * @param {{ password: string, identity?: string }} props
 */
export function PasswordStrength({ password, identity = '' }) {
  const [result, setResult] = useState(/** @type {any} */ (null));

  useEffect(() => {
    if (!password || password.length < 4) {
      setResult(null);
      return;
    }
    const timer = setTimeout(() => {
      api.checkPasswordStrength(password, identity)
        .then(setResult)
        .catch(() => setResult(null));
    }, 300);
    return () => clearTimeout(timer);
  }, [password, identity]);

  if (!result) return null;

  const score = result.score ?? 0;
  const label = SCORE_LABEL_KEYS[score] ? t(SCORE_LABEL_KEYS[score]) : t('pwstrength.score.unknown');
  const color = SCORE_COLORS[score] ?? 'bg-danger';

  return html`
    <div class="flex flex-col gap-1.5 mt-1.5">
      <div class="flex items-center gap-2">
        <div class="flex-1 flex gap-0.5 h-1.5">
          ${[0,1,2,3].map(i => html`
            <div key=${i} class="flex-1 rounded-full ${i <= score - 1 ? color : 'bg-surface-3'} transition-colors"></div>
          `)}
        </div>
        <span class="text-xs text-text-muted shrink-0">${label}</span>
      </div>
      ${result.feedback?.length > 0 && html`
        <p class="text-xs text-text-muted">${result.feedback[0]}</p>
      `}
      ${result.pwned && html`
        <p class="text-xs text-danger">
          ${result.pwned_count ? t('pwstrength.pwned.with_count', { count: result.pwned_count.toLocaleString() }) : t('pwstrength.pwned.plain')}
        </p>
      `}
    </div>
  `;
}
