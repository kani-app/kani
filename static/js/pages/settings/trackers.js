// @ts-check

import { h } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { showConfirm } from '../../components/modal.js';
import { showToast, showApiError } from '../../components/toast.js';
import { hasPermission } from '../../session.js';
import { SettingsGroup, SettingsRow, ToggleRow, NumberRow } from './_shared.js';
import { skeletonSettingsCards } from '../../components/skeletons.js';
import { ErrorState } from '../../components/error-state.js';
import { useBusy } from '../../hooks/use-busy.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/**
 * @param {number} trackerId @param {string} trackerName @param {() => void} onLinked
 */
function openTrackerPopup(trackerId, trackerName, onLinked) {
  const redirectUri = `${location.origin}/rest/trackers/${trackerId}/callback`;
  api
    .getTrackerAuthUrl(trackerId, redirectUri)
    .then(({ url }) => {
      const popup = window.open(url, `link_${trackerName}`, 'popup,width=640,height=720');
      if (!popup) {
        showToast(t('trackers.error.popup_blocked'), { type: 'error' });
        return;
      }
      /** @param {MessageEvent} e */
      function onMessage(e) {
        if (e.origin !== location.origin) return;
        if (e.data?.type === 'tracker_linked') {
          window.removeEventListener('message', onMessage);
          clearInterval(closedTimer);
          popup.close();
          onLinked();
        }
      }
      window.addEventListener('message', onMessage);
      const closedTimer = setInterval(() => {
        if (popup.closed) {
          clearInterval(closedTimer);
          window.removeEventListener('message', onMessage);
        }
      }, 500);
    })
    .catch((e) => {
      showToast(e?.message ?? t('trackers.error.auth_url_failed', { name: trackerName }), {
        type: 'error',
      });
    });
}

function BehaviourGroup({ settings }) {
  const [tracking, setTracking] = useState({
    default_tracking_enabled: settings?.default_tracking_enabled ?? true,
    tracker_auto_sync_enabled: settings?.tracker_auto_sync_enabled ?? false,
    tracker_sync_interval_hours: settings?.tracker_sync_interval_hours ?? 24,
  });

  const [syncing, setSyncing] = useState(false);

  const syncNow = async () => {
    setSyncing(true);
    try {
      await api.syncAllTrackers();
      showToast(t('settings.trackers.sync_now.started'), { type: 'success' });
    } catch (e) {
      showApiError(e);
    } finally {
      setSyncing(false);
    }
  };

  const patch = async (/** @type {any} */ next) => {
    const prev = tracking;
    setTracking(next);
    try {
      await api.updateSettings({ Tracking: next });
    } catch (e) {
      setTracking(prev);
      showApiError(e);
    }
  };

  return html`
    <${SettingsGroup} label=${t('settings.trackers.behaviour')}>
      <${ToggleRow}
        label=${t('settings.trackers.default_enabled')}
        description=${t('settings.trackers.default_enabled_desc')}
        checked=${tracking.default_tracking_enabled}
        onChange=${(v) => patch({ ...tracking, default_tracking_enabled: v })}
      />
      <${ToggleRow}
        label=${t('settings.trackers.auto_sync')}
        description=${t('settings.trackers.auto_sync_desc')}
        checked=${tracking.tracker_auto_sync_enabled}
        onChange=${(v) => patch({ ...tracking, tracker_auto_sync_enabled: v })}
      />
      <${NumberRow}
        label=${t('settings.trackers.sync_interval')}
        description=${t('settings.trackers.sync_interval_desc')}
        value=${tracking.tracker_sync_interval_hours}
        min=${1}
        max=${168}
        onChange=${(v) => patch({ ...tracking, tracker_sync_interval_hours: v })}
      />
      <${SettingsRow}
        label=${t('settings.trackers.sync_now')}
        description=${t('settings.trackers.sync_now_desc')}
      >
        <button
          type="button"
          class="btn-secondary btn-sm"
          disabled=${syncing}
          onClick=${syncNow}
        >
          ${t('settings.trackers.sync_now.action')}
        </button>
      <//>
    <//>
  `;
}

function TrackerSetup({ tracker, onChanged }) {
  const [config, setConfig] = useState(/** @type {any} */ (undefined));
  const [clientId, setClientId] = useState('');
  const [secret, setSecret] = useState('');
  const { busy, run } = useBusy();

  useEffect(() => {
    api
      .getTrackerConfig(tracker.id)
      .then((c) => {
        setConfig(c);
        setClientId(c?.client_id ?? '');
      })
      .catch(() => setConfig(null));
  }, [tracker.id]);

  const isAniList = tracker.name === 'AniList';
  const isMAL = tracker.name === 'MyAnimeList';
  const redirect = `${location.origin}/rest/trackers/${tracker.id}/callback`;

  const save = () =>
    run(async () => {
      const id = clientId.trim();
      if (!id) {
        showToast(t('trackers.setup.client_id.required'), { type: 'error' });
        return;
      }
      try {
        const body = /** @type {any} */ ({ client_id: id });
        if (secret) body.client_secret = secret;
        await api.setTrackerConfig(tracker.id, body);
        showToast(t('common.saved'), { type: 'success' });
        onChanged();
      } catch (e) {
        showApiError(e);
      }
    });

  const del = async () => {
    if (
      !(await showConfirm(t('trackers.confirm.remove_creds.msg', { name: tracker.name }), {
        title: t('trackers.confirm.remove_creds.title'),
        danger: true,
      }))
    )
      return;
    try {
      await api.deleteTrackerConfig(tracker.id);
      onChanged();
    } catch (e) {
      showToast(e?.message ?? t('trackers.error.remove_failed'), { type: 'error' });
    }
  };

  return html`
    <${SettingsGroup} label=${t('trackers.setup.group')}>
      <div class="px-4 py-4 flex flex-col gap-3">
        ${isAniList &&
        html`<p class="text-xs text-text-muted leading-relaxed mb-2">
          ${t('trackers.setup.anilist.register')}
          <strong>anilist.co → Settings → Developer → Create New Client${/* i18n-ignore */ ''}</strong>.
          ${t('trackers.setup.anilist.redirect')}
          <code class="font-mono bg-surface-alt px-1 rounded">${redirect}</code>.
        </p>`}
        ${isMAL &&
        html`<p class="text-xs text-text-muted leading-relaxed mb-2">
          ${t('trackers.setup.mal.register')}
          <strong>myanimelist.net → Account Settings → API → Create ID${/* i18n-ignore */ ''}</strong>.
          ${t('trackers.setup.mal.app_type')}
          <strong>web${/* i18n-ignore */ ''}</strong> ${t('trackers.setup.mal.redirect')}
          <code class="font-mono bg-surface-alt px-1 rounded">${redirect}</code>.
        </p>`}
        <div class="flex flex-col gap-1">
          <label class="text-xs font-medium text-text">${t('trackers.setup.client_id')}</label>
          <input
            type="text"
            class="input text-sm font-mono"
            value=${clientId}
            placeholder=${t('trackers.setup.client_id.placeholder')}
            autocomplete="off"
            spellcheck="false"
            onInput=${(e) => setClientId(e.target.value)}
          />
        </div>
        ${isAniList &&
        html`<div class="flex flex-col gap-1">
          <label class="text-xs font-medium text-text">${t('trackers.setup.client_secret')}</label>
          <input
            type="password"
            class="input text-sm font-mono"
            placeholder=${config?.secret_configured
              ? t('trackers.setup.client_secret.already_set')
              : t('trackers.setup.client_secret.placeholder')}
            autocomplete="off"
            value=${secret}
            onInput=${(e) => setSecret(e.target.value)}
          />
          <p class="text-xs text-text-muted">${t('trackers.setup.client_secret.note')}</p>
        </div>`}
        <div class="flex items-center gap-2 flex-wrap">
          <button type="button" class="btn-secondary btn-sm" disabled=${busy} onClick=${save}>
            ${t('trackers.setup.save')}
          </button>
          ${config?.client_id &&
          html`<button type="button" class="btn-danger btn-sm" onClick=${del}>
            ${t('trackers.setup.remove')}
          </button>`}
        </div>
      </div>
    <//>
  `;
}

function TrackerCard({ tracker, isAdmin, onChanged }) {
  const unlink = async () => {
    if (
      !(await showConfirm(t('trackers.confirm.unlink.msg', { name: tracker.name }), {
        title: t('trackers.confirm.unlink.title'),
        danger: true,
      }))
    )
      return;
    try {
      await api.unlinkTracker(tracker.id);
      onChanged();
    } catch (e) {
      showToast(e?.message ?? t('trackers.error.unlink_failed'), { type: 'error' });
    }
  };

  const link = () => openTrackerPopup(tracker.id, tracker.name, onChanged);

  return html`
    <${SettingsGroup} label=${tracker.name}>
      ${tracker.configured
        ? html`<${SettingsRow}
            label=${tracker.needs_reauth
              ? t('trackers.needs_reauth_label')
              : tracker.linked
                ? t('trackers.linked_label')
                : t('trackers.not_linked_label')}
            description=${tracker.needs_reauth
              ? t('trackers.needs_reauth_desc', { name: tracker.name })
              : tracker.linked
                ? t('trackers.linked_desc', { name: tracker.name })
                : t('trackers.not_linked_desc', { name: tracker.name })}
          >
            ${tracker.needs_reauth
              ? html`<button type="button" class="btn-secondary btn-sm" onClick=${link}>
                  ${t('trackers.relink')}
                </button>`
              : html`<button
                  type="button"
                  class=${(tracker.linked ? 'btn-danger' : 'btn-secondary') + ' btn-sm'}
                  onClick=${tracker.linked ? unlink : link}
                >
                  ${tracker.linked ? t('trackers.unlink') : t('trackers.link')}
                </button>`}
          <//>`
        : html`<${SettingsRow}
            label=${t('trackers.not_configured')}
            description=${isAdmin
              ? t('trackers.not_configured.admin_desc')
              : t('trackers.not_configured.user_desc')}
          />`}
    <//>
    ${isAdmin && html`<${TrackerSetup} tracker=${tracker} onChanged=${onChanged} />`}
  `;
}

/** @param {{ settings: any }} props */
export function TrackersSection({ settings }) {
  const isAdmin = hasPermission('settings:edit_advanced');
  const [state, setState] = useState(
    /** @type {{ status: string, trackers: any[] }} */ ({ status: 'loading', trackers: [] }),
  );

  const load = useCallback(async () => {
    setState((s) => ({ ...s, status: 'loading' }));
    try {
      const trackers = await api.getTrackers();
      setState({ status: 'ready', trackers: Array.isArray(trackers) ? trackers : [] });
    } catch {
      setState({ status: 'error', trackers: [] });
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  if (state.status === 'loading') return html`${html([skeletonSettingsCards(3)])}`;
  if (state.status === 'error') {
    return html`<${ErrorState} message=${t('trackers.error.load_failed')} onRetry=${load} />`;
  }

  return html`
    <${BehaviourGroup} settings=${settings} />
    ${state.trackers.map(
      (tracker) => html`<${TrackerCard}
        key=${tracker.id}
        tracker=${tracker}
        isAdmin=${isAdmin}
        onChanged=${load}
      />`,
    )}
  `;
}
