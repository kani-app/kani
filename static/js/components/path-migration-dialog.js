// @ts-check

import { h } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import { Modal } from './modal.js';
import * as api from '../api.js';
import { t } from '../i18n.js';

const html = htm.bind(h);

/**
 * @param {{
 *   open: boolean,
 *   field: 'library_path' | 'wasm_storage_path',
 *   currentPath: string,
 *   newPath: string,
 *   onDone: (movedFiles: boolean) => void,
 *   onCancel: () => void,
 * }} props
 */
export function PathMigrationDialog({ open, field, currentPath, newPath, onDone, onCancel }) {
  const [phase, setPhase] = useState('estimating');
  const [estimate, setEstimate] = useState(/** @type {any} */ (null));
  const [estimateError, setEstimateError] = useState(/** @type {string|null} */ (null));
  const [bytesCopied, setBytesCopied] = useState(0);
  const [totalBytes, setTotalBytes] = useState(0);
  const [migError, setMigError] = useState(/** @type {string|null} */ (null));
  const sseListenerRef = useRef(/** @type {((e: Event) => void) | null} */ (null));

  useEffect(() => {
    if (!open) return;
    setPhase('estimating');
    setEstimate(null);
    setEstimateError(null);
    setBytesCopied(0);
    setTotalBytes(0);
    setMigError(null);

    api.estimatePathMigration(field, newPath).then((est) => {
      setEstimate(est);
      setPhase('confirm');
    }).catch((/** @type {any} */ e) => {
      setEstimateError(e?.message ?? t('path_migration.error.estimate_failed'));
      setPhase('confirm');
    });
  }, [open, field, newPath]);

  // Detach SSE listener on unmount / close
  useEffect(() => {
    return () => {
      if (sseListenerRef.current) {
        window.removeEventListener('kani:sse', sseListenerRef.current);
        sseListenerRef.current = null;
      }
    };
  }, []);

  function detachSseListener() {
    if (sseListenerRef.current) {
      window.removeEventListener('kani:sse', sseListenerRef.current);
      sseListenerRef.current = null;
    }
  }

  async function handleMoveFiles() {
    setPhase('migrating');
    setBytesCopied(0);

    /** @param {Event} ev */
    const handler = (ev) => {
      const data = /** @type {CustomEvent} */ (ev).detail;
      if (!data || data.field !== field) return;
      if (data.type === 'path_migration_started') {
        setTotalBytes(data.total_bytes ?? 0);
      } else if (data.type === 'path_migration_progress') {
        setBytesCopied(data.bytes_copied ?? 0);
        setTotalBytes(data.total_bytes ?? 0);
      } else if (data.type === 'path_migration_completed') {
        detachSseListener();
        setPhase('done');
        setTimeout(() => onDone(true), 800);
      } else if (data.type === 'path_migration_failed') {
        detachSseListener();
        setMigError(data.error ?? t('path_migration.error.failed'));
        setPhase('error');
      }
    };

    sseListenerRef.current = handler;
    window.addEventListener('kani:sse', handler);

    try {
      await api.startPathMigration(field, newPath);
    } catch (/** @type {any} */ e) {
      detachSseListener();
      setMigError(e?.message ?? t('path_migration.error.start_failed'));
      setPhase('error');
    }
  }

  function handleChangePathOnly() {
    onDone(false);
  }

  function fmtBytes(bytes) {
    const gib = 1024 * 1024 * 1024;
    const mib = 1024 * 1024;
    if (bytes >= gib) return (bytes / gib).toFixed(1) + ' GB';
    return Math.round(bytes / mib) + ' MB';
  }

  const progressPct = totalBytes > 0 ? Math.round((bytesCopied / totalBytes) * 100) : 0;
  const label = t(field === 'library_path' ? 'path_migration.label.library' : 'path_migration.label.wasm');

  let footer = null;
  if (phase === 'confirm') {
    footer = html`
      <button type="button" class="btn-ghost btn-sm" onClick=${onCancel}>${t('common.cancel')}</button>
      <button type="button" class="btn-ghost btn-sm" onClick=${handleChangePathOnly}>
        ${t('path_migration.btn.change_path_only')}
      </button>
      <button
        type="button"
        class="btn-primary btn-sm"
        onClick=${handleMoveFiles}
        disabled=${estimateError != null || (estimate && !estimate.can_migrate)}
      >
        ${t('path_migration.btn.move_files')}
      </button>
    `;
  }

  return html`
    <${Modal}
      open=${open}
      onClose=${phase === 'migrating' ? undefined : onCancel}
      title=${t('path_migration.title', { label })}
      footer=${footer}
    >
      ${phase === 'estimating' && html`
        <p class="text-sm text-text-muted">${t('path_migration.estimating')}</p>
      `}

      ${phase === 'confirm' && html`
        <div class="space-y-4 text-sm">
          <div class="flex flex-col gap-1">
            <span class="text-text-muted">${t('path_migration.confirm.from')}</span>
            <span class="font-mono text-text break-all">${currentPath}</span>
          </div>
          <div class="flex flex-col gap-1">
            <span class="text-text-muted">${t('path_migration.confirm.to')}</span>
            <span class="font-mono text-text break-all">${newPath}</span>
          </div>

          ${estimateError && html`
            <p class="text-danger">${estimateError}</p>
          `}

          ${estimate && html`
            <div class="bg-surface-2 rounded-lg p-3 space-y-1">
              <div class="flex justify-between">
                <span class="text-text-muted">${t('path_migration.confirm.data_to_copy')}</span>
                <span class="font-medium">${fmtBytes(estimate.current_bytes)}</span>
              </div>
              <div class="flex justify-between">
                <span class="text-text-muted">${t('path_migration.confirm.available_space')}</span>
                <span class="font-medium">${fmtBytes(estimate.available_bytes)}</span>
              </div>
            </div>
          `}

          ${estimate && !estimate.can_migrate && html`
            <p class="text-danger text-sm">${estimate.reason ?? t('path_migration.confirm.not_possible')}</p>
          `}

          <p class="text-text-muted text-xs">
            <strong>${t('path_migration.confirm.note_bold')}</strong> ${t('path_migration.confirm.note_rest')}
          </p>
        </div>
      `}

      ${phase === 'migrating' && html`
        <div class="space-y-3 text-sm">
          <p class="text-text">${t('path_migration.migrating')}</p>
          <progress
            class="w-full h-2"
            value=${bytesCopied}
            max=${totalBytes || 1}
          ></progress>
          <p class="text-text-muted text-xs text-right">
            ${fmtBytes(bytesCopied)} / ${fmtBytes(totalBytes)} (${progressPct}%)
          </p>
        </div>
      `}

      ${phase === 'done' && html`
        <p class="text-success text-sm">${t('path_migration.done')}</p>
      `}

      ${phase === 'error' && html`
        <div class="space-y-3">
          <p class="text-danger text-sm">${migError ?? t('path_migration.error.failed')}</p>
          <p class="text-text-muted text-xs">${t('path_migration.error.not_moved')}</p>
          <button type="button" class="btn-ghost btn-sm" onClick=${onCancel}>${t('common.close')}</button>
        </div>
      `}
    </${Modal}>
  `;
}
