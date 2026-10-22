// @ts-check

import { h } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { ErrorState } from '../../components/error-state.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/**
 * @typedef {{
 *   id: string,
 *   titleKey: string,
 *   perm?: string | null,
 *   order?: number,
 *   span?: 1 | 2,
 *   Component: any,
 * }} DiagnosticsCardDef
 */

/** @type {DiagnosticsCardDef[]} */
const cards = [];

/** @param {DiagnosticsCardDef} def */
export function registerDiagnosticsCard(def) {
  const existing = cards.findIndex(c => c.id === def.id);
  if (existing >= 0) cards.splice(existing, 1);
  cards.push({ order: 100, span: 1, perm: null, ...def });
  cards.sort((a, b) => (a.order ?? 100) - (b.order ?? 100));
}

export function getDiagnosticsCards() {
  return cards.slice();
}

let cached = { token: -1, promise: null };

/**
 * Shared fetcher so the cards reading the diagnostics payload issue one
 * request per refresh rather than one each.
 * @param {number} refreshToken
 */
function fetchDiagnostics(refreshToken) {
  if (cached.token !== refreshToken || !cached.promise) {
    cached = { token: refreshToken, promise: api.getDiagnostics() };
  }
  return cached.promise;
}

/**
 * @param {{ refreshToken: number }} props
 */
export function useDiagnostics({ refreshToken }) {
  const [state, setState] = useState({ data: null, error: null });

  useEffect(() => {
    let cancelled = false;
    setState(s => ({ data: s.data, error: null }));
    fetchDiagnostics(refreshToken)
      .then(data => {
        if (!cancelled) setState({ data, error: null });
      })
      .catch(e => {
        if (!cancelled) setState({ data: null, error: e });
      });
    return () => {
      cancelled = true;
    };
  }, [refreshToken]);

  return state;
}

/**
 * @param {{ titleKey: string, span?: 1 | 2, loading?: boolean, error?: any, onRetry?: () => void, children?: any }} props
 */
export function DiagCard({ titleKey, span = 1, loading, error, onRetry, children }) {
  return html`
    <section
      class=${`card p-4 flex flex-col gap-3 ${span === 2 ? 'md:col-span-2' : ''}`}
      aria-label=${t(titleKey)}
    >
      <h3 class="text-sm font-semibold text-text">${t(titleKey)}</h3>
      ${error
        ? html`<${ErrorState} message=${error?.message ?? t('common.error_occurred')} onRetry=${onRetry} />`
        : loading
          ? html`<div class="flex flex-col gap-2" aria-busy="true">
              ${[0, 1, 2].map(
                i => html`<div key=${i} class="skeleton h-4 w-full rounded"></div>`
              )}
            </div>`
          : children}
    </section>
  `;
}
