// @ts-check
// Session inventory component — shows active sessions with revoke capability.

import { h } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { showConfirm } from './modal.js';
import { t } from '../i18n.js';

const html = htm.bind(h);

/** Relative time formatter */
function relativeTime(unixTs) {
  const diff = Math.floor(Date.now() / 1000) - unixTs;
  if (diff < 60) return t('session.time.just_now');
  if (diff < 3600) return t('session.time.minutes_ago', { n: Math.floor(diff / 60) });
  if (diff < 86400) return t('session.time.hours_ago', { n: Math.floor(diff / 3600) });
  return t('session.time.days_ago', { n: Math.floor(diff / 86400) });
}

/** Parse a rough browser/OS label from a user-agent string */
function parseUA(ua) {
  if (!ua) return 'Unknown device';
  if (/iPhone|iPad/.test(ua)) return 'iOS device';
  if (/Android/.test(ua)) return 'Android device';
  if (/Firefox/.test(ua)) return 'Firefox';
  if (/Edg/.test(ua)) return 'Edge';
  if (/Chrome/.test(ua)) return 'Chrome';
  if (/Safari/.test(ua)) return 'Safari';
  return 'Browser';
}

export function SessionList() {
  const [sessions, setSessions] = useState(/** @type {any[]} */ ([]));
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(/** @type {string|null} */ (null));

  useEffect(() => {
    api.getSessions()
      .then(data => { setSessions(data.sessions ?? []); setLoading(false); })
      .catch(e => { setError(String(e?.message ?? t('session.error.load_failed'))); setLoading(false); });
  }, []);

  async function handleRevoke(id) {
    const ok = await showConfirm(t('session.confirm.revoke'));
    if (!ok) return;
    try {
      await api.revokeSession(id);
      setSessions(prev => prev.filter(s => s.id !== id));
    } catch (e) {
      alert(t('session.error.revoke_failed', { msg: e?.message ?? '' }));
    }
  }

  async function handleRevokeAll() {
    const ok = await showConfirm(t('session.confirm.revoke_all'));
    if (!ok) return;
    try {
      await api.revokeOtherSessions();
      setSessions(prev => prev.filter(s => s.is_current));
    } catch (e) {
      alert(t('session.error.revoke_all_failed', { msg: e?.message ?? '' }));
    }
  }

  if (loading) {
    return html`
      <div class="divide-y divide-border-subtle">
        ${[1,2,3].map(i => html`
          <div key=${i} class="flex items-center gap-3 px-3 py-3 animate-pulse">
            <div class="h-4 bg-surface-raised rounded w-32"></div>
            <div class="h-3 bg-surface-raised rounded w-20 ml-auto"></div>
          </div>
        `)}
      </div>
    `;
  }

  if (error) {
    return html`
      <div class="px-3 py-4 text-sm text-danger">${error}
        <button class="ml-2 text-accent underline" onClick=${() => { setLoading(true); setError(null); api.getSessions().then(d => { setSessions(d.sessions ?? []); setLoading(false); }).catch(e => { setError(String(e?.message)); setLoading(false); }); }}>${t('common.retry')}</button>
      </div>
    `;
  }

  if (sessions.length === 0) {
    return html`<div class="px-3 py-4 text-sm text-text-muted">${t('session.empty')}</div>`;
  }

  const otherCount = sessions.filter(s => !s.is_current).length;

  return html`
    <div>
      <div class="divide-y divide-border-subtle">
        ${sessions.map(s => html`
          <div key=${s.id} class="flex items-center gap-3 px-3 py-3">
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2">
                <span class="text-sm font-medium text-text truncate">${parseUA(s.user_agent)}</span>
                ${s.is_current && html`<span class="shrink-0 text-xs font-medium px-1.5 py-0.5 rounded bg-accent/15 text-accent">${t('session.current')}</span>`}
              </div>
              <div class="text-xs text-text-muted mt-0.5 truncate">
                ${s.ip_addr ? html`${s.ip_addr} · ` : ''}${t('session.last_active', { time: relativeTime(s.last_seen_at) })}
              </div>
            </div>
            ${!s.is_current && html`
              <button type="button" class="shrink-0 btn-ghost btn-sm text-danger"
                onClick=${() => handleRevoke(s.id)}>
                ${t('session.action.revoke')}
              </button>
            `}
          </div>
        `)}
      </div>
      ${otherCount > 0 && html`
        <div class="px-3 py-2 border-t border-border-subtle">
          <button type="button" class="btn-danger btn-sm w-full"
            onClick=${handleRevokeAll}>
            ${t('session.action.revoke_all', { count: otherCount })}
          </button>
        </div>
      `}
    </div>
  `;
}
