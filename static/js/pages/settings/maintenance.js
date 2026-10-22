// @ts-check
// Settings — Maintenance & Security (server-backed admin tunables).

import { h } from 'preact';
import { useState, useCallback } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { showToast } from '../../components/toast.js';
import { SettingsGroup, SettingsRow, NumberRow, SelectRow, ToggleRow } from './_shared.js';
import { useSettingsForm } from './form-bus.js';
import { showApiError } from '../../components/toast.js';
import { useBusy } from '../../hooks/use-busy.js';
import { showConfirm } from '../../components/modal.js';
import { formatBytes } from '../../utils.js';
import { useEffect } from 'preact/hooks';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/** Recurring jobs an operator may plausibly want to run early. */
const TRIGGERABLE = [
  'db_maintenance',
  'db_vacuum',
  'audit_prune',
  'trash_purge',
  'storage_monitor',
  'update_check',
  'integrity_scrub_quick',
  'integrity_scrub_deep',
];

/**
 * Database size and reclaim operations.
 */
function DatabaseCard() {
  const [stats, setStats] = useState(/** @type {any} */ (null));
  const { busy, run } = useBusy();

  const load = useCallback(() => {
    api.getDbStats().then(setStats).catch(() => {});
  }, []);
  useEffect(load, [load]);

  const analyze = () =>
    run(async () => {
      try {
        await api.analyzeDb();
        showToast(t('settings.db.analyzed'), { type: 'success' });
        load();
      } catch (e) {
        showApiError(e);
      }
    });

  const vacuum = () =>
    run(async () => {
      const ok = await showConfirm(t('settings.db.vacuum.confirm'), {
        title: t('settings.db.vacuum'),
        confirmLabel: t('settings.db.vacuum'),
      });
      if (!ok) return;
      try {
        await api.vacuumDb();
        showToast(t('settings.db.vacuumed'), { type: 'success' });
        load();
      } catch (e) {
        showApiError(e);
      }
    });

  return html`
    <${SettingsGroup} label=${t('settings.db.group')}>
      <${SettingsRow}
        label=${t('settings.db.size')}
        description=${t('settings.db.size.desc')}
      >
        <span class="text-sm text-text-muted">
          ${stats ? formatBytes(stats.db_size_bytes) : '—'}
          ${stats && stats.wal_size_bytes > 0
            ? html` <span class="text-xs">${t('settings.db.size.wal', { size: formatBytes(stats.wal_size_bytes) })}</span>`
            : null}
        </span>
      <//>
      <${SettingsRow}
        label=${t('settings.db.actions')}
        description=${t('settings.db.actions.desc')}
      >
        <span class="flex gap-2">
          <button type="button" class="btn-secondary btn-sm" disabled=${busy} onClick=${analyze}>
            ${t('settings.db.analyze')}
          </button>
          <button type="button" class="btn-secondary btn-sm" disabled=${busy} onClick=${vacuum}>
            ${t('settings.db.vacuum')}
          </button>
        </span>
      <//>
    <//>
  `;
}

/** Runs a scheduled job now instead of waiting for its next due time. */
function RecurringCard() {
  const { busy, run } = useBusy();
  const [last, setLast] = useState('');

  const trigger = (/** @type {string} */ kind) =>
    run(async () => {
      try {
        await api.triggerRecurring(kind);
        setLast(kind);
        showToast(t('settings.recurring.triggered', { kind: t(`settings.recurring.${kind}`) }), {
          type: 'success',
        });
      } catch (e) {
        showApiError(e);
      }
    });

  return html`
    <${SettingsGroup} label=${t('settings.recurring.group')}>
      <p class="text-xs text-text-muted px-1 pb-1">${t('settings.recurring.desc')}</p>
      <div class="flex flex-wrap gap-2 px-1 pb-2">
        ${TRIGGERABLE.map(
          (kind) => html`
            <button
              type="button"
              key=${kind}
              class=${last === kind ? 'chip chip-active' : 'chip'}
              disabled=${busy}
              onClick=${() => trigger(kind)}
            >
              ${t(`settings.recurring.${kind}`)}
            </button>
          `,
        )}
      </div>
    <//>
  `;
}

/** @param {{ settings: any }} props */
export function MaintenanceSection({ settings }) {
  const initMaint = {
    trash_retention_days: Number(settings?.trash_retention_days ?? 30),
    audit_retention_days: Number(settings?.audit_retention_days ?? 365),
    audit_security_retention_days: Number(settings?.audit_security_retention_days ?? 0),
    disk_warn_pct: Math.round(Number(settings?.disk_warn_threshold ?? 0.1) * 100),
    thumbnail_formats: String(settings?.thumbnail_formats ?? 'jpeg'),
    integrity_quick_scrub_interval_hours: Number(
      settings?.integrity_quick_scrub_interval_hours ?? 24,
    ),
    integrity_deep_scrub_interval_hours: Number(
      settings?.integrity_deep_scrub_interval_hours ?? 168,
    ),
    scrub_on_startup: Boolean(settings?.scrub_on_startup ?? false),
    integrity_revalidate_after_days: Number(settings?.integrity_revalidate_after_days ?? 30),
  };
  const initSec = {
    max_login_attempts: Number(settings?.max_login_attempts ?? 5),
    max_ip_attempts: Number(settings?.max_ip_attempts ?? 20),
    login_lockout_seconds: Number(settings?.login_lockout_seconds ?? 900),
    session_timeout_secs: Number(settings?.session_timeout_secs ?? 2592000),
  };
  const initPerf = {
    max_concurrent_jobs: Number(settings?.max_concurrent_jobs ?? 10),
    db_maintenance_interval_hours: Number(settings?.db_maintenance_interval_hours ?? 24),
    db_vacuum_interval_hours: Number(settings?.db_vacuum_interval_hours ?? 168),
    audit_prune_interval_hours: Number(settings?.audit_prune_interval_hours ?? 168),
    trash_purge_interval_hours: Number(settings?.trash_purge_interval_hours ?? 168),
  };

  const [maint, setMaint] = useState(initMaint);
  const [sec, setSec] = useState(initSec);
  const [perf, setPerf] = useState(initPerf);
  const [saved, setSaved] = useState({ maint: initMaint, sec: initSec, perf: initPerf });

  const current = { maint, sec, perf };

  const save = useCallback(async () => {
    await api.updateSettings({
      Maintenance: {
        trash_retention_days: Number(maint.trash_retention_days),
        audit_retention_days: Number(maint.audit_retention_days),
        audit_security_retention_days: Number(maint.audit_security_retention_days),
        disk_warn_threshold: Math.max(0, Math.min(100, Number(maint.disk_warn_pct))) / 100,
        thumbnail_formats: String(maint.thumbnail_formats),
        integrity_quick_scrub_interval_hours: Number(maint.integrity_quick_scrub_interval_hours),
        integrity_deep_scrub_interval_hours: Number(maint.integrity_deep_scrub_interval_hours),
        scrub_on_startup: Boolean(maint.scrub_on_startup),
        integrity_revalidate_after_days: Number(maint.integrity_revalidate_after_days),
      },
    });
    await api.updateSettings({
      Security: {
        max_login_attempts: Number(sec.max_login_attempts),
        max_ip_attempts: Number(sec.max_ip_attempts),
        login_lockout_seconds: Number(sec.login_lockout_seconds),
        session_timeout_secs: Number(sec.session_timeout_secs),
      },
    });
    await api.updateSettings({
      Performance: {
        max_concurrent_jobs: Number(perf.max_concurrent_jobs),
        db_maintenance_interval_hours: Number(perf.db_maintenance_interval_hours),
        db_vacuum_interval_hours: Number(perf.db_vacuum_interval_hours),
        audit_prune_interval_hours: Number(perf.audit_prune_interval_hours),
        trash_purge_interval_hours: Number(perf.trash_purge_interval_hours),
      },
    });
    setSaved({ maint, sec, perf });
    showToast(t('settings.maintenance.saved'), { type: 'success' });
  }, [maint, sec, perf]);

  useSettingsForm({
    current,
    saved,
    save,
    reset: () => {
      setMaint(saved.maint);
      setSec(saved.sec);
      setPerf(saved.perf);
    },
  });

  const setM = (/** @type {string} */ k, /** @type {any} */ v) => setMaint((o) => ({ ...o, [k]: v }));
  const setS = (/** @type {string} */ k, /** @type {any} */ v) => setSec((o) => ({ ...o, [k]: v }));
  const setP = (/** @type {string} */ k, /** @type {any} */ v) => setPerf((o) => ({ ...o, [k]: v }));

  return html`
    <${SettingsGroup} label=${t('settings.maintenance.group')}>
      <${NumberRow}
        label=${t('settings.maintenance.trash_retention')}
        description=${t('settings.maintenance.trash_retention.desc')}
        value=${maint.trash_retention_days}
        min=${0}
        onChange=${(v) => setM('trash_retention_days', v)}
      />
      <${NumberRow}
        label=${t('settings.maintenance.audit_retention')}
        description=${t('settings.maintenance.audit_retention.desc')}
        value=${maint.audit_retention_days}
        min=${0}
        onChange=${(v) => setM('audit_retention_days', v)}
      />
      <${NumberRow}
        label=${t('settings.maintenance.audit_security_retention')}
        description=${t('settings.maintenance.audit_security_retention.desc')}
        value=${maint.audit_security_retention_days}
        min=${0}
        onChange=${(v) => setM('audit_security_retention_days', v)}
      />
      <${NumberRow}
        label=${t('settings.maintenance.disk_warn')}
        description=${t('settings.maintenance.disk_warn.desc')}
        value=${maint.disk_warn_pct}
        min=${0}
        max=${100}
        onChange=${(v) => setM('disk_warn_pct', v)}
      />
      <${SelectRow}
        label=${t('settings.maintenance.thumbnail_formats')}
        description=${t('settings.maintenance.thumbnail_formats.desc')}
        options=${[{ value: 'jpeg', label: 'JPEG' }]}
        value=${maint.thumbnail_formats}
        onChange=${(v) => setM('thumbnail_formats', v)}
      />
      <${NumberRow}
        label=${t('settings.maintenance.quick_scrub_hours')}
        description=${t('settings.maintenance.quick_scrub_hours.desc')}
        value=${maint.integrity_quick_scrub_interval_hours}
        min=${1}
        stepper=${true}
        onChange=${(v) => setM('integrity_quick_scrub_interval_hours', v)}
      />
      <${NumberRow}
        label=${t('settings.maintenance.deep_scrub_hours')}
        description=${t('settings.maintenance.deep_scrub_hours.desc')}
        value=${maint.integrity_deep_scrub_interval_hours}
        min=${1}
        stepper=${true}
        onChange=${(v) => setM('integrity_deep_scrub_interval_hours', v)}
      />
      <${ToggleRow}
        label=${t('settings.maintenance.scrub_on_startup')}
        description=${t('settings.maintenance.scrub_on_startup.desc')}
        checked=${maint.scrub_on_startup}
        onChange=${(v) => setM('scrub_on_startup', v)}
      />
      <${NumberRow}
        label=${t('settings.maintenance.revalidate_days')}
        description=${t('settings.maintenance.revalidate_days.desc')}
        value=${maint.integrity_revalidate_after_days}
        min=${0}
        onChange=${(v) => setM('integrity_revalidate_after_days', v)}
      />
    <//>

    <${DatabaseCard} />
    <${RecurringCard} />

    <${SettingsGroup} label=${t('settings.security.group')}>
      <${NumberRow}
        label=${t('settings.security.max_login_attempts')}
        description=${t('settings.security.max_login_attempts.desc')}
        value=${sec.max_login_attempts}
        min=${1}
        stepper=${true}
        onChange=${(v) => setS('max_login_attempts', v)}
      />
      <${NumberRow}
        label=${t('settings.security.max_ip_attempts')}
        description=${t('settings.security.max_ip_attempts.desc')}
        value=${sec.max_ip_attempts}
        min=${1}
        stepper=${true}
        onChange=${(v) => setS('max_ip_attempts', v)}
      />
      <${NumberRow}
        label=${t('settings.security.lockout_seconds')}
        description=${t('settings.security.lockout_seconds.desc')}
        value=${sec.login_lockout_seconds}
        min=${1}
        onChange=${(v) => setS('login_lockout_seconds', v)}
      />
      <${NumberRow}
        label=${t('settings.security.session_timeout')}
        description=${t('settings.security.session_timeout.desc')}
        badge=${t('settings.security.restart_badge')}
        value=${sec.session_timeout_secs}
        min=${60}
        onChange=${(v) => setS('session_timeout_secs', v)}
      />
    <//>

    <${SettingsGroup} label=${t('settings.performance.group')}>
      <${NumberRow}
        label=${t('settings.performance.max_concurrent_jobs')}
        description=${t('settings.performance.max_concurrent_jobs.desc')}
        badge=${t('settings.security.restart_badge')}
        value=${perf.max_concurrent_jobs}
        min=${1}
        onChange=${(v) => setP('max_concurrent_jobs', v)}
      />
      <${NumberRow}
        label=${t('settings.performance.db_maintenance_interval')}
        description=${t('settings.performance.db_maintenance_interval.desc')}
        value=${perf.db_maintenance_interval_hours}
        min=${1}
        onChange=${(v) => setP('db_maintenance_interval_hours', v)}
      />
      <${NumberRow}
        label=${t('settings.performance.db_vacuum_interval')}
        description=${t('settings.performance.db_vacuum_interval.desc')}
        value=${perf.db_vacuum_interval_hours}
        min=${1}
        onChange=${(v) => setP('db_vacuum_interval_hours', v)}
      />
      <${NumberRow}
        label=${t('settings.performance.audit_prune_interval')}
        description=${t('settings.performance.audit_prune_interval.desc')}
        value=${perf.audit_prune_interval_hours}
        min=${1}
        onChange=${(v) => setP('audit_prune_interval_hours', v)}
      />
      <${NumberRow}
        label=${t('settings.performance.trash_purge_interval')}
        description=${t('settings.performance.trash_purge_interval.desc')}
        value=${perf.trash_purge_interval_hours}
        min=${1}
        onChange=${(v) => setP('trash_purge_interval_hours', v)}
      />
    <//>
  `;
}
