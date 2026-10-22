// @ts-check
// Settings — Offline Reading. Preferences are localStorage-only.
// (OPDS access lives in Settings → Clients & API tokens.)

import { h } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import { getLocal, setLocal } from '../../utils.js';
import { SettingsGroup, SettingsRow, SelectRow, NumberRow } from './_shared.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/** @returns {Promise<number|null>} */
async function estimateCacheSize() {
  if (!('caches' in window)) return null;
  try {
    const cache = await caches.open('kani-pages-v1');
    const keys = await cache.keys();
    if (keys.length === 0) return 0;
    let total = 0;
    for (const req of keys) {
      const resp = await cache.match(req);
      if (!resp) continue;
      const buf = await resp.clone().arrayBuffer().catch(() => null);
      if (buf) total += buf.byteLength;
    }
    return Math.round(total / (1024 * 1024));
  } catch {
    return null;
  }
}

export function OfflineSection() {
  const [mode, setMode] = useState(getLocal('kani_offline_mode') || 'off');
  const [scope, setScope] = useState(getLocal('kani_offline_scope') || 'mine');
  const [filter, setFilter] = useState(getLocal('kani_offline_filter') || 'unread');
  const [nextN, setNextN] = useState(Number(getLocal('kani_offline_next_n') || '5') || 5);
  const [maxMb, setMaxMb] = useState(getLocal('kani_offline_max_mb') || '');
  const [cacheMb, setCacheMb] = useState(/** @type {number | null | undefined} */ (undefined));
  const [clearing, setClearing] = useState(false);

  useEffect(() => {
    estimateCacheSize().then(setCacheMb);
  }, []);

  const pick = (/** @type {string} */ key, /** @type {(v:string)=>void} */ set) => (
    /** @type {string} */ v,
  ) => {
    setLocal(key, v);
    set(v);
  };

  const onNextN = (/** @type {number} */ v) => {
    const clamped = Math.max(1, Math.min(100, v || 5));
    setLocal('kani_offline_next_n', String(clamped));
    setNextN(clamped);
  };

  const onMaxMb = (/** @type {number} */ v) => {
    if (!v || Number.isNaN(v) || v <= 0) {
      setLocal('kani_offline_max_mb', '');
      setMaxMb('');
    } else {
      setLocal('kani_offline_max_mb', String(v));
      setMaxMb(String(v));
    }
  };

  const clearCache = async () => {
    setClearing(true);
    try {
      if ('caches' in window) await caches.delete('kani-pages-v1');
      setCacheMb(0);
    } finally {
      setTimeout(() => setClearing(false), 1200);
    }
  };

  const cacheText =
    cacheMb === undefined
      ? t('settings.offline.cache.calculating')
      : cacheMb === null
      ? t('settings.offline.cache.unknown')
      : t('settings.offline.cache.size', { mb: cacheMb });

  return html`
    <${SettingsGroup} label=${t('settings.offline.group')}>
      <${SelectRow}
        label=${t('settings.offline.mode.label')}
        description=${t('settings.offline.mode.desc')}
        value=${mode}
        onChange=${pick('kani_offline_mode', setMode)}
        options=${[
          { value: 'off', label: t('settings.offline.mode.off') },
          { value: 'auto', label: t('settings.offline.mode.auto') },
          { value: 'manual', label: t('settings.offline.mode.manual') },
        ]}
      />
      ${mode === 'auto' &&
      html`
        <${SelectRow}
          label=${t('settings.offline.scope.label')}
          description=${t('settings.offline.scope.desc')}
          value=${scope}
          onChange=${pick('kani_offline_scope', setScope)}
          options=${[
            { value: 'mine', label: t('settings.offline.scope.mine') },
            { value: 'all', label: t('settings.offline.scope.all') },
          ]}
        />
        <${SelectRow}
          label=${t('settings.offline.filter.label')}
          description=${t('settings.offline.filter.desc')}
          value=${filter}
          onChange=${pick('kani_offline_filter', setFilter)}
          options=${[
            { value: 'all', label: t('settings.offline.filter.all') },
            { value: 'unread', label: t('settings.offline.filter.unread') },
            { value: 'next', label: t('settings.offline.filter.next') },
          ]}
        />
        ${filter === 'next' &&
        html`<${NumberRow}
          label=${t('settings.offline.ahead.label')}
          description=${t('settings.offline.ahead.desc')}
          value=${nextN}
          min=${1}
          max=${100}
          onChange=${onNextN}
        />`}
      `}
      <${NumberRow}
        label=${t('settings.offline.max_mb.label')}
        description=${t('settings.offline.max_mb.desc')}
        value=${maxMb === '' ? '' : Number(maxMb)}
        min=${1}
        stepper=${false}
        onChange=${onMaxMb}
      />
    <//>

    <${SettingsGroup} label=${t('settings.offline.cache.group')}>
      <${SettingsRow}
        label=${t('settings.offline.cache.size_row.label')}
        description=${t('settings.offline.cache.size_row.desc')}
      >
        <span class="text-sm text-text-muted">${cacheText}</span>
      <//>
      <${SettingsRow}
        label=${t('settings.offline.cache.clear_row.label')}
        description=${t('settings.offline.cache.clear_row.desc')}
      >
        <button type="button" class="btn-danger btn-sm" disabled=${clearing} onClick=${clearCache}>
          ${clearing ? t('settings.offline.cache.clearing') : t('settings.offline.cache.clear_btn')}
        </button>
      <//>
    <//>

    <p class="text-xs text-text-muted">${t('settings.offline.footer')}</p>
  `;
}
