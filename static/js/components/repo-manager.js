// @ts-check

import { h, render } from 'preact';
import htm from 'htm';
import { mountMasterDetail } from './master-detail.js';
import { ListItem } from './list-item.js';
import { Modal, mountIntoModalRoot, showConfirm } from './modal.js';
import { showToast, showApiError } from './toast.js';
import { EmptyState } from './empty-state.js';
import { ErrorState } from './error-state.js';
import * as api from '../api.js';
import { hasPermission } from '../session.js';
import { iconCube } from '../icons.js';
import { t } from '../i18n.js';
import { updateState as updateCacheState } from '../cache.js';
const html = htm.bind(h);

/**
 * @param {HTMLElement} container
 * @returns {{ destroy: () => void }}
 */
export function mountRepoManager(container) {
  const { listEl, detailEl, setView, destroy: destroyLayout } = mountMasterDetail(container, { listWidth: 288 });

  /** @type {any[]} */
  let _repos = [];
  /** @type {any[]} */
  let _sources = [];
  /** @type {number | null} */
  let _selectedRepoId = null;
  /** @type {any[]} */
  let _extensions = [];
  /** @type {'prompt' | 'loading' | 'loaded' | 'error'} */
  let _detailStatus = 'prompt';

  let _tofuPending = /** @type {{ fingerprint: string, repoUrl: string } | null} */ (null);
  let _tofuOpen = false;

  function _renderTofuModal() {
    if (!_tofuPending) { mountIntoModalRoot(null); return; }
    const { fingerprint, repoUrl } = _tofuPending;
    mountIntoModalRoot(html`
      <${Modal}
        open=${_tofuOpen}
        onClose=${_cancelTofu}
        title=${t('repo.tofu.title')}
        footer=${html`
          <button class="btn-ghost btn-sm" onClick=${_cancelTofu}>${t('repo.tofu.cancel')}</button>
          <button class="btn-primary btn-sm" onClick=${() => _confirmTofu(fingerprint, repoUrl)}>
            ${t('repo.tofu.trust')}
          </button>
        `}
      >
        <p class="text-sm text-text-muted">${t('repo.tofu.warning')}</p>
        <p class="text-sm font-medium mt-3">${t('repo.tofu.fingerprint_label')}</p>
        <pre class="font-mono text-xs mt-1 px-3 py-2 bg-surface-2 rounded-lg break-all">${fingerprint}</pre>
        <p class="text-xs text-text-muted mt-2 break-all">${repoUrl}</p>
      </${Modal}>
    `);
  }

  function _cancelTofu() {
    _tofuOpen = false;
    _tofuPending = null;
    mountIntoModalRoot(null);
  }

  async function _confirmTofu(fingerprint, repoUrl) {
    _cancelTofu();
    try {
      const result = await api.addRepo(repoUrl, fingerprint);
      await _loadRepos();
      showToast(t('repo.add.success').replace('{name}', result?.name ?? repoUrl), { type: 'success' });
    } catch (err) {
      showApiError(err);
    }
  }

  /** @param {any} err @param {string} url */
  function _handleAddRepoError(err, url) {
    if (err.status === 428) {
      const fp = err.body?.fingerprint ?? '';
      _tofuPending = { fingerprint: fp, repoUrl: url };
      _tofuOpen = true;
      _renderTofuModal();
    } else if (err.status === 409) {
      const oldFp = err.body?.old_fingerprint ?? '';
      const newFp = err.body?.new_fingerprint ?? '';
      _showKeyChangedBanner(oldFp, newFp, url);
    } else {
      showApiError(err);
    }
  }

  function _showKeyChangedBanner(oldFp, newFp, repoUrl) {
    let banner = listEl.querySelector('.js-key-changed-banner');
    if (banner) banner.remove();
    const bannerEl = document.createElement('div');
    bannerEl.className = 'js-key-changed-banner mx-2 mt-2 p-3 bg-warn/10 border border-warn/30 rounded-lg text-sm flex flex-col gap-2';
    render(html`<${KeyChangedBanner}
      oldFp=${oldFp}
      newFp=${newFp}
      onDismiss=${() => bannerEl.remove()}
      onTrustNew=${async () => {
        bannerEl.remove();
        try {
          await api.addRepo(repoUrl, newFp);
          await _loadRepos();
        } catch (err) {
          showApiError(err);
        }
      }}
    />`, bannerEl);
    const listContent = listEl.querySelector('.js-repo-list-content');
    if (listContent) listEl.insertBefore(bannerEl, listContent);
    else listEl.appendChild(bannerEl);
  }


  function _buildListPane() {
    listEl.innerHTML = '';

    if (hasPermission('source:install')) {
      const formWrap = document.createElement('div');
      listEl.appendChild(formWrap);
      render(html`<${AddRepoForm}
        onAdd=${async (url) => {
          try {
            const result = await api.addRepo(url);
            await _loadRepos();
            showToast(t('repo.add.success').replace('{name}', result?.name ?? url), { type: 'success' });
          } catch (err) {
            _handleAddRepoError(err, url);
          }
        }}
      />`, formWrap);
    }

    const listContent = document.createElement('div');
    listContent.className = 'js-repo-list-content flex-1 overflow-y-auto';
    listEl.appendChild(listContent);

    _renderRepoList(listContent);
  }

  function _renderRepoList(container) {
    render(html`<${RepoList}
      repos=${_repos}
      selectedRepoId=${_selectedRepoId}
      onSelect=${_selectRepo}
    />`, container);
  }


  function _renderDetailPrompt() {
    _detailStatus = 'prompt';
    render(html`<${RepoDetail}
      status="prompt"
      repo=${null}
      extensions=${[]}
      sources=${[]}
      repoId=${null}
      onRefresh=${() => {}}
      onRemove=${() => {}}
    />`, detailEl);
  }

  async function _selectRepo(repoId) {
    _selectedRepoId = repoId;
    _detailStatus = 'loading';
    setView('detail');

    const listContent = listEl.querySelector('.js-repo-list-content');
    if (listContent) _renderRepoList(/** @type {HTMLElement} */ (listContent));

    render(html`<${RepoDetail} status="loading" repo=${null} extensions=${[]} sources=${[]} repoId=${repoId} onRefresh=${() => {}} onRemove=${() => {}} />`, detailEl);

    try {
      const [extensions, sources] = await Promise.all([
        api.listRepoExtensions(repoId),
        api.getSources(),
      ]);
      _extensions = Array.isArray(extensions) ? extensions : [];
      _sources = Array.isArray(sources) ? sources : [];
      _detailStatus = 'loaded';
      _renderDetail(repoId);
    } catch {
      _detailStatus = 'error';
      render(html`<${RepoDetail}
        status="error"
        repo=${null}
        extensions=${[]}
        sources=${[]}
        repoId=${repoId}
        onRefresh=${() => _selectRepo(repoId)}
        onRemove=${() => {}}
      />`, detailEl);
    }
  }

  function _renderDetail(repoId) {
    const repo = _repos.find(r => r.id === repoId);
    if (!repo) return;
    render(html`<${RepoDetail}
      status="loaded"
      repo=${repo}
      extensions=${_extensions}
      sources=${_sources}
      repoId=${repoId}
      onRefresh=${async () => {
        try {
          await api.refreshRepo(repoId);
          await _loadRepos();
          _selectRepo(repoId);
        } catch (err) {
          showApiError(err);
        }
      }}
      onRemove=${async () => {
        const ok = await showConfirm(t('repo.remove.confirm'));
        if (!ok) return;
        try {
          await api.removeRepo(repoId);
          _selectedRepoId = null;
          await _loadRepos();
          _renderDetailPrompt();
          setView('list');
        } catch (err) {
          showApiError(err);
        }
      }}
    />`, detailEl);
  }


  async function _loadRepos() {
    try {
      _repos = await api.listRepos();
    } catch { _repos = []; }

    const listContent = listEl.querySelector('.js-repo-list-content');
    if (listContent) _renderRepoList(/** @type {HTMLElement} */ (listContent));
  }


  function _onSse(e) {
    const d = /** @type {any} */ (e).detail;
    if (!d?.type) return;
    if (d.type === 'source_updating') {
      showToast(t('repo.source_updating', { name: d.source_name ?? '?' }));
      return;
    }
    if (d.type === 'repo_refreshed' || d.type === 'update_available' || d.type === 'source_installed') {
      _loadRepos();
      if (_selectedRepoId !== null) _selectRepo(_selectedRepoId);
      // Covers an install performed elsewhere (another tab, or the server
      // installing on its own behalf), which never runs handleAction here.
      if (d.type === 'source_installed') updateCacheState('sourcesInvalidation', n => n + 1);
    }
  }
  window.addEventListener('kani:sse', _onSse);

  _buildListPane();
  _renderDetailPrompt();
  _loadRepos();

  return {
    destroy() {
      window.removeEventListener('kani:sse', _onSse);
      mountIntoModalRoot(null);
      destroyLayout();
    },
  };
}


function AddRepoForm({ onAdd }) {
  async function handleSubmit(e) {
    e.preventDefault();
    const form = /** @type {HTMLFormElement} */ (e.currentTarget);
    const input = /** @type {HTMLInputElement} */ (form.querySelector('input'));
    const btn = /** @type {HTMLButtonElement} */ (form.querySelector('button[type="submit"]'));
    const url = input.value.trim();
    if (!url) return;
    btn.disabled = true;
    btn.textContent = t('repo.add.adding');
    try {
      await onAdd(url);
      input.value = '';
    } finally {
      btn.disabled = false;
      btn.textContent = t('repo.add.button');
    }
  }

  return html`
    <form class="p-3 border-b border-border-subtle flex gap-2" onSubmit=${handleSubmit}>
      <input
        type="url"
        class="input input-sm flex-1 min-w-0"
        placeholder=${t('repo.add.placeholder')}
        aria-label=${t('repo.add.placeholder')}
      />
      <button type="submit" class="btn-primary btn-sm shrink-0">${t('repo.add.button')}</button>
    </form>
  `;
}

function KeyChangedBanner({ oldFp, newFp, onDismiss, onTrustNew }) {
  return html`
    <div>
      <p class="font-medium text-warn">${t('repo.key_changed.title')}</p>
      <p class="text-xs text-text-muted">${t('repo.key_changed.warning')}</p>
      <p class="text-xs"><span class="text-text-muted">${t('repo.key_changed.old')}:</span> <span class="font-mono break-all">${oldFp}</span></p>
      <p class="text-xs"><span class="text-text-muted">${t('repo.key_changed.new')}:</span> <span class="font-mono break-all">${newFp}</span></p>
      <div class="flex gap-2 justify-end">
        <button class="btn-ghost btn-sm" onClick=${onDismiss}>${t('common.dismiss')}</button>
        <button class="btn-danger btn-sm" onClick=${onTrustNew}>${t('repo.key_changed.trust')}</button>
      </div>
    </div>
  `;
}

function RepoList({ repos, selectedRepoId, onSelect }) {
  if (repos.length === 0) {
    return html`<${EmptyState} icon=${iconCube} title=${t('repo.empty.title')} subtitle=${t('repo.empty.subtitle')} />`;
  }
  return html`
    <div>
      ${repos.map(repo => {
        const extCount = _countExtensions(repo);
        const subtitle = t('repo.list.extension_count').replace('{count}', String(extCount));
        return html`<${ListItem}
          key=${repo.id}
          title=${repo.name}
          subtitle=${subtitle}
          active=${repo.id === selectedRepoId}
          onClick=${() => onSelect(repo.id)}
        />`;
      })}
    </div>
  `;
}

function RepoDetail({ status, repo, extensions, sources, repoId, onRefresh, onRemove }) {
  if (status === 'prompt') {
    return html`<${EmptyState} icon=${iconCube} title=${t('repo.select_prompt')} />`;
  }
  if (status === 'loading') {
    return html`<div class="p-6 text-sm text-text-muted">${t('common.loading')}</div>`;
  }
  if (status === 'error') {
    return html`<${ErrorState} message=${t('repo.extensions.load_failed')} onRetry=${onRefresh} />`;
  }

  return html`
    <div>
      <div class="flex items-center justify-between gap-3 px-6 py-4 border-b border-border-subtle">
        <div class="min-w-0">
          <p class="font-medium text-text truncate">${repo.name}</p>
          <p class="text-xs text-text-muted mt-0.5 truncate">${repo.url ?? ''}</p>
        </div>
        ${hasPermission('source:install') && html`
          <div class="flex gap-2 shrink-0">
            <button class="btn-ghost btn-sm" onClick=${onRefresh}>${t('repo.action.refresh')}</button>
            <button class="btn-ghost btn-sm text-danger" onClick=${onRemove}>${t('repo.action.remove')}</button>
          </div>
        `}
      </div>
      <div class="flex flex-col divide-y divide-border-subtle">
        ${extensions.length === 0
          ? html`<${EmptyState} title=${t('repo.extensions.empty')} />`
          : extensions.map(ext => html`<${ExtensionRow}
              key=${ext.id}
              ext=${ext}
              repoId=${repoId}
              sources=${sources}
              onInstalled=${onRefresh}
            />`)
        }
      </div>
    </div>
  `;
}

function ExtensionRow({ ext, repoId, sources, onInstalled }) {
  const installedSource = sources.find(s =>
    s.name?.toLowerCase() === ext.name?.toLowerCase() || s.base_url?.includes(ext.id)
  );
  const isInstalled = !!installedSource;
  const hasUpdate = isInstalled && _isNewerVersion(ext.version, installedSource.version);

  async function handleAction(btn, action, successMsg) {
    btn.disabled = true;
    const origText = btn.textContent;
    btn.textContent = action === 'update' ? t('repo.extensions.updating') : t('repo.extensions.installing');
    try {
      if (action === 'update') {
        await api.updateFromRepo(repoId, ext.id, installedSource.id);
      } else {
        await api.installFromRepo(repoId, ext.id);
      }
      showToast(successMsg, { type: 'success' });
      onInstalled();
      // The repo browser refreshes its own "Installed" badges, but the sources
      // list is a separate page component with no other way to learn about
      // this. Bump the counter it subscribes to rather than reaching across.
      updateCacheState('sourcesInvalidation', n => n + 1);
    } catch (err) {
      showApiError(err);
      btn.disabled = false;
      btn.textContent = origText;
    }
  }

  const parts = ['v' + ext.version];
  if (ext.language) parts.push(ext.language);
  if (ext.format) parts.push(ext.format.toUpperCase());

  return html`
    <div class="flex items-center gap-4 px-6 py-3">
      <div class="flex-1 min-w-0 flex flex-col gap-0.5">
        <div class="flex items-center gap-2">
          <span class="text-sm font-medium text-text">${ext.name}</span>
          ${hasUpdate && html`<span class="text-2xs px-1.5 py-0.5 rounded-full bg-accent/15 text-accent font-medium">${t('repo.extensions.update_available')}</span>`}
          ${!hasUpdate && isInstalled && html`<span class="text-2xs px-1.5 py-0.5 rounded-full bg-surface-2 text-text-muted">${t('repo.extensions.installed')}</span>`}
          ${ext.nsfw && html`<span class="text-2xs px-1.5 py-0.5 rounded-full bg-danger/15 text-danger font-medium">${t('repo.extensions.nsfw')}</span>`}
        </div>
        <p class="text-xs text-text-muted">${parts.join(' · ')}</p>
        ${ext.description && html`<p class="text-xs text-text-faint mt-0.5 line-clamp-1">${ext.description}</p>`}
      </div>
      ${hasPermission('source:install') && html`
        <button
          type="button"
          class=${hasUpdate ? 'btn-secondary btn-sm shrink-0' : isInstalled ? 'btn-ghost btn-sm shrink-0' : 'btn-secondary btn-sm shrink-0'}
          disabled=${isInstalled && !hasUpdate}
          onClick=${(/** @type {MouseEvent} */ e) => {
            const btn = /** @type {HTMLButtonElement} */ (e.currentTarget);
            if (hasUpdate) {
              handleAction(btn, 'update', t('repo.extensions.update_success').replace('{name}', ext.name));
            } else if (!isInstalled) {
              handleAction(btn, 'install', t('repo.extensions.install_success').replace('{name}', ext.name));
            }
          }}
        >
          ${hasUpdate ? t('repo.extensions.update') : isInstalled ? t('repo.extensions.installed') : t('repo.extensions.install')}
        </button>
      `}
    </div>
  `;
}


function _countExtensions(repo) {
  if (!repo.index_cache) return 0;
  try {
    const index = JSON.parse(repo.index_cache);
    return Array.isArray(index.extensions) ? index.extensions.length : 0;
  } catch { return 0; }
}

function _isNewerVersion(versionA, versionB) {
  const parse = (v) => String(v ?? '0').split('.').map(n => parseInt(n, 10) || 0);
  const a = parse(versionA);
  const b = parse(versionB);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0);
    if (diff !== 0) return diff > 0;
  }
  return false;
}
