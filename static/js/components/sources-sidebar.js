// @ts-check
// SourcesSidebar — desktop sources sidebar with search, star toggles, and add source.
// AddSourceModal is co-located here since it is only used with this sidebar.

import { h } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { Modal } from './modal.js';
import { SearchInput } from './form/search-input.js';
import { SidebarNav } from './sidebar-nav.js';
import { iconStarFilled, iconStarOutline } from '../icons.js';
import { navigate } from '../router.js';
import { t } from '../i18n.js';
import { StatusDot } from './status-dot.js';
import { Tabs } from './tabs.js';
const html = htm.bind(h);

/** True if a filename / URL path points at an interpreted-YAML extension. */
function _isYamlName(name) {
  return /\.ya?ml$/i.test(name ?? '');
}


/**
 * Modal for adding a new source from a WASM URL or file upload.
 * @param {{
 *   open: boolean,
 *   onClose: () => void,
 *   onCreated: () => void,
 * }} props
 */
export function AddSourceModal({ open, onClose, onCreated }) {
  const [mode, setMode]         = useState(/** @type {'url'|'file'|'yaml'} */ ('url'));
  const [wasmUrl, setWasmUrl]   = useState('');
  const [wasmFile, setWasmFile] = useState(/** @type {File|null} */ (null));
  const [yamlText, setYamlText] = useState('');
  const [loading, setLoading]   = useState(false);
  const [error, setError]       = useState(/** @type {string|null} */ (null));

  useEffect(() => {
    if (open) {
      setMode('url');
      setWasmUrl('');
      setWasmFile(null);
      setYamlText('');
      setLoading(false);
      setError(null);
    }
  }, [open]);

  // Block close while install is in-flight to prevent orphaned source records
  const handleClose = () => { if (!loading) onClose(); };

  const canSubmit = mode === 'url' ? !!wasmUrl.trim()
    : mode === 'file' ? !!wasmFile
    : !!yamlText.trim();

  async function handleSubmit() {
    if (mode === 'url') {
      if (!wasmUrl.trim()) { setError(t('source.add.error.url_required')); return; }
      if (!wasmUrl.trim().startsWith('https://')) { setError(t('source.add.error.url_https')); return; }
    } else if (mode === 'file') {
      if (!wasmFile) { setError(t('source.add.error.file_required')); return; }
    } else if (!yamlText.trim()) {
      setError(t('source.add.error.yaml_required')); return;
    }

    setLoading(true);
    setError(null);

    const url = wasmUrl.trim();
    const isYaml = mode === 'yaml'
      || (mode === 'url' && _isYamlName(new URL(url).pathname))
      || (mode === 'file' && _isYamlName(/** @type {File} */ (wasmFile).name));

    try {
      if (isYaml) {
        if (mode === 'yaml') {
          await api.installYaml(yamlText);
        } else if (mode === 'url') {
          await api.fetchYaml(url);
        } else {
          const text = await /** @type {File} */ (wasmFile).text();
          await api.installYaml(text);
        }
      } else {
        if (mode === 'url') await api.fetchWasm(url);
        else await api.uploadWasm(/** @type {File} */ (wasmFile));
      }
    } catch (e) {
      setError(/** @type {any} */ (e)?.message ?? t('source.add.error.install_failed'));
      setLoading(false);
      return;
    }

    setLoading(false);
    onClose();
    onCreated();
  }

  const footer = html`
    <button class="btn-ghost" disabled=${loading} onClick=${handleClose}>${t('common.cancel')}</button>
    <button
      class="btn-primary"
      disabled=${loading || !canSubmit}
      onClick=${handleSubmit}
    >${loading ? t('source.add.installing') : t('source.add.btn')}</button>
  `;

  return html`
    <${Modal} open=${open} onClose=${handleClose} title=${t('source.add.title')} footer=${footer}>
      <div class="flex flex-col gap-4">

        <${Tabs}
          variant="pill"
          tabs=${[
            { id: 'url', name: t('source.add.mode.url') },
            { id: 'file', name: t('source.add.mode.file') },
            { id: 'yaml', name: t('source.add.mode.yaml') },
          ]}
          activeId=${mode}
          onSelect=${(/** @type {string} */ id) => { if (!loading) setMode(/** @type {any} */ (id)); }}
        />

        ${mode === 'url' && html`
          <div class="flex flex-col gap-1.5">
            <label class="text-sm font-medium text-text" for="add-source-url">${t('source.add.url.label')}</label>
            <input
              id="add-source-url"
              type="url"
              class="input"
              placeholder="https://example.com/extension.wasm"
              value=${wasmUrl}
              disabled=${loading}
              onInput=${(/** @type {any} */ e) => setWasmUrl(e.target.value)}
              onKeyDown=${(/** @type {KeyboardEvent} */ e) => { if (e.key === 'Enter') handleSubmit(); }}
            />
            <p class="text-xs text-text-muted">${t('source.add.url.hint')}</p>
          </div>
        `}

        ${mode === 'file' && html`
          <div class="flex flex-col gap-1.5">
            <label class="text-sm font-medium text-text" for="add-source-file">${t('source.add.file.label')}</label>
            <input
              id="add-source-file"
              type="file"
              class="input"
              accept=".wasm,.yaml,.yml"
              disabled=${loading}
              onChange=${(/** @type {any} */ e) => setWasmFile(e.target.files?.[0] ?? null)}
            />
            <p class="text-xs text-text-muted">${t('source.add.file.hint')}</p>
          </div>
        `}

        ${mode === 'yaml' && html`
          <div class="flex flex-col gap-1.5">
            <label class="text-sm font-medium text-text" for="add-source-yaml">${t('source.add.yaml.label')}</label>
            <textarea
              id="add-source-yaml"
              class="input font-mono text-xs resize-y min-h-48 p-3"
              placeholder=${t('source.add.yaml.placeholder')}
              value=${yamlText}
              disabled=${loading}
              onInput=${(/** @type {any} */ e) => setYamlText(e.target.value)}
            ></textarea>
            <p class="text-xs text-text-muted">${t('source.add.yaml.hint')}</p>
          </div>
        `}

        ${error && html`<p class="text-sm text-danger">${error}</p>`}

      </div>
    </${Modal}>
  `;
}


/**
 * Desktop sources sidebar: search input, source list with star toggles, and add source button.
 * Receives sources from the parent; call onCreated to signal a refresh is needed.
 *
 * @param {{
 *   sources: any[],
 *   activeSourceId?: number,
 *   onCreated?: () => void,
 * }} props
 */
export function SourcesSidebar({ sources, activeSourceId, onCreated }) {
  const [query, setQuery]     = useState('');
  const [starred, setStarred] = useState(() => _buildStarred(sources));

  useEffect(() => {
    setStarred(prev => {
      /** @type {Record<number, boolean>} */
      const next = {};
      for (const s of sources) {
        next[s.id] = s.id in prev ? prev[s.id] : (s.favourited ?? false);
      }
      return next;
    });
  }, [sources]);

  /** @param {any[]} srcs @returns {Record<number, boolean>} */
  function _buildStarred(srcs) {
    /** @type {Record<number, boolean>} */
    const m = {};
    for (const s of srcs) m[s.id] = s.favourited ?? false;
    return m;
  }

  async function _toggleStar(/** @type {number} */ id, /** @type {boolean} */ current) {
    const newVal = !current;
    setStarred(prev => ({ ...prev, [id]: newVal }));
    try {
      await api.toggleSourceFavourite(id, newVal);
    } catch {
      setStarred(prev => ({ ...prev, [id]: current }));
    }
  }

  const filtered = query
    ? sources.filter(s => s.name?.toLowerCase().includes(query.toLowerCase()))
    : sources;

  return html`
    <div class="p-3 border-b border-border-subtle">
      <${SearchInput}
        size="sm"
        value=${query}
        onInput=${setQuery}
        placeholder=${t('source.sidebar.filter.placeholder')}
        ariaLabel=${t('source.sidebar.filter.label')}
      />
    </div>

    <${SidebarNav}>
      ${filtered.length === 0
        ? html`<p class="text-xs text-text-muted px-3 py-2">${t('source.sidebar.empty')}</p>`
        : filtered.map(src => {
            const isActive  = src.id === activeSourceId;
            const isStarred = starred[src.id] ?? false;
            const initial   = (src.name ?? '?')[0].toUpperCase();
            return html`
              <div
                key=${src.id}
                class=${['li-row group cursor-pointer', isActive ? 'active' : '', !src.enabled ? 'opacity-60' : ''].filter(Boolean).join(' ')}
                style="padding: 9px 12px; gap: 10px;"
                role="link"
                tabIndex=${0}
                aria-current=${isActive ? 'page' : 'false'}
                onClick=${(/** @type {MouseEvent} */ e) => {
                  if (/** @type {HTMLElement} */ (e.target).closest('button')) return;
                  navigate(`/source/${src.id}`);
                }}
                onKeyDown=${(/** @type {KeyboardEvent} */ e) => {
                  if (e.key === 'Enter') navigate(`/source/${src.id}`);
                }}
              >
                <div class="flex items-center gap-3 border-b border-border-subtle last:border-0 w-full">
                    ${src.icon
                      ? html`<img src=${`data:image/png;base64,${src.icon}`} alt="" class="avatar shrink-0 object-contain" style="background:var(--color-surface-3)" />`
                      : html`<span
                          class="avatar shrink-0"
                          style="background:var(--color-surface-3);color:var(--color-text-muted)"
                          aria-hidden="true"
                      >${initial}</span>`
                    }
                    <span class="flex flex-col min-w-0 flex-1">
                        <span class="li-title truncate">${src.name}</span>
                        <span class="li-sub flex items-center gap-1.5">
                            <span>v${(src.version ?? '').replace('+debug', '')}${src.language ? ` · ${src.language}` : ''}</span>
                            ${src.version?.includes('+debug') && html`<span class="text-2xs px-1 py-0.5 rounded bg-warn/20 text-warn font-medium leading-none">DEBUG</span>`}
                            ${!src.enabled && html`<span class="text-2xs px-1 py-0.5 rounded bg-warn/20 text-warn font-medium leading-none">${t('source.status.off')}</span>`}
                            ${src.circuit_state === 'open' && html`<${StatusDot} state="open" label=${t('source.circuit.open')} />`}
                            ${src.circuit_state === 'half_open' && html`<${StatusDot} state="half_open" label=${t('source.circuit.half_open')} />`}
                        </span>
                    </span>
                    <button
                        type="button"
                        class=${[
                            'shrink-0 p-1 rounded-md transition-colors icon-xs',
                            'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent',
                            isStarred ? 'text-accent' : 'text-text-faint opacity-0 group-hover:opacity-100',
                        ].join(' ')}
                        aria-label=${isStarred ? t('source.sidebar.unstar') : t('source.sidebar.star')}
                        onClick=${(/** @type {MouseEvent} */ e) => {
                            e.preventDefault();
                            e.stopPropagation();
                            _toggleStar(src.id, isStarred);
                        }}
                        dangerouslySetInnerHTML=${{ __html: isStarred ? iconStarFilled : iconStarOutline }}
                    />
                </div>
              </div>
            `;
          })
      }
    <//>

  `;
}
