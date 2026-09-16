// @ts-check
// Folder browser modal — lets users pick or create a directory on the server.

import { h } from 'preact';
import { useState, useEffect, useCallback } from 'preact/hooks';
import htm from 'htm';
import { Modal } from './modal.js';
import { Icon } from './icon.js';
import { EmptyState } from './empty-state.js';
import { iconFolder, iconChevronRight, iconSpinner } from '../icons.js';
import * as api from '../api.js';
import { t } from '../i18n.js';

const html = htm.bind(h);

/**
 * @param {{
 *   open: boolean,
 *   onClose: () => void,
 *   onSelect: (path: string) => void,
 *   initialPath?: string,
 * }} props
 */
export function FolderPicker({ open, onClose, onSelect, initialPath = '/' }) {
  const [currentPath, setCurrentPath] = useState(initialPath);
  const [dirs, setDirs] = useState(/** @type {string[]} */ ([]));
  const [segments, setSegments] = useState(/** @type {string[]} */ ([]));
  const [drives, setDrives] = useState(/** @type {string[]} */ ([]));
  const [selectedPath, setSelectedPath] = useState(initialPath);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(/** @type {string|null} */ (null));
  const [newFolderName, setNewFolderName] = useState('');
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState(/** @type {string|null} */ (null));

  const load = useCallback(/** @param {string} path */ async (path) => {
    setLoading(true);
    setError(null);
    try {
      const result = await api.fsBrowse(path);
      setCurrentPath(result.path);
      setSelectedPath(result.path);
      setDirs(result.dirs ?? []);
      setSegments(result.segments ?? []);
      setDrives(result.drives ?? []);
    } catch (/** @type {any} */ e) {
      setError(e?.message ?? t('folder_picker.error.read_dir'));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (open) load(initialPath);
  }, [open, initialPath, load]);

  /** Build path from segments up to index i (inclusive). */
  function buildPath(segs, i) {
    const parts = segs.slice(0, i + 1);
    if (parts.length === 0) return '/';
    if (parts[0].endsWith(':\\') || parts[0].endsWith(':/')) {
      const [drive, ...rest] = parts;
      if (rest.length === 0) return drive;
      return drive + rest.join('/');
    }
    return parts.map((p, j) => (j === 0 && p === '/') ? '' : p).join('/') || '/';
  }

  async function handleCreate() {
    if (!newFolderName.trim()) return;
    setCreating(true);
    setCreateError(null);
    try {
      const result = await api.fsMkdir(currentPath, newFolderName.trim());
      setNewFolderName('');
      await load(result.path);
    } catch (/** @type {any} */ e) {
      setCreateError(e?.message ?? t('folder_picker.error.create_folder'));
    } finally {
      setCreating(false);
    }
  }

  const footer = html`
    <button type="button" class="btn-ghost btn-sm" onClick=${onClose}>${t('common.cancel')}</button>
    <button type="button" class="btn-primary btn-sm" onClick=${() => onSelect(selectedPath)}>
      ${t('folder_picker.select')}
    </button>
  `;

  return html`
    <${Modal} open=${open} onClose=${onClose} title=${t('folder_picker.title')} wide=${true} footer=${footer}>
      ${loading && html`
        <p class="flex items-center gap-2 text-sm text-text-muted py-2">
          <span class="icon-sm"><${Icon} svg=${iconSpinner} /></span>
          ${t('common.loading')}
        </p>
      `}
      ${error && html`<p class="text-sm text-danger py-2">${error}</p>`}

      ${/* Drives (Windows only, shown at root level) */ drives.length > 0 && html`
        <div class="mb-3">
          <p class="text-xs text-text-muted mb-1 uppercase tracking-wide">${t('folder_picker.drives')}</p>
          <div class="flex flex-wrap gap-2">
            ${drives.map(d => html`
              <button
                type="button"
                class="btn-ghost btn-sm font-mono"
                onClick=${() => load(d)}
              >${d}</button>
            `)}
          </div>
        </div>
      `}

      ${/* Breadcrumb */ segments.length > 0 && html`
        <nav class="flex items-center flex-wrap gap-1 text-sm mb-3" aria-label=${t('folder_picker.breadcrumb')}>
          ${segments.map((seg, i) => html`
            ${i > 0 && html`
              <span class="icon-xs text-text-faint" aria-hidden="true"><${Icon} svg=${iconChevronRight} /></span>
            `}
            <button
              type="button"
              class=${'touch-min-h font-mono px-2 py-0.5 rounded ' + (i === segments.length - 1
                ? 'text-text font-medium'
                : 'text-text-muted hover:text-text hover:bg-surface-2')}
              aria-current=${i === segments.length - 1 ? 'location' : undefined}
              onClick=${() => load(buildPath(segments, i))}
            >${seg}</button>
          `)}
        </nav>
      `}

      ${/* Directory list */ !loading && html`
        <div class="border border-border rounded-lg overflow-hidden mb-3 max-h-72 overflow-y-auto">
          ${dirs.length === 0
            ? html`<${EmptyState} compact=${true} title=${t('folder_picker.no_subdirs')} />`
            : html`
              <ul class="divide-y divide-border-subtle">
                ${dirs.map(dir => html`
                  <li key=${dir}>
                    <button
                      type="button"
                      class="w-full text-left flex items-center gap-2.5 px-3 py-2 text-sm hover:bg-surface-2 focus-visible:bg-surface-2"
                      onClick=${() => {
                        const next = currentPath.endsWith('/') || currentPath.endsWith('\\')
                          ? currentPath + dir
                          : currentPath + '/' + dir;
                        setSelectedPath(next);
                        load(next);
                      }}
                    >
                      <span class="icon-sm text-text-faint shrink-0"><${Icon} svg=${iconFolder} /></span>
                      <span class="truncate">${dir}</span>
                      <span class="icon-xs text-text-faint ml-auto shrink-0" aria-hidden="true">
                        <${Icon} svg=${iconChevronRight} />
                      </span>
                    </button>
                  </li>
                `)}
              </ul>
            `}
        </div>
      `}

      <div class="flex items-center gap-2 mt-2">
        <input
          type="text"
          class="input text-sm flex-1"
          aria-label=${t('folder_picker.new_folder')}
          placeholder=${t('folder_picker.new_folder_placeholder')}
          value=${newFolderName}
          onInput=${(/** @type {any} */ e) => { setNewFolderName(e.target.value); setCreateError(null); }}
          onKeyDown=${(/** @type {any} */ e) => { if (e.key === 'Enter') handleCreate(); }}
          disabled=${creating}
        />
        <button
          type="button"
          class="btn-ghost btn-sm"
          onClick=${handleCreate}
          disabled=${creating || !newFolderName.trim()}
        >${creating ? t('folder_picker.creating') : t('folder_picker.new_folder')}</button>
      </div>
      ${createError && html`<p class="text-xs text-danger mt-1">${createError}</p>`}

      <div class="mt-4 pt-3 border-t border-border-subtle">
        <p class="text-xs uppercase tracking-wide text-text-muted">${t('folder_picker.selected')}</p>
        <p class="text-sm font-mono text-text break-all mt-0.5">${selectedPath}</p>
      </div>
    </${Modal}>
  `;
}
