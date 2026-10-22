// @ts-check
// Settings — library-wide scanlator defaults. Per-manga preferences still win;
// these only fill the gaps.

import { h } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { t } from '../../i18n.js';
import { SettingsGroup, SettingsRow } from './_shared.js';
import { showApiError, showToast } from '../../components/toast.js';
import { showConfirm } from '../../components/modal.js';
import { useBusy } from '../../hooks/use-busy.js';

const html = htm.bind(h);

export function ScanlatorsSection() {
  const [prefs, setPrefs] = useState(/** @type {any[] | null} */ (null));
  const [known, setKnown] = useState(/** @type {any[]} */ ([]));
  const [picked, setPicked] = useState('');
  const { busy, run } = useBusy();

  const load = useCallback(async () => {
    try {
      const [p, k] = await Promise.all([
        api.getGlobalScanlatorPrefs(),
        api.getKnownScanlators(),
      ]);
      setPrefs(p);
      setKnown(k);
    } catch (e) {
      showApiError(e);
      setPrefs([]);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const listed = new Set((prefs ?? []).map((p) => p.scanlator));
  const available = known.filter((k) => !listed.has(k.scanlator));

  const save = (/** @type {string} */ scanlator, /** @type {number} */ priority, /** @type {boolean} */ blocked) =>
    run(async () => {
      try {
        await api.setGlobalScanlatorPref(scanlator, priority, blocked);
        await load();
      } catch (e) {
        showApiError(e);
      }
    });

  const add = async () => {
    if (!picked) return;
    const top = Math.max(0, ...(prefs ?? []).map((p) => p.priority));
    await save(picked, top + 1, false);
    setPicked('');
  };

  const move = async (/** @type {any} */ pref, /** @type {number} */ delta) => {
    const ordered = (prefs ?? []).filter((p) => !p.blocked);
    const i = ordered.findIndex((p) => p.id === pref.id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= ordered.length) return;
    await save(pref.scanlator, ordered[j].priority, false);
    await save(ordered[j].scanlator, pref.priority, false);
  };

  const remove = async (/** @type {any} */ pref) => {
    const ok = await showConfirm(t('settings.scanlators.remove.confirm', { name: pref.scanlator }), {
      title: t('settings.scanlators.remove'),
      confirmLabel: t('common.remove'),
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteScanlatorPref(pref.id);
      showToast(t('common.saved'), { type: 'success' });
      await load();
    } catch (e) {
      showApiError(e);
    }
  };

  const preferred = (prefs ?? []).filter((p) => !p.blocked);
  const blocked = (prefs ?? []).filter((p) => p.blocked);

  const row = (/** @type {any} */ pref, /** @type {number} */ idx, /** @type {boolean} */ ordered) => html`
    <${SettingsRow}
      key=${pref.id}
      label=${pref.scanlator}
      description=${ordered ? t('settings.scanlators.rank', { n: idx + 1 }) : null}
    >
      <div class="flex items-center gap-1">
        ${ordered
          ? html`<button
                type="button"
                class="btn-ghost btn-sm"
                disabled=${busy || idx === 0}
                onClick=${() => move(pref, -1)}
                aria-label=${t('settings.scanlators.up')}
              >
                ↑
              </button>
              <button
                type="button"
                class="btn-ghost btn-sm"
                disabled=${busy || idx === preferred.length - 1}
                onClick=${() => move(pref, 1)}
                aria-label=${t('settings.scanlators.down')}
              >
                ↓
              </button>`
          : null}
        <button
          type="button"
          class="btn-ghost btn-sm"
          disabled=${busy}
          onClick=${() => save(pref.scanlator, pref.priority, !pref.blocked)}
        >
          ${pref.blocked ? t('settings.scanlators.unblock') : t('settings.scanlators.block')}
        </button>
        <button type="button" class="btn-ghost btn-sm text-danger" onClick=${() => remove(pref)}>
          ${t('common.remove')}
        </button>
      </div>
    <//>
  `;

  return html`
    <div class="flex flex-col gap-6">
      <${SettingsGroup} label=${t('settings.scanlators.preferred.group')}>
        <p class="text-xs text-text-muted px-4 py-2">${t('settings.scanlators.desc')}</p>
        ${preferred.length === 0
          ? html`<p class="px-4 py-3 text-sm text-text-muted">
              ${t('settings.scanlators.none')}
            </p>`
          : preferred.map((p, i) => row(p, i, true))}
        <${SettingsRow}
          label=${t('settings.scanlators.add.label')}
          description=${t('settings.scanlators.add.desc')}
        >
          <div class="flex items-center gap-2">
            <select
              class="input text-sm w-auto max-w-56"
              value=${picked}
              onChange=${(/** @type {any} */ e) => setPicked(e.target.value)}
            >
              <option value="">${t('settings.scanlators.add.placeholder')}</option>
              ${available.map(
                (k) => html`<option key=${k.scanlator} value=${k.scanlator}>
                  ${k.scanlator} (${k.chapters})
                </option>`,
              )}
            </select>
            <button
              type="button"
              class="btn-secondary btn-sm"
              disabled=${busy || !picked}
              onClick=${add}
            >
              ${t('settings.scanlators.add')}
            </button>
          </div>
        <//>
      <//>

      ${blocked.length > 0
        ? html`<${SettingsGroup} label=${t('settings.scanlators.blocked.group')}>
            <p class="text-xs text-text-muted px-4 py-2">
              ${t('settings.scanlators.blocked.desc')}
            </p>
            ${blocked.map((p, i) => row(p, i, false))}
          <//>`
        : null}
    </div>
  `;
}
