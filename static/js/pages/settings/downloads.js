// @ts-check

import { h } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { getLocal, setLocal } from '../../utils.js';
import { showToast } from '../../components/toast.js';
import { SettingsGroup, SettingsRow, ToggleRow, NumberRow } from './_shared.js';
import { useSettingsForm } from './form-bus.js';
import { Combobox } from '../../components/combobox.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

const FIELDS = [
  { key: 'concurrent_page_downloads', label: 'settings.downloads.concurrent_pages', desc: 'settings.downloads.concurrent_pages.desc', min: 1 },
  { key: 'per_source_download_concurrency', label: 'settings.downloads.per_source_concurrency', desc: 'settings.downloads.per_source_concurrency.desc', min: 1 },
  { key: 'scan_concurrency', label: 'settings.downloads.scan_concurrency', desc: 'settings.downloads.scan_concurrency.desc', min: 1 },
  { key: 'max_retries', label: 'settings.downloads.max_retries', desc: 'settings.downloads.max_retries.desc', min: 0 },
  { key: 'initial_retry_delay_ms', label: 'settings.downloads.initial_retry_delay', desc: 'settings.downloads.initial_retry_delay.desc', min: 0 },
];

function CategoryPicker({ value, onChange }) {
  const [cats, setCats] = useState(/** @type {any[] | null} */ (null));
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    api
      .getCategories()
      .then((c) => setCats(Array.isArray(c) ? c.map((x) => ({ id: x.id, name: x.name })) : []))
      .catch(() => setFailed(true));
  }, []);

  if (failed) {
    return html`<p class="text-text-muted text-xs">${t('settings.downloads.categories.load_failed')}</p>`;
  }
  if (cats === null) return html`<p class="text-text-muted text-xs">${t('common.loading')}</p>`;
  if (cats.length === 0) {
    return html`<p class="text-text-muted text-xs">${t('settings.downloads.categories.empty')}</p>`;
  }
  return html`
    <div class="w-64">
      <${Combobox}
        options=${cats}
        value=${value}
        onChange=${onChange}
        multiple=${true}
        placeholder=${t('settings.downloads.categories.placeholder')}
      />
    </div>
  `;
}

function DownloadAheadGroup() {
  const [enabled, setEnabled] = useState(getLocal('kani_download_ahead_enabled') === 'true');
  const [count, setCount] = useState(Number(getLocal('kani_download_ahead_count') || '3') || 3);

  const toggle = (/** @type {boolean} */ v) => {
    setLocal('kani_download_ahead_enabled', String(v));
    setEnabled(v);
  };
  const onCount = (/** @type {number} */ v) => {
    const clamped = Math.max(1, Math.min(10, v || 3));
    setLocal('kani_download_ahead_count', String(clamped));
    setCount(clamped);
  };

  return html`
    <${SettingsGroup} label=${t('settings.downloads.ahead.group')}>
      <${ToggleRow}
        label=${t('settings.downloads.ahead.enable')}
        description=${t('settings.downloads.ahead.enable.desc')}
        checked=${enabled}
        onChange=${toggle}
      />
      ${enabled &&
      html`<${NumberRow}
        label=${t('settings.downloads.ahead.count')}
        description=${t('settings.downloads.ahead.count.desc')}
        value=${count}
        min=${1}
        max=${10}
        onChange=${onCount}
      />`}
    <//>
  `;
}

/** @param {{ settings: any }} props */
export function DownloadsSection({ settings }) {
  const initialNums = Object.fromEntries(FIELDS.map((f) => [f.key, Number(settings?.[f.key] ?? 0)]));
  const initialCats = (
    Array.isArray(settings?.auto_download_category_ids) ? settings.auto_download_category_ids : []
  )
    .slice()
    .sort((a, b) => a - b);

  const [nums, setNums] = useState(initialNums);
  const [catIds, setCatIds] = useState(initialCats);
  const [savedNums, setSavedNums] = useState(initialNums);
  const [savedCats, setSavedCats] = useState(initialCats);

  const current = { ...nums, auto_download_category_ids: [...catIds].sort((a, b) => a - b) };
  const saved = { ...savedNums, auto_download_category_ids: [...savedCats].sort((a, b) => a - b) };

  const save = useCallback(async () => {
    await api.updateSettings({ Download: current });
    setSavedNums(nums);
    setSavedCats(catIds);
    showToast(t('common.saved'), { type: 'success' });
    // eslint-disable-next-line
  }, [nums, catIds]);

  useSettingsForm({
    current,
    saved,
    save,
    reset: () => {
      setNums(savedNums);
      setCatIds(savedCats);
    },
  });

  return html`
    <${SettingsGroup} label=${t('settings.downloads.server_group')}>
      ${FIELDS.map(
        (f) => html`
          <${NumberRow}
            key=${f.key}
            label=${t(f.label)}
            description=${t(f.desc)}
            tooltip=${f.tooltip ? t(f.tooltip) : undefined}
            value=${nums[f.key]}
            min=${f.min}
            onChange=${(v) => setNums((n) => ({ ...n, [f.key]: v }))}
          />
        `,
      )}
      <${SettingsRow}
        label=${t('settings.downloads.auto_download_categories')}
        description=${t('settings.downloads.auto_download_categories.desc')}
      >
        <${CategoryPicker} value=${catIds} onChange=${setCatIds} />
      <//>
    <//>
    <${DownloadAheadGroup} />
  `;
}
