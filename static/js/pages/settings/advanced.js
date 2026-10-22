// @ts-check

import { h } from 'preact';
import { useState, useEffect, useCallback, useRef } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { setLocal } from '../../utils.js';
import { addPendingFields } from '../../components/restart-tray.js';
import { showToast, showApiError } from '../../components/toast.js';
import { SettingsGroup, SettingsRow, ToggleRow } from './_shared.js';
import { useSettingsForm } from './form-bus.js';
import { useBusy } from '../../hooks/use-busy.js';
import { FolderPicker } from '../../components/folder-picker.js';
import { PathMigrationDialog } from '../../components/path-migration-dialog.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/**
 * Probes the URL in the box rather than the saved one, so a change can be
 * checked before it is committed. A solver without scripted capture is warned
 * about, not failed: it still solves ordinary HTTP challenges.
 */
function SolverTest({ url }) {
  const [result, setResult] = useState(
    /** @type {{ tone: string, text: string, insecure: boolean } | null} */ (null),
  );
  const { busy, run } = useBusy();

  const check = () =>
    run(async () => {
      setResult(null);
      const trimmed = (url ?? '').trim();
      if (!trimmed) {
        setResult({
          tone: 'text-text-muted',
          text: t('settings.advanced.flaresolverr.result_not_configured'),
          insecure: false,
        });
        return;
      }
      try {
        const res = await api.testSolver(trimmed);
        const tone =
          res.status === 'capture'
            ? 'text-success'
            : res.status === 'unreachable'
              ? 'text-danger'
              : 'text-warn';
        setResult({
          tone,
          text: t(`settings.advanced.flaresolverr.result_${res.status}`),
          insecure: !!res.insecure_transport,
        });
      } catch (e) {
        setResult({
          tone: 'text-danger',
          text: /** @type {any} */ (e)?.message ?? t('settings.advanced.flaresolverr.result_unreachable'),
          insecure: false,
        });
      }
    });

  return html`
    <div class="flex flex-col items-end gap-1">
      <button type="button" class="btn-secondary btn-sm" disabled=${busy} onClick=${check}>
        ${busy ? t('settings.advanced.flaresolverr.testing') : t('settings.advanced.flaresolverr.test_btn')}
      </button>
      ${result &&
      html`<span class=${`text-xs text-right max-w-64 ${result.tone}`}>${result.text}</span>`}
      ${result?.insecure &&
      html`<span class="text-xs text-right max-w-64 text-warn"
        >${t('settings.advanced.flaresolverr.insecure')}</span
      >`}
    </div>
  `;
}

function EncryptionGroup() {
  const [status, setStatus] = useState(/** @type {any} */ (null));
  const [failed, setFailed] = useState(false);
  const [result, setResult] = useState(/** @type {{ ok: boolean, text: string } | null} */ (null));
  const { busy, run } = useBusy();

  const refresh = useCallback(async () => {
    try {
      setStatus(await api.getCredentialEncryptionStatus());
    } catch {
      setFailed(true);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const migrate = () =>
    run(async () => {
      setResult(null);
      try {
        await api.migrateCredentialsToEncrypted();
        setResult({ ok: true, text: t('settings.advanced.encryption.result_done') });
        await refresh();
      } catch (/** @type {any} */ e) {
        setResult({ ok: false, text: e?.message ?? 'Failed' });
      }
    });

  let text = t('common.loading');
  let showBtn = false;
  if (failed) {
    text = t('settings.advanced.encryption.load_failed');
  } else if (status) {
    if (!status.encryption_enabled) {
      text = t('settings.advanced.encryption.disabled');
    } else if (status.plaintext_count > 0) {
      text = t('settings.advanced.encryption.partial', {
        count: status.plaintext_count,
        s: status.plaintext_count === 1 ? '' : 's',
      });
      showBtn = true;
    } else {
      text = t('settings.advanced.encryption.all_done');
    }
  }

  return html`
    <${SettingsGroup} label=${t('settings.advanced.encryption.group')}>
      <${SettingsRow}
        label=${t('settings.advanced.encryption.label')}
        description=${t('settings.advanced.encryption.desc')}
        tooltip=${t('settings.advanced.encryption.tooltip')}
      >
        <div class="flex flex-col gap-2">
          <p class="text-sm text-text-muted px-1">${text}</p>
          <div class="flex items-center gap-3">
            ${showBtn &&
            html`<button type="button" class="btn-primary btn-sm" disabled=${busy} onClick=${migrate}>
              ${busy ? t('settings.advanced.encryption.encrypting') : t('settings.advanced.encryption.btn')}
            </button>`}
            ${result &&
            html`<span class=${`text-xs ${result.ok ? 'text-success' : 'text-danger'}`}
              >${result.text}</span
            >`}
          </div>
        </div>
      <//>
    <//>
  `;
}

function MaintenanceActions() {
  const maint = useBusy();
  const dedup = useBusy();

  const runMaint = () =>
    maint.run(async () => {
      try {
        const res = await api.runMaintenance();
        const freed = (res?.before_bytes ?? 0) - (res?.after_bytes ?? 0);
        const mb = (freed / 1024 / 1024).toFixed(1);
        showToast(
          freed > 0
            ? t('settings.advanced.maintenance.freed', { mb })
            : t('settings.advanced.maintenance.no_freed'),
          { type: 'success' },
        );
      } catch (e) {
        showApiError(e);
      }
    });

  const runDedup = () =>
    dedup.run(async () => {
      try {
        const res = await api.rescanDuplicates();
        const n = res?.new_pairs ?? 0;
        showToast(
          n === 0
            ? t('settings.advanced.dedup.no_pairs')
            : t('settings.advanced.dedup.pairs', { count: n, s: n === 1 ? '' : 's' }),
          { type: 'success' },
        );
      } catch (e) {
        showApiError(e);
      }
    });

  return html`
    <${SettingsGroup} label=${t('settings.advanced.maintenance.group')}>
      <${SettingsRow}
        label=${t('settings.advanced.maintenance.label')}
        description=${t('settings.advanced.maintenance.desc')}
      >
        <button type="button" class="btn-ghost btn-sm" disabled=${maint.busy} onClick=${runMaint}>
          ${maint.busy ? t('settings.advanced.maintenance.running') : t('settings.advanced.maintenance.btn')}
        </button>
      <//>
      <${SettingsRow}
        label=${t('settings.advanced.dedup.label')}
        description=${t('settings.advanced.dedup.desc')}
      >
        <button type="button" class="btn-ghost btn-sm" disabled=${dedup.busy} onClick=${runDedup}>
          ${dedup.busy ? t('settings.advanced.dedup.scanning') : t('settings.advanced.dedup.btn')}
        </button>
      <//>
    <//>
  `;
}

/** @param {{ settings: any, bootId: string }} props */
export function AdvancedSection({ settings, bootId }) {
  const initial = {
    flaresolverr_url: settings?.flaresolverr_url ?? '',
    library_path: settings?.library_path ?? '',
    wasm_storage_path: settings?.wasm_storage_path ?? '',
    max_wasm_instances: settings?.max_wasm_instances ?? null,
    cover_max_dimension: settings?.cover_max_dimension ?? null,
    v8_max_memory_mb: settings?.v8_max_memory_mb ?? null,
    v8_idle_timeout_s: settings?.v8_idle_timeout_s ?? null,
    http_request_logging: settings?.http_request_logging ?? false,
    update_check_enabled: settings?.update_check_enabled ?? true,
    global_search_timeout_secs: Number(settings?.global_search_timeout_secs ?? 6),
    opds_page_index_zero_based: settings?.opds_page_index_zero_based ?? false,
    v8_debug_logging: settings?.v8_debug_logging ?? false,
    registration_enabled: settings?.registration_enabled ?? false,
  };
  const [form, setForm] = useState(initial);
  const [saved, setSaved] = useState(initial);
  const set = (/** @type {string} */ k, /** @type {any} */ v) => setForm((f) => ({ ...f, [k]: v }));

  const [picker, setPicker] = useState({ open: false, initial: '/', target: '' });
  const [mig, setMig] = useState({ open: false, field: '', current: '', next: '' });
  const migResolve = useRef(/** @type {null | ((moved: boolean) => void)} */ (null));

  const promptMigration = (/** @type {string} */ field, /** @type {string} */ oldPath, /** @type {string} */ newPath) => {
    if (!newPath || newPath === oldPath) return Promise.resolve(false);
    return new Promise((resolve) => {
      migResolve.current = resolve;
      setMig({ open: true, field, current: oldPath, next: newPath });
    });
  };

  const save = useCallback(async () => {
    const libChanged = form.library_path !== saved.library_path;
    const wasmChanged = form.wasm_storage_path !== saved.wasm_storage_path;
    if (libChanged) await promptMigration('library_path', saved.library_path, form.library_path);
    if (wasmChanged) {
      await promptMigration('wasm_storage_path', saved.wasm_storage_path, form.wasm_storage_path);
    }
    await api.updateSettings({ Advanced: form });
    setSaved(form);
    showToast(t('common.saved'), { type: 'success' });
    if ((form.max_wasm_instances ?? null) !== (settings?.max_wasm_instances ?? null)) {
      setLocal('kani_restart_boot_id', bootId);
      addPendingFields(['max_wasm_instances']);
      window.dispatchEvent(new StorageEvent('storage', { key: 'kani_restart_needed' }));
    }
  }, [form, saved]);

  useSettingsForm({ current: form, saved, save, reset: () => setForm(saved) });

  const numInput = (/** @type {string} */ key, /** @type {any} */ opts) => html`<input
    type="number"
    class="input w-24 text-sm"
    aria-label=${opts.label}
    min=${opts.min}
    max=${opts.max}
    placeholder=${opts.placeholder}
    value=${form[key] != null ? String(form[key]) : ''}
    onInput=${(/** @type {Event} */ e) => {
      const val = /** @type {HTMLInputElement} */ (e.target).value;
      set(key, val === '' ? null : Number(val));
    }}
  />`;

  const pathControl = (/** @type {string} */ key) => html`<div class="flex items-center gap-2">
    <span class="text-sm font-mono text-text truncate max-w-xs" title=${form[key]}
      >${form[key] || t('settings.advanced.path.not_set')}</span
    >
    <button
      type="button"
      class="btn-ghost btn-sm"
      onClick=${() => setPicker({ open: true, initial: form[key] || '/', target: key })}
    >
      ${t('settings.advanced.path.browse')}
    </button>
  </div>`;

  return html`
    <${SettingsGroup} label=${t('settings.advanced.server.group')}>
      <${SettingsRow}
        label=${t('settings.advanced.flaresolverr.label')}
        description=${t('settings.advanced.flaresolverr.desc')}
      >
        <div class="flex items-center gap-2">
          <input
            type="url"
            class="input w-56 text-sm"
            placeholder="http://localhost:8191"
            value=${form.flaresolverr_url}
            onInput=${(e) => set('flaresolverr_url', e.target.value)}
          />
          <${SolverTest} url=${form.flaresolverr_url} />
        </div>
      <//>
      <${SettingsRow}
        label=${t('settings.advanced.library_path.label')}
        description=${t('settings.advanced.library_path.desc')}
      >
        ${pathControl('library_path')}
      <//>
      <${SettingsRow}
        label=${t('settings.advanced.wasm_path.label')}
        description=${t('settings.advanced.wasm_path.desc')}
      >
        ${pathControl('wasm_storage_path')}
      <//>
      <${SettingsRow}
        label=${t('settings.advanced.wasm_instances.label')}
        description=${t('settings.advanced.wasm_instances.desc')}
        badge=${t('settings.badge.restart_required')}
      >
        ${numInput('max_wasm_instances', { min: 1, label: t('settings.advanced.wasm_instances.label') })}
      <//>
      <${SettingsRow}
        label=${t('settings.advanced.cover_dim.label')}
        description=${t('settings.advanced.cover_dim.desc')}
      >
        ${numInput('cover_max_dimension', { min: 100, max: 2000, placeholder: '800', label: t('settings.advanced.cover_dim.label') })}
      <//>
      <${SettingsRow}
        label=${t('settings.advanced.v8_max_memory.label')}
        description=${t('settings.advanced.v8_max_memory.desc')}
        tooltip=${t('settings.advanced.v8_caps.tooltip')}
      >
        ${numInput('v8_max_memory_mb', { min: 64, max: 8192, placeholder: '512', label: t('settings.advanced.v8_max_memory.label') })}
      <//>
      <${ToggleRow}
        label=${t('settings.advanced.http_logging.label')}
        description=${t('settings.advanced.http_logging.desc')}
        checked=${form.http_request_logging}
        onChange=${(v) => set('http_request_logging', v)}
      />
      <${ToggleRow}
        label=${t('settings.advanced.update_check.label')}
        description=${t('settings.advanced.update_check.desc')}
        checked=${form.update_check_enabled}
        onChange=${(v) => set('update_check_enabled', v)}
      />
      <${SettingsRow}
        label=${t('settings.advanced.global_search_timeout.label')}
        description=${t('settings.advanced.global_search_timeout.desc')}
      >
        ${numInput('global_search_timeout_secs', { min: 1, max: 60, placeholder: '6', label: t('settings.advanced.global_search_timeout.label') })}
      <//>
      <${ToggleRow}
        label=${t('settings.advanced.opds_zero_based.label')}
        description=${t('settings.advanced.opds_zero_based.desc')}
        checked=${form.opds_page_index_zero_based}
        onChange=${(v) => set('opds_page_index_zero_based', v)}
      />
      <${ToggleRow}
        label=${t('settings.advanced.v8_logging.label')}
        description=${t('settings.advanced.v8_logging.desc')}
        checked=${form.v8_debug_logging}
        onChange=${(v) => set('v8_debug_logging', v)}
      />
      <${ToggleRow}
        label=${t('settings.advanced.registration.label')}
        description=${t('settings.advanced.registration.desc')}
        checked=${form.registration_enabled}
        onChange=${(v) => set('registration_enabled', v)}
      />
    <//>

    <${EncryptionGroup} />
    <${MaintenanceActions} />

    <${FolderPicker}
      open=${picker.open}
      initialPath=${picker.initial}
      onClose=${() => setPicker((p) => ({ ...p, open: false }))}
      onSelect=${(/** @type {string} */ path) => {
        if (picker.target) set(picker.target, path);
        setPicker((p) => ({ ...p, open: false }));
      }}
    />
    <${PathMigrationDialog}
      open=${mig.open}
      field=${mig.field}
      currentPath=${mig.current}
      newPath=${mig.next}
      onDone=${(/** @type {boolean} */ moved) => {
        setMig((m) => ({ ...m, open: false }));
        migResolve.current?.(moved);
      }}
      onCancel=${() => {
        setMig((m) => ({ ...m, open: false }));
        migResolve.current?.(false);
      }}
    />
  `;
}
