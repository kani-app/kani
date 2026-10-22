// @ts-check

import { h } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import * as api from '../../api.js';
import { showToast, showApiError } from '../../components/toast.js';
import { iconPencil, iconX } from '../../icons.js';
import { Modal, showConfirm } from '../../components/modal.js';
import { SettingsGroup, SettingsRow, ToggleRow, NumberRow } from './_shared.js';
import { useBusy } from '../../hooks/use-busy.js';
import { t } from '../../i18n.js';
import { EmptyState } from '../../components/empty-state.js';
import { FolderPicker } from '../../components/folder-picker.js';

const html = htm.bind(h);


function CategoryRow({ cat, onDragStart, onDrop, onRename, onDelete }) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(cat.name);

  const commit = async () => {
    setEditing(false);
    const nn = name.trim();
    if (!nn || nn === cat.name) {
      setName(cat.name);
      return;
    }
    await onRename(cat, nn);
  };

  return html`
    <div
      class="flex items-center gap-2 px-4 py-2.5"
      draggable=${!editing}
      onDragStart=${() => onDragStart(cat.id)}
      onDragOver=${(/** @type {Event} */ e) => e.preventDefault()}
      onDrop=${() => onDrop(cat.id)}
    >
      <span class="text-text-faint cursor-grab select-none" aria-hidden="true">⠿</span>
      ${editing
        ? html`<input
            type="text"
            class="input flex-1 text-sm"
            value=${name}
            autoFocus
            aria-label=${t('library.categories.rename', { name: cat.name })}
            onInput=${(e) => setName(e.target.value)}
            onBlur=${commit}
            onKeyDown=${(/** @type {KeyboardEvent} */ e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                commit();
              } else if (e.key === 'Escape') {
                setName(cat.name);
                setEditing(false);
              }
            }}
          />`
        : html`<span class="flex-1 text-sm text-text truncate">${cat.name}</span>`}
      <button
        type="button"
        class="btn-icon btn-sm shrink-0"
        aria-label=${t('library.categories.rename', { name: cat.name })}
        onClick=${() => setEditing(true)}
      >
        ${html([iconPencil])}
      </button>
      <button
        type="button"
        class="btn-icon btn-sm text-danger shrink-0"
        aria-label=${t('library.categories.delete', { name: cat.name })}
        onClick=${() => onDelete(cat)}
      >
        ${html([iconX])}
      </button>
    </div>
  `;
}

function AddCategory({ onAdd }) {
  const [name, setName] = useState('');
  const submit = async () => {
    const n = name.trim();
    if (!n) return;
    setName('');
    await onAdd(n);
  };
  return html`
    <div class="border-t border-border-subtle px-4 py-3 flex items-center gap-2">
      <input
        type="text"
        class="input text-sm flex-1"
        placeholder=${t('library.categories.name.placeholder')}
        aria-label=${t('library.categories.name.label')}
        value=${name}
        onInput=${(e) => setName(e.target.value)}
        onKeyDown=${(/** @type {KeyboardEvent} */ e) => e.key === 'Enter' && submit()}
      />
      <button type="button" class="btn-secondary btn-sm shrink-0" onClick=${submit}>
        ${t('library.categories.add')}
      </button>
    </div>
  `;
}

function CategoriesGroup({ initialCategories }) {
  const [cats, setCats] = useState([...initialCategories]);
  const dragId = useRef(/** @type {any} */ (null));

  const onDrop = async (/** @type {any} */ targetId) => {
    const from = dragId.current;
    dragId.current = null;
    if (from == null || from === targetId) return;
    const arr = [...cats];
    const fromIdx = arr.findIndex((c) => c.id === from);
    const toIdx = arr.findIndex((c) => c.id === targetId);
    if (fromIdx < 0 || toIdx < 0) return;
    const [moved] = arr.splice(fromIdx, 1);
    arr.splice(toIdx, 0, moved);
    setCats(arr);
    try {
      await api.reorderCategories(arr.map((c) => c.id));
    } catch (/** @type {any} */ e) {
      showToast(e?.message ?? t('library.categories.error.reorder'), { type: 'error' });
    }
  };

  const rename = async (/** @type {any} */ cat, /** @type {string} */ newName) => {
    try {
      await api.renameCategory(cat.id, newName);
      setCats((cs) => cs.map((c) => (c.id === cat.id ? { ...c, name: newName } : c)));
    } catch (/** @type {any} */ e) {
      showToast(e?.message ?? t('library.categories.error.rename'), { type: 'error' });
    }
  };

  const del = async (/** @type {any} */ cat) => {
    if (
      !(await showConfirm(t('library.categories.confirm.delete.msg', { name: cat.name }), {
        title: t('library.categories.confirm.delete.title'),
        danger: true,
      }))
    )
      return;
    try {
      await api.deleteCategory(cat.id);
      setCats((cs) => cs.filter((c) => c.id !== cat.id));
    } catch (/** @type {any} */ e) {
      showToast(e?.message ?? t('library.categories.error.delete'), { type: 'error' });
    }
  };

  const add = async (/** @type {string} */ name) => {
    try {
      await api.createCategory(name, cats.length);
      const updated = await api.getCategories();
      setCats(Array.isArray(updated) ? updated : cats);
    } catch (e) {
      showApiError(e);
    }
  };

  const count = cats.length;
  return html`
    <${SettingsGroup} label=${t('library.categories.group')}>
      <div class="detail-card-head">
        <span>${t('library.categories.count', { count, s: count !== 1 ? 'ies' : 'y' })}</span>
      </div>
      ${count === 0
        ? html`<${EmptyState}
            title=${t('library.categories.empty.title')}
            subtitle=${t('library.categories.empty.subtitle')}
          />`
        : html`<div class="divide-y divide-border-subtle">
            ${cats.map(
              (cat) => html`<${CategoryRow}
                key=${cat.id}
                cat=${cat}
                onDragStart=${(/** @type {any} */ id) => (dragId.current = id)}
                onDrop=${onDrop}
                onRename=${rename}
                onDelete=${del}
              />`,
            )}
          </div>`}
      <${AddCategory} onAdd=${add} />
    <//>
  `;
}


function ActionCard({ title, desc, children }) {
  return html`
    <div class="bg-surface border border-border-subtle rounded-xl p-4 flex flex-col gap-3 min-w-0">
      <div>
        <p class="text-sm font-medium text-text">${title}</p>
        <p class="text-xs text-text-muted mt-0.5">${desc}</p>
      </div>
      <div class="flex flex-col gap-2">${children}</div>
    </div>
  `;
}

function ProgressBar({ progress }) {
  const pct = progress.total > 0 ? Math.round((progress.completed / progress.total) * 100) : 0;
  return html`
    <div>
      <div class="flex justify-between text-xs text-text-muted mb-1">
        <span class="truncate min-w-0 mr-2">${progress.title}</span>
        <span class="shrink-0">${progress.completed} / ${progress.total}</span>
      </div>
      <div class="w-full bg-surface-3 rounded-full h-1.5">
        <div class="bg-accent h-1.5 rounded-full transition-all" style=${`width:${pct}%`}></div>
      </div>
    </div>
  `;
}

function useImportProgress(loading, origin) {
  const [progress, setProgress] = useState(/** @type {any} */ (null));
  useEffect(() => {
    if (!loading) return;
    /** @param {Event} e */
    function onSse(e) {
      const d = /** @type {any} */ (e).detail;
      if (d?.origin !== origin) return;
      if (d.type === 'import_started') {
        // Show liveness before the first imported item reports progress.
        setProgress({ completed: 0, total: d.total, title: null });
      } else if (d.type === 'import_progress') {
        setProgress({ completed: d.completed, total: d.total, title: d.title });
      } else if (d.type === 'import_completed') {
        setProgress(null);
      }
    }
    window.addEventListener('kani:sse', onSse);
    return () => window.removeEventListener('kani:sse', onSse);
  }, [loading, origin]);
  return [progress, setProgress];
}

function CheckboxList({ defs, opts, setOpts }) {
  return html`${defs.map(
    ([label, key, available]) => html`
      <label
        class=${'flex items-center gap-2 ' + (available ? 'cursor-pointer' : 'opacity-40 pointer-events-none')}
        key=${key}
      >
        <input
          type="checkbox"
          checked=${opts[key]}
          disabled=${!available}
          onChange=${(e) => setOpts((o) => ({ ...o, [key]: e.target.checked }))}
        />
        ${label}
      </label>
    `,
  )}`;
}

function RestoreModal({ file, preview, passphrase = '', onClose }) {
  const [opts, setOpts] = useState({
    merge: false,
    import_manga: true,
    import_categories: !!preview.category_count,
    import_download_rules: !!preview.download_rule_count,
    import_tracking: !!preview.has_tracking,
    import_chapter_progress: false,
    import_settings: !!preview.has_settings,
    import_repos: !!preview.repo_count,
  });
  const [loading, setLoading] = useState(false);
  const [progress] = useImportProgress(loading, 'kani_backup');

  /** @type {Array<[string, string, boolean]>} */
  const checkDefs = [
    [t('library.restore.import_manga', { count: preview.manga_count }), 'import_manga', true],
    [t('library.restore.import_categories'), 'import_categories', !!preview.category_count],
    [t('library.restore.import_download_rules'), 'import_download_rules', !!preview.download_rule_count],
    [t('library.restore.import_tracking'), 'import_tracking', !!preview.has_tracking],
    [t('library.restore.import_chapter_progress'), 'import_chapter_progress', !!preview.has_chapter_progress],
    [t('library.restore.import_settings'), 'import_settings', !!preview.has_settings],
    [t('library.restore.import_repos', { count: preview.repo_count || 0 }), 'import_repos', !!preview.repo_count],
  ];

  const doImport = async () => {
    setLoading(true);
    try {
      const r = await api.restoreBackupEncrypted(file, opts, passphrase);
      showToast(t('library.restore.success', { count: r.imported_manga }), { type: 'success' });
      if (r.trashed_manga > 0) {
        showToast(t('library.import.trashed', { count: r.trashed_manga }), { type: 'warn' });
      }
      onClose();
    } catch (e) {
      showApiError(e);
    } finally {
      setLoading(false);
    }
  };

  return html`
    <${Modal}
      open=${true}
      title=${t('library.restore.modal.title')}
      onClose=${onClose}
      footer=${html`
        <button type="button" class="btn-ghost btn-sm" onClick=${onClose}>${t('common.cancel')}</button>
        <button type="button" class="btn-primary btn-sm" disabled=${loading} onClick=${doImport}>
          ${loading ? t('library.restore.importing') : t('library.restore.import_btn')}
        </button>
      `}
    >
      <div class="flex flex-col gap-4">
        <p class="text-sm text-text-muted">
          ${t('library.restore.meta', {
            date: preview.exported_at?.slice(0, 10) ?? t('common.unknown'),
            manga: preview.manga_count,
            categories: preview.category_count,
          })}
        </p>
        ${preview.sources?.length
          ? html`<div class="text-xs flex flex-col gap-1 bg-surface-3 rounded p-2">
              ${preview.sources.map(
                (s) => html`<div class="flex items-center justify-between" key=${s.source_name}>
                  <span>${s.source_name} (${s.manga_count})</span>
                  <span class=${s.found ? 'text-success' : 'text-danger'}
                    >${s.found ? t('library.restore.source_available') : t('library.restore.source_missing')}</span
                  >
                </div>`,
              )}
            </div>`
          : null}
        <div class="flex flex-col gap-1.5 text-sm">
          <${CheckboxList} defs=${checkDefs} opts=${opts} setOpts=${setOpts} />
          <label class="flex items-center gap-2 cursor-pointer mt-1">
            <input type="checkbox" checked=${opts.merge} onChange=${(e) => setOpts((o) => ({ ...o, merge: e.target.checked }))} />
            ${t('library.restore.merge_option')}
          </label>
        </div>
        ${loading && progress ? html`<${ProgressBar} progress=${progress} />` : null}
      </div>
    <//>
  `;
}

function TachiyomiImportModal({ file, preview, onClose }) {
  const [opts, setOpts] = useState({
    import_manga: true,
    import_categories: !!preview.category_count,
    import_tracking: !!preview.has_tracking,
    import_chapter_progress: false,
  });
  const [loading, setLoading] = useState(false);
  const [progress] = useImportProgress(loading, 'tachiyomi');

  /** @type {Array<[string, string, boolean]>} */
  const checkDefs = [
    [t('library.tachiyomi.import_manga'), 'import_manga', true],
    [t('library.restore.import_categories'), 'import_categories', !!preview.category_count],
    [t('library.restore.import_tracking'), 'import_tracking', !!preview.has_tracking],
    [t('library.restore.import_chapter_progress'), 'import_chapter_progress', !!preview.has_chapter_progress],
  ];

  const doImport = async () => {
    setLoading(true);
    try {
      const r = await api.importTachiyomiBackup(file, opts);
      showToast(t('library.tachiyomi.success', { count: r.imported_manga }), { type: 'success' });
      if (r.unmatched_progress > 0) {
        showToast(t('library.restore.unmatched_progress', { count: r.unmatched_progress }), { type: 'warn' });
      }
      if (r.trashed_manga > 0) {
        showToast(t('library.import.trashed', { count: r.trashed_manga }), { type: 'warn' });
      }
      onClose();
    } catch (e) {
      showApiError(e);
    } finally {
      setLoading(false);
    }
  };

  const pendingNote =
    preview.pending_import_estimate > 0
      ? ` · ${t('library.tachiyomi.pending_note', { count: preview.pending_import_estimate })}`
      : '';

  return html`
    <${Modal}
      open=${true}
      title=${t('library.tachiyomi.modal.title')}
      onClose=${onClose}
      footer=${html`
        <button type="button" class="btn-ghost btn-sm" onClick=${onClose}>${t('common.cancel')}</button>
        <button type="button" class="btn-primary btn-sm" disabled=${loading} onClick=${doImport}>
          ${loading ? t('library.restore.importing') : t('library.restore.import_btn')}
        </button>
      `}
    >
      <div class="flex flex-col gap-4">
        <p class="text-sm text-text-muted">
          ${t('library.tachiyomi.meta', {
            manga: preview.total_manga,
            categories: preview.category_count,
          })}${pendingNote}
        </p>
        ${preview.sources?.length
          ? html`<div class="text-xs flex flex-col gap-1 bg-surface-3 rounded p-2">
              ${preview.sources.map(
                (s) => html`<div class="flex items-center justify-between" key=${s.source_id}>
                  <span>${s.source_name} (${s.manga_count})</span>
                  <span class=${s.found ? 'text-success' : 'text-danger'}
                    >${s.found ? t('library.tachiyomi.source_matched') : t('library.tachiyomi.source_unmatched')}</span
                  >
                </div>`,
              )}
            </div>`
          : null}
        <div class="flex flex-col gap-1.5 text-sm">
          <${CheckboxList} defs=${checkDefs} opts=${opts} setOpts=${setOpts} />
        </div>
        ${loading && progress ? html`<${ProgressBar} progress=${progress} />` : null}
      </div>
    <//>
  `;
}

function ImportExport() {
  const [includeProgress, setIncludeProgress] = useState(false);
  const [exportPass, setExportPass] = useState('');
  const [restorePass, setRestorePass] = useState('');
  const [restore, setRestore] = useState(/** @type {any} */ (null));
  const [tachi, setTachi] = useState(/** @type {any} */ (null));
  const restoreInput = useRef(/** @type {HTMLInputElement|null} */ (null));
  const tachiInput = useRef(/** @type {HTMLInputElement|null} */ (null));

  const onRestoreFile = async (/** @type {Event} */ e) => {
    const file = /** @type {HTMLInputElement} */ (e.target).files?.[0];
    /** @type {HTMLInputElement} */ (e.target).value = '';
    if (!file) return;
    try {
      const preview = await api.previewBackupEncrypted(file, restorePass);
      setRestore({ file, preview, passphrase: restorePass });
    } catch (/** @type {any} */ err) {
      showToast(t('library.restore.preview_failed', { msg: err.message }), { type: 'error' });
    }
  };

  const onTachiFile = async (/** @type {Event} */ e) => {
    const file = /** @type {HTMLInputElement} */ (e.target).files?.[0];
    /** @type {HTMLInputElement} */ (e.target).value = '';
    if (!file) return;
    try {
      const preview = await api.previewTachiyomiImport(file);
      setTachi({ file, preview });
    } catch (/** @type {any} */ err) {
      showToast(t('library.tachiyomi.preview_failed', { msg: err.message }), { type: 'error' });
    }
  };

  return html`
    <${SettingsGroup} label=${t('library.import_export.group')}>
      <div class="grid sm:grid-cols-2 gap-3 p-3">
        <${ActionCard} title=${t('library.export.label')} desc=${t('library.export.desc')}>
          <label class="flex items-center gap-2 text-sm text-text cursor-pointer select-none">
            <input type="checkbox" checked=${includeProgress} onChange=${(e) => setIncludeProgress(e.target.checked)} />
            ${t('library.export.include_progress')}
          </label>
          <input
            type="password"
            class="input text-sm w-full"
            placeholder=${t('backup.export.passphrase')}
            title=${t('backup.export.passphrase.desc')}
            autocomplete="new-password"
            value=${exportPass}
            onInput=${(e) => setExportPass(e.target.value)}
          />
          <button
            type="button"
            class="btn-secondary btn-sm w-full"
            onClick=${() => api.downloadBackupEncrypted(includeProgress, exportPass)}
          >
            ${t('library.export.btn')}
          </button>
        <//>
        <${ActionCard} title=${t('library.restore.label')} desc=${t('library.restore.desc')}>
          <input
            type="password"
            class="input text-sm w-full"
            placeholder=${t('backup.restore.passphrase')}
            autocomplete="current-password"
            value=${restorePass}
            onInput=${(e) => setRestorePass(e.target.value)}
          />
          <input ref=${restoreInput} type="file" accept=".zip" class="hidden" onChange=${onRestoreFile} />
          <button type="button" class="btn-secondary btn-sm w-full" onClick=${() => restoreInput.current?.click()}>
            ${t('library.restore.choose_file')}
          </button>
          <div class="border-t border-border-subtle my-1"></div>
          <div>
            <p class="text-sm font-medium text-text">${t('library.tachiyomi.label')}</p>
            <p class="text-xs text-text-muted mt-0.5">${t('library.tachiyomi.desc')}</p>
          </div>
          <input ref=${tachiInput} type="file" accept=".tachibk,.proto.gz" class="hidden" onChange=${onTachiFile} />
          <button type="button" class="btn-secondary btn-sm w-full" onClick=${() => tachiInput.current?.click()}>
            ${t('library.tachiyomi.choose_file')}
          </button>
        <//>
      </div>
    <//>
    ${restore &&
    html`<${RestoreModal}
      file=${restore.file}
      preview=${restore.preview}
      passphrase=${restore.passphrase}
      onClose=${() => setRestore(null)}
    />`}
    ${tachi &&
    html`<${TachiyomiImportModal} file=${tachi.file} preview=${tachi.preview} onClose=${() => setTachi(null)} />`}
  `;
}


const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

function ScheduledBackup() {
  const [cfg, setCfg] = useState({
    enabled: false,
    frequency: { type: 'daily', hour: 2 },
    retain_n: 7,
    destination: { type: 'local', path: '/backups' },
    passphrase: null,
  });
  const [pickerOpen, setPickerOpen] = useState(false);
  const { busy: saving, run: runSave } = useBusyImport();
  const { busy: running, run: runRun } = useBusyImport();

  useEffect(() => {
    api
      .getBackupSchedule()
      .then((loaded) => {
        if (loaded) setCfg(loaded);
      })
      .catch(() => {});
  }, []);

  const patch = (/** @type {any} */ p) => setCfg((c) => ({ ...c, ...p }));
  const patchFreq = (/** @type {any} */ p) =>
    setCfg((c) => ({ ...c, frequency: { ...c.frequency, ...p } }));

  const disabled = !cfg.enabled;
  const isWeekly = cfg.frequency?.type === 'weekly';

  const save = () =>
    runSave(async () => {
      try {
        await api.setBackupSchedule(cfg);
        showToast(t('backup.schedule.saved'), { type: 'success' });
      } catch (e) {
        showApiError(e);
      }
    });

  const runNow = () =>
    runRun(async () => {
      try {
        await api.runBackupNow();
        showToast(t('backup.schedule.job_submitted'), {
          type: 'success',
          action: { label: t('backup.schedule.job_view'), href: '/admin/jobs' },
        });
      } catch (e) {
        showApiError(e);
      }
    });

  const dimmed = disabled ? 'opacity-50' : '';

  return html`
    <${SettingsGroup} label=${t('backup.group.schedule')}>
      <${ToggleRow}
        label=${t('backup.schedule.enabled')}
        checked=${!!cfg.enabled}
        onChange=${(v) => patch({ enabled: v })}
      />
      <div class=${dimmed}>
        <${SettingsRow} label=${t('backup.schedule.frequency')}>
          <select
            class="input text-sm w-28"
            disabled=${disabled}
            value=${cfg.frequency?.type ?? 'daily'}
            onChange=${(e) => {
              const type = e.target.value;
              const freq = { ...cfg.frequency, type };
              if (type === 'weekly' && freq.weekday == null) freq.weekday = 0;
              patch({ frequency: freq });
            }}
          >
            <option value="daily">${t('backup.schedule.daily')}</option>
            <option value="weekly">${t('backup.schedule.weekly')}</option>
          </select>
        <//>
        <${NumberRow}
          label=${t('backup.schedule.hour')}
          tooltip=${t('backup.schedule.hour.tooltip')}
          value=${cfg.frequency?.hour ?? 2}
          min=${0}
          max=${23}
          onChange=${(v) => patchFreq({ hour: v })}
        />
        ${isWeekly &&
        html`<${SettingsRow} label=${t('backup.schedule.weekday')}>
          <select
            class="input text-sm"
            disabled=${disabled}
            value=${String(cfg.frequency?.weekday ?? 0)}
            onChange=${(e) => patchFreq({ weekday: Number(e.target.value) })}
          >
            ${WEEKDAYS.map((d, i) => html`<option value=${i}>${d}</option>`)}
          </select>
        <//>`}
        <${NumberRow}
          label=${t('backup.schedule.retain')}
          tooltip=${t('backup.schedule.retain.tooltip')}
          value=${cfg.retain_n ?? 7}
          min=${1}
          max=${365}
          stepper=${true}
          onChange=${(v) => patch({ retain_n: v })}
        />
      </div>
      <${SettingsRow} label=${t('backup.schedule.path')} tooltip=${t('backup.schedule.path.tooltip')}>
        <div class="flex items-center gap-2">
          <span class="font-mono text-sm text-text truncate max-w-56">${cfg.destination?.path ?? '/backups'}</span>
          <button type="button" class="btn-secondary btn-sm shrink-0" onClick=${() => setPickerOpen(true)}>
            ${t('backup.schedule.path.browse')}
          </button>
        </div>
      <//>
      <${SettingsRow}
        label=${t('backup.schedule.passphrase')}
        description=${t('backup.schedule.passphrase.desc')}
      >
        <input
          type="password"
          class="input text-sm w-44"
          placeholder=${cfg.passphrase === '***' ? '••••••••' : ''}
          autocomplete="new-password"
          onChange=${(e) => patch({ passphrase: e.target.value || null })}
        />
      <//>
      <div class="flex items-center gap-2 px-4 py-3 border-t border-border-subtle">
        <button type="button" class="btn-primary btn-sm" disabled=${saving} onClick=${save}>
          ${saving ? t('backup.schedule.saving') : t('backup.schedule.save')}
        </button>
        <button
          type="button"
          class="btn-secondary btn-sm"
          disabled=${running}
          data-tooltip=${t('backup.schedule.run_now.desc')}
          onClick=${runNow}
        >
          ${t('backup.schedule.run_now')}
        </button>
      </div>
    <//>
    <${FolderPicker}
      open=${pickerOpen}
      initialPath=${cfg.destination?.path ?? '/backups'}
      onClose=${() => setPickerOpen(false)}
      onSelect=${(/** @type {string} */ path) => {
        setPickerOpen(false);
        patch({ destination: { type: 'local', path } });
      }}
    />
  `;
}

function useBusyImport() {
  const [busy, setBusy] = useState(false);
  const run = async (/** @type {() => Promise<any>} */ fn) => {
    if (busy) return;
    setBusy(true);
    try {
      return await fn();
    } finally {
      setBusy(false);
    }
  };
  return { busy, run };
}

/** @param {{ categories: any[] }} props */
/**
 * Kicks off a scan of every series now.
 *
 * `scanAllLibrary` had no caller: scanning was reachable only per-manga or on
 * the auto-scan schedule, so there was no way to say "check everything now"
 * after adding a source or changing scan settings.
 */
function ScanAllGroup() {
  const { busy, run } = useBusy();

  const scanAll = () =>
    run(async () => {
      const ok = await showConfirm(t('library.scan_all.confirm'), {
        title: t('library.scan_all.label'),
        confirmLabel: t('library.scan_all.label'),
      });
      if (!ok) return;
      try {
        await api.scanAllLibrary();
        showToast(t('library.scan_all.started'), { type: 'success' });
      } catch (e) {
        showApiError(e);
      }
    });

  return html`
    <${SettingsGroup} label=${t('library.scan_all.group')}>
      <${SettingsRow} label=${t('library.scan_all.label')} description=${t('library.scan_all.desc')}>
        <button type="button" class="btn-secondary btn-sm" disabled=${busy} onClick=${scanAll}>
          ${t('library.scan_all.action')}
        </button>
      <//>
    <//>
  `;
}

export function LibrarySection({ categories }) {
  return html`
    <${CategoriesGroup} initialCategories=${categories ?? []} />
    <${ScanAllGroup} />
    <${ImportExport} />
    <${ScheduledBackup} />
  `;
}
