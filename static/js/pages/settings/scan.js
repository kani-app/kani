// @ts-check

import { h } from 'preact';
import { useState, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { showToast } from '../../components/toast.js';
import { SettingsGroup, ToggleRow, NumberRow, SelectRow } from './_shared.js';
import { useSettingsForm } from './form-bus.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

const AXES = ['resolution', 'colour', 'encoder', 'bitrate'];
const AXIS_RULES = ['off', 'gain', 'both'];
const AUTO_REPLACE_REASONS = [
  'preferred_scanlator',
  'resolution',
  'colour',
  'encoder',
  'bitrate',
];

/** @param {{ settings: any }} props */
export function ScanSection({ settings }) {
  const initial = {
    auto_scan: !!settings?.auto_scan,
    scan_interval_minutes: settings?.scan_interval_minutes ?? 60,
    scan_exclude_completed: !!settings?.scan_exclude_completed,
    upgrade_detection_enabled: settings?.upgrade_detection_enabled ?? true,
    upgrade_min_res_gain: Number(settings?.upgrade_min_res_gain ?? 1.2),
    upgrade_confirm_fetches: Number(settings?.upgrade_confirm_fetches ?? 3),
    upgrade_axis_resolution: settings?.upgrade_axis_resolution ?? 'both',
    upgrade_axis_colour: settings?.upgrade_axis_colour ?? 'both',
    upgrade_axis_encoder: settings?.upgrade_axis_encoder ?? 'both',
    upgrade_axis_bitrate: settings?.upgrade_axis_bitrate ?? 'gain',
    upgrade_show_downgrades: !!settings?.upgrade_show_downgrades,
    upgrade_auto_replace_reasons:
      settings?.upgrade_auto_replace_reasons ?? 'preferred_scanlator,resolution,colour',
    scan_barren_page_tolerance: Number(settings?.scan_barren_page_tolerance ?? 3),
  };
  const [form, setForm] = useState(initial);
  const [saved, setSaved] = useState(initial);

  const save = useCallback(async () => {
    await api.updateSettings({ Scan: form });
    setSaved(form);
    showToast(t('common.saved'), { type: 'success' });
  }, [form]);

  useSettingsForm({ current: form, saved, save, reset: () => setForm(saved) });

  const set = (/** @type {string} */ k, /** @type {any} */ v) =>
    setForm((f) => ({ ...f, [k]: v }));

  const reasonSet = new Set(
    String(form.upgrade_auto_replace_reasons)
      .split(',')
      .map((r) => r.trim())
      .filter(Boolean),
  );
  const toggleReason = (/** @type {string} */ reason, /** @type {boolean} */ on) => {
    const next = new Set(reasonSet);
    if (on) next.add(reason);
    else next.delete(reason);
    // Keep a stable order so the saved string does not churn on every toggle.
    set(
      'upgrade_auto_replace_reasons',
      AUTO_REPLACE_REASONS.filter((r) => next.has(r)).join(','),
    );
  };

  return html`
    <${SettingsGroup} label=${t('settings.scan.group')}>
      <${ToggleRow}
        label=${t('settings.scan.auto.label')}
        description=${t('settings.scan.auto.desc')}
        checked=${form.auto_scan}
        onChange=${(v) => set('auto_scan', v)}
      />
      ${form.auto_scan &&
      html`<${NumberRow}
        label=${t('settings.scan.interval.label')}
        description=${t('settings.scan.interval.desc')}
        value=${form.scan_interval_minutes}
        min=${1}
        onChange=${(v) => set('scan_interval_minutes', v)}
      />`}
      <${ToggleRow}
        label=${t('settings.scan.exclude.label')}
        description=${t('settings.scan.exclude.desc')}
        checked=${form.scan_exclude_completed}
        onChange=${(v) => set('scan_exclude_completed', v)}
      />
      <${NumberRow}
        label=${t('settings.scan.barren_pages.label')}
        description=${t('settings.scan.barren_pages.desc')}
        value=${form.scan_barren_page_tolerance}
        min=${1}
        max=${20}
        stepper=${true}
        onChange=${(v) => set('scan_barren_page_tolerance', v)}
      />
    <//>

    <${SettingsGroup} label=${t('settings.upgrades.group')}>
      <${ToggleRow}
        label=${t('settings.upgrades.enabled.label')}
        description=${t('settings.upgrades.enabled.desc')}
        checked=${form.upgrade_detection_enabled}
        onChange=${(v) => set('upgrade_detection_enabled', v)}
      />
      ${form.upgrade_detection_enabled &&
      html`<${NumberRow}
          label=${t('settings.upgrades.min_gain.label')}
          description=${t('settings.upgrades.min_gain.desc')}
          value=${form.upgrade_min_res_gain}
          min=${1}
          max=${5}
          step=${0.1}
          onChange=${(v) => set('upgrade_min_res_gain', v)}
        />
        <${NumberRow}
          label=${t('settings.upgrades.confirm_fetches.label')}
          description=${t('settings.upgrades.confirm_fetches.desc')}
          value=${form.upgrade_confirm_fetches}
          min=${0}
          stepper=${true}
          onChange=${(v) => set('upgrade_confirm_fetches', v)}
        />
        <${ToggleRow}
          label=${t('settings.upgrades.show_downgrades.label')}
          description=${t('settings.upgrades.show_downgrades.desc')}
          checked=${form.upgrade_show_downgrades}
          onChange=${(v) => set('upgrade_show_downgrades', v)}
        />`}
    <//>

    ${form.upgrade_detection_enabled &&
    html`<${SettingsGroup} label=${t('settings.upgrades.axes.group')}>
        ${AXES.map(
          (axis) => html`
            <${SelectRow}
              key=${axis}
              label=${t(`settings.upgrades.axis.${axis}.label`)}
              description=${t(`settings.upgrades.axis.${axis}.desc`)}
              options=${AXIS_RULES.map((v) => ({ value: v, label: t(`settings.upgrades.rule.${v}`) }))}
              value=${form[`upgrade_axis_${axis}`]}
              onChange=${(v) => set(`upgrade_axis_${axis}`, v)}
            />
          `,
        )}
      <//>

      <${SettingsGroup} label=${t('settings.upgrades.auto_replace.group')}>
        <p class="text-xs text-text-muted px-1 pb-1">
          ${t('settings.upgrades.auto_replace.desc')}
        </p>
        ${AUTO_REPLACE_REASONS.map(
          (reason) => html`
            <${ToggleRow}
              key=${reason}
              label=${t(`settings.upgrades.auto_replace.${reason}.label`)}
              description=${t(`settings.upgrades.auto_replace.${reason}.desc`)}
              checked=${reasonSet.has(reason)}
              onChange=${(v) => toggleReason(reason, v)}
            />
          `,
        )}
      <//>`}
  `;
}
