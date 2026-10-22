// @ts-check

import { h } from 'preact';
import htm from 'htm';
import { formatRelativeTime } from '../utils.js';
import { t } from '../i18n.js';
const html = htm.bind(h);

/**
 * @param {{
 *   events: Array<{ at: string, kind: string, description: string }>,
 *   loading?: boolean,
 *   error?: string | null,
 *   hasMore?: boolean,
 *   onLoadMore?: () => void,
 * }} props
 */
export function ActivityFeed({ events, loading = false, error = null, hasMore = false, onLoadMore }) {
  if (error) {
    return html`<p class="meta px-3 py-4">${error}</p>`;
  }

  if (!loading && events.length === 0) {
    return html`<p class="meta px-3 py-4">${t('activity_feed.empty')}</p>`;
  }

  return html`
    <div>
      ${events.map(ev => html`
        <div class="flex items-start gap-3 px-3 py-2 border-b border-border-subtle last:border-0" key=${ev.at + ev.kind}>
          <span class="meta shrink-0 w-14 text-right">${_shortTime(ev.at)}</span>
          <span class="text-sm text-text flex-1">${ev.description}</span>
          <span class="badge badge-muted shrink-0">${ev.kind}</span>
        </div>
      `)}
      ${loading ? html`<p class="meta px-3 py-3">${t('common.loading')}</p>` : null}
      ${!loading && hasMore ? html`
        <button type="button" class="btn-ghost btn-sm w-full mt-2" onClick=${onLoadMore}>
          ${t('activity_feed.load_more')}
        </button>
      ` : null}
    </div>
  `;
}

/**
 * Returns a compact relative-time string ("12m", "3h", "Yesterday", "Jan 5").
 * @param {string} isoDate
 * @returns {string}
 */
function _shortTime(isoDate) {
  try {
    const d = new Date(isoDate);
    if (isNaN(d.getTime())) return '';
    const diffMs = Date.now() - d.getTime();
    const diffMin = Math.floor(diffMs / 60000);
    if (diffMin < 1)  return t('activity_feed.time.now');
    if (diffMin < 60) return `${diffMin}m`;
    const diffHr = Math.floor(diffMin / 60);
    if (diffHr  < 24) return `${diffHr}h`;
    const diffDay = Math.floor(diffHr / 24);
    if (diffDay === 1) return t('activity_feed.time.yesterday');
    if (diffDay < 7)   return `${diffDay}d`;
    return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
  } catch { return ''; }
}
