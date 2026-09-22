// @ts-check

import { h, render } from 'preact';
import { useState, useEffect } from 'preact/hooks';
import htm from 'htm';
import * as api from '../api.js';
import { hasPermission } from '../session.js';
import { escapeHtml, formatDate } from '../utils.js';
import { t } from '../i18n.js';
import { showToast, showApiError } from '../components/toast.js';
import { Modal, mountIntoModalRoot, showConfirm } from '../components/modal.js';
import { mountMasterDetail } from '../components/master-detail.js';
import { renderTabs } from '../components/tabs.js';
import { iconPencil, iconX, iconAccounts } from '../icons.js';
import { ActivityFeed } from '../components/activity-feed.js';
import { setPageHeader, clearPageHeader } from '../components/app-header.js';
import { EmptyState } from '../components/empty-state.js';
import { createSearchInput } from '../components/form/search-input.js';
import { ListItem } from '../components/list-item.js';
import { pushState } from '../url-params.js';
const html = htm.bind(h);


/** @type {any[]} */ let _users = [];
/** @type {any[]} */ let _roles = [];
/** @type {HTMLElement | null} */ let _container = null;
/** @type {(() => void) | null} */ let _destroyMasterDetail = null;
/** @type {'users' | 'roles'} */ let _activeTab = 'users';
/** @type {any | null} */ let _selected = null;

const ALL_PERMISSIONS = [
  'library:view', 'library:add', 'library:delete', 'library:refresh', 'library:manage',
  'chapter:download', 'chapter:delete',
  'source:browse', 'source:install', 'source:delete', 'source:toggle_enabled', 'source:configure',
  'settings:view', 'settings:edit_download', 'settings:edit_scan', 'settings:edit_advanced',
  'user:manage',
  'server:manage',
];


/** @param {HTMLElement} container */
export async function init(container) {
  document.title = t('accounts.page_title');
  _container = container;
  _activeTab = 'users';
  _selected = null;

  setPageHeader({ crumbs: [{ label: t('accounts.crumb') }] });

  if (!hasPermission('user:manage')) {
    container.innerHTML = `
      <div class="flex flex-col items-center justify-center gap-3 py-20 text-text-muted">
        <p class="text-base font-medium text-text">${t('accounts.denied.title')}</p>
        <p class="text-sm">${t('accounts.denied.desc')}</p>
      </div>
    `;
    return;
  }

  const contentEl = document.createElement('div');
  contentEl.style.cssText = 'display:flex;flex-direction:column;overflow:hidden;height:100%;min-height:0;';

  container.innerHTML = '';
  container.appendChild(contentEl);

  await _reload();

  // Restore tab + selection from URL
  const _qs = new URLSearchParams(location.search);
  const _tabParam = _qs.get('tab');
  if (_tabParam === 'roles') _activeTab = 'roles';
  const _userParam  = _qs.get('user');
  const _roleParam  = _qs.get('role');
  if (_userParam) _selected = _users.find(u => u.id === Number(_userParam)) ?? null;
  if (_roleParam) _selected = _roles.find(r => r.slug === _roleParam) ?? null;

  _mountMasterDetail(contentEl);
  _updateHeaderActions();
}

/** @param {HTMLElement} container */
export function destroy(container) {
  clearPageHeader();
  mountIntoModalRoot(null);
  _destroyMasterDetail?.();
  _destroyMasterDetail = null;
  _container = null;
  _users = [];
  _roles = [];
  _activeTab = 'users';
  _selected = null;
}

function _updateHeaderActions() {
  const addBtn = document.createElement('button');
  addBtn.type = 'button';
  addBtn.className = 'btn-primary btn-sm';
  addBtn.textContent = _activeTab === 'users' ? t('accounts.add_user') : t('accounts.add_role');
  addBtn.addEventListener('click', () => {
    if (_activeTab === 'users') {
      _showUserModal(null, async () => { await _reload(); _rerenderList(); });
    } else {
      _showRoleModal(null, async () => { await _reload(); _rerenderList(); });
    }
  });

  const tabLabel = _activeTab === 'users' ? t('accounts.tab.users') : t('accounts.tab.roles');
  const crumbs = [{ label: t('accounts.crumb'), href: '/accounts' }];
  if (_selected) {
    crumbs.push({ label: tabLabel, href: '/accounts?tab=' + _activeTab });
    crumbs.push({ label: _activeTab === 'users' ? _selected.username : _selected.slug });
  } else {
    crumbs.push({ label: tabLabel });
  }

  setPageHeader({ crumbs, actions: addBtn });
}


async function _reload() {
  const [usersRes, rolesRes] = await Promise.allSettled([
    api.adminListUsers(),
    api.adminListRoles(),
  ]);
  _users = usersRes.status === 'fulfilled' ? usersRes.value ?? [] : [];
  _roles = rolesRes.status === 'fulfilled' ? rolesRes.value ?? [] : [];
}


/** @type {HTMLElement | null} */ let _listEl = null;
/** @type {HTMLElement | null} */ let _detailEl = null;
/** @type {((v: 'list'|'detail') => void) | null} */ let _setMdView = null;

/** Re-renders the list pane (called after data changes). */
function _rerenderList() {
  if (_listEl) _renderList(_listEl);
}

/** @param {HTMLElement} el */
function _mountMasterDetail(el) {
  const { listEl, detailEl, setView, destroy } = mountMasterDetail(el);
  _destroyMasterDetail = destroy;
  _listEl = listEl;
  _detailEl = detailEl;
  _setMdView = setView;

  _renderList(listEl);
  _renderDetail(detailEl);
  setView(_selected ? 'detail' : 'list');
}

/** @param {HTMLElement} detailEl */
function _renderDetail(detailEl) {
  if (!_selected) {
    detailEl.innerHTML = `
      <div class="flex flex-col items-center justify-center gap-3 h-full py-20 text-text-muted">
        <span class="icon-2xl opacity-30" aria-hidden="true">${iconAccounts}</span>
        <p class="text-sm">${_activeTab === 'users' ? t('accounts.detail.empty.user') : t('accounts.detail.empty.role')}</p>
      </div>
    `;
    return;
  }
  if (_activeTab === 'users') {
    _renderUserDetail(detailEl, _selected);
  } else {
    _renderRoleDetail(detailEl, _selected);
  }
  // On mobile, prepend a back affordance that returns to the list pane.
  const backBtn = document.createElement('button');
  backBtn.type = 'button';
  backBtn.className = 'btn-ghost btn-sm md:hidden mb-3 ml-4 mt-4';
  backBtn.textContent = t('accounts.back');
  backBtn.addEventListener('click', () => {
    _selected = null;
    _setMdView?.('list');
    _rerenderList();
    _renderDetail(detailEl);
    _updateHeaderActions();
  });
  detailEl.prepend(backBtn);
}

/** @param {HTMLElement} listEl */
function _renderList(listEl) {
  listEl.innerHTML = '';

  const headerEl = document.createElement('div');
  headerEl.className = 'list-pane-header';

  const tabsEl = document.createElement('div');
  const tabsHandle = renderTabs(tabsEl, {
    tabs: [
      { id: 'users', name: t('accounts.tab.users'), count: _users.length },
      { id: 'roles', name: t('accounts.tab.roles'), count: _roles.length },
    ],
    activeId: _activeTab,
    stretch: true,
    onSelect: (id) => {
      _activeTab = /** @type {'users' | 'roles'} */ (id);
      _selected = null;
      pushState({ tab: id });
      tabsHandle.update(id);
      _updateHeaderActions();
      _renderList(listEl);
      if (_detailEl) _renderDetail(_detailEl);
    },
  });
  headerEl.appendChild(tabsEl);

  const { el: searchEl, input: searchInput } = createSearchInput({
    size: 'sm',
    placeholder: t('accounts.search'),
  });
  headerEl.appendChild(searchEl);
  listEl.appendChild(headerEl);

  const bodyEl = document.createElement('div');
  bodyEl.className = 'list-pane-body';

  const items = _activeTab === 'users' ? _users : _roles;
  let filter = '';

  /** @param {any} item */
  function _selectItem(item) {
    _selected = item;
    pushState(_activeTab === 'users'
      ? { tab: _activeTab, user: item.id }
      : { tab: _activeTab, role: item.slug });
    _renderList(listEl);
    _updateHeaderActions();
    if (_detailEl) _renderDetail(_detailEl);
    _setMdView?.('detail');
  }

  function renderItems() {
    const filtered = filter
      ? items.filter(i => (i.username ?? i.slug ?? '').toLowerCase().includes(filter.toLowerCase()))
      : items;

    if (filtered.length === 0) {
      const emptyTitle = filter
        ? t('accounts.empty.no_results')
        : _activeTab === 'users' ? t('accounts.empty.users') : t('accounts.empty.roles');
      const emptySub = filter ? t('accounts.empty.no_results.desc') : undefined;
      render(html`<${EmptyState} title=${emptyTitle} subtitle=${emptySub} />`, bodyEl);
      return;
    }

    render(html`${filtered.map(item => {
      const isActive = _selected != null && (
        _activeTab === 'users' ? _selected.id   === item.id
                               : _selected.slug === item.slug
      );
      if (_activeTab === 'users') {
        return html`<${ListItem}
          key=${item.id}
          avatar=${(item.username ?? '?')[0].toUpperCase()}
          title=${item.username}
          subtitle=${item.roles?.join(', ') ?? ''}
          right=${html`<span class=${item.is_active ? 'badge badge-success' : 'badge badge-danger'}>${item.is_active ? t('accounts.user.active') : t('accounts.user.inactive')}</span>`}
          active=${isActive}
          onClick=${() => _selectItem(item)}
        />`;
      }
      const isSystemRole = item.slug === 'user' || item.slug === 'admin';
      return html`<${ListItem}
        key=${item.slug}
        title=${html`<span class="font-mono">${item.slug}</span>`}
        subtitle=${item.description ?? t('accounts.role.perm_count', { count: item.permissions?.length ?? 0 })}
        right=${isSystemRole ? html`<span class="badge badge-muted text-2xs">${t('accounts.role.system')}</span>` : null}
        active=${isActive}
        onClick=${() => _selectItem(item)}
      />`;
    })}`, bodyEl);
  }

  renderItems();
  searchInput.addEventListener('input', () => {
    filter = searchInput.value;
    renderItems();
  });

  listEl.appendChild(bodyEl);
}


/**
 * @param {HTMLElement} el
 * @param {any} user
 */
function _renderUserDetail(el, user) {
  const effectivePerms = new Map();
  for (const roleName of (user.roles ?? [])) {
    const role = _roles.find(r => r.slug === roleName);
    for (const perm of (role?.permissions ?? [])) {
      if (!effectivePerms.has(perm)) effectivePerms.set(perm, roleName);
    }
  }

  el.innerHTML = `
    <div class="p-6 flex flex-col gap-5 min-h-0 md:h-full">
      <!-- User header -->
      <div class="flex flex-col gap-3 shrink-0 md:flex-row md:items-start md:gap-4">
        <div class="flex items-start gap-4 min-w-0">
          <span class="avatar xl" aria-hidden="true">${escapeHtml((user.username ?? '?')[0].toUpperCase())}</span>
          <div class="flex flex-col gap-1 min-w-0">
            <h2 class="text-lg font-semibold text-text truncate">${escapeHtml(user.username)}</h2>
            <p class="meta break-words">${escapeHtml(user.email ?? '')} · ${t('accounts.user.created_meta', { date: formatDate(user.created_at) || t('accounts.user.unknown_date') })}</p>
          </div>
        </div>
        <div class="flex items-center gap-1 flex-wrap md:ml-auto md:shrink-0">
          <button type="button" class="btn-ghost btn-sm js-edit-user">${t('accounts.action.edit')}</button>
          <button type="button" class="btn-ghost btn-sm js-reset-pw">${t('accounts.action.reset_password')}</button>
          <button type="button" class="btn-danger btn-sm js-delete-user">${t('common.delete')}</button>
        </div>
      </div>

      <!-- Identity card -->
      <div class="detail-card shrink-0">
        <div class="detail-card-head">${t('accounts.identity')}</div>
        <div class="kv"><span class="k">${t('accounts.user.username')}</span><span class="v">${escapeHtml(user.username)}</span></div>
        <div class="kv"><span class="k">${t('accounts.user.email')}</span><span class="v">${escapeHtml(user.email ?? '—')}</span></div>
        <div class="kv"><span class="k">${t('accounts.user.status')}</span><span class="v">
          <span class="${user.is_active ? 'badge badge-success' : 'badge badge-danger'}">${user.is_active ? t('accounts.user.active') : t('accounts.user.inactive')}</span>
        </span></div>
        <div class="kv"><span class="k">${t('accounts.user.roles')}</span><span class="v flex flex-wrap gap-1.5 justify-end">
          ${(user.roles ?? []).map(r => `<span class="badge badge-muted font-mono">${escapeHtml(r)}</span>`).join('') || `<span class="meta">${t('accounts.user.no_roles')}</span>`}
        </span></div>
        <div class="kv"><span class="k">${t('accounts.user.created')}</span><span class="v">${escapeHtml(formatDate(user.created_at) || '—')}</span></div>
      </div>

      <!-- Permissions | Activity — two columns, each scrolling independently -->
      <div class="grid grid-cols-1 md:grid-cols-2 gap-4 md:flex-1 md:min-h-0">
        <div class="detail-card flex flex-col md:min-h-0">
          <div class="detail-card-head shrink-0">${t('accounts.user.perms', { count: effectivePerms.size })}</div>
          <div class="md:flex-1 md:min-h-0 md:overflow-y-auto">
            ${[...effectivePerms.entries()].map(([perm, via]) => `
              <div class="flex items-center justify-between gap-3 px-3 py-2 border-b border-border-subtle last:border-0 text-sm">
                <span class="font-mono text-xs text-text">${escapeHtml(perm)}</span>
                <span class="meta shrink-0">${t('accounts.user.perm.via', { role: escapeHtml(via) })}</span>
              </div>
            `).join('') || `<p class="meta px-3 py-3">${t('accounts.user.no_perms')}</p>`}
          </div>
        </div>

        <div class="detail-card flex flex-col md:min-h-0">
          <div class="detail-card-head shrink-0">${t('accounts.user.activity')}</div>
          <div class="js-activity-feed md:flex-1 md:min-h-0 md:overflow-y-auto"></div>
        </div>
      </div>
    </div>
  `;

  el.querySelector('.js-edit-user')?.addEventListener('click', () => {
    _showUserModal(user, async () => {
      await _reload();
      _selected = _users.find(u => u.id === user.id) ?? null;
      _rerenderList();
      if (_detailEl && _selected) _renderUserDetail(_detailEl, _selected);
    });
  });

  // Sends the user a reset link rather than setting a password on their behalf,
  // so an admin never learns or chooses their credential.
  const _resetPwBtn = /** @type {HTMLButtonElement|null} */ (el.querySelector('.js-reset-pw'));
  _resetPwBtn?.addEventListener('click', async () => {
    if (!(await showConfirm(t('accounts.user.reset_pw.message', { username: user.username }),
      { title: t('accounts.action.reset_password') }))) return;
    try {
      await api.adminTriggerPasswordReset(user.id);
      showToast(t('accounts.user.reset_pw.sent'), { type: 'success' });
    } catch (e) {
      showApiError(e);
    }
  });

  const _delUserBtn = /** @type {HTMLButtonElement|null} */ (el.querySelector('.js-delete-user'));
  _delUserBtn?.addEventListener('click', async () => {
    if (!(await showConfirm(t('accounts.user.delete.message', { username: user.username }), { title: t('accounts.user.delete.title'), danger: true }))) return;
    if (_delUserBtn) _delUserBtn.disabled = true;
    try {
      await api.adminDeleteUser(user.id);
      showToast(t('accounts.user.deleted', { username: user.username }));
      await _reload();
      _selected = null;
      _rerenderList();
      if (_detailEl) _renderDetail(_detailEl);
    } catch (e) {
      showToast(e?.message ?? t('accounts.user.delete.failed'), { type: 'error' });
      if (_delUserBtn) _delUserBtn.disabled = false;
    }
  });

  const feedEl = /** @type {HTMLElement} */ (el.querySelector('.js-activity-feed'));
  /** @type {Array<{ at: string, kind: string, description: string }>} */
  let _activityEvents = [];
  let _activityLoading = true;
  let _activityError = /** @type {string|null} */ (null);

  function _renderFeed() {
    render(html`<${ActivityFeed}
      events=${_activityEvents}
      loading=${_activityLoading}
      error=${_activityError}
    />`, feedEl);
  }

  async function _loadActivity() {
    _activityLoading = true;
    _renderFeed();
    try {
      const data = await api.getUserActivity(user.id, { limit: 20 });
      _activityEvents = (data.events ?? []).map(/** @param {any} e */ e => ({
        at: e.created_at,
        kind: e.action,
        description: [e.target, e.details].filter(Boolean).join(' — ') || e.action,
      }));
      _activityError = null;
    } catch {
      _activityError = t('accounts.user.activity.failed');
    }
    _activityLoading = false;
    _renderFeed();
  }

  _renderFeed();
  _loadActivity();
}


/**
 * @param {HTMLElement} el
 * @param {any} role
 */
function _renderRoleDetail(el, role) {
  const usersWithRole = _users.filter(u => u.roles?.includes(role.slug));
  const isProtected = role.slug === 'user' || role.slug === 'admin';

  const directPerms = new Set(role.permissions ?? []);
  /** @type {Map<string, string>} */
  const inheritedPerms = new Map();
  let parentSlug = role.parent;
  while (parentSlug) {
    const parentRole = _roles.find(r => r.slug === parentSlug);
    if (!parentRole) break;
    for (const perm of (parentRole.permissions ?? [])) {
      if (!directPerms.has(perm) && !inheritedPerms.has(perm)) {
        inheritedPerms.set(perm, parentSlug);
      }
    }
    parentSlug = parentRole.parent;
  }

  const directChips = [...directPerms].map(p =>
    `<span class="badge badge-muted font-mono">${escapeHtml(p)}</span>`
  ).join('');

  const inheritedChips = [...inheritedPerms.entries()].map(([p, via]) =>
    `<span class="badge badge-muted font-mono opacity-60 italic" title="${t('accounts.role.inherited_from', { parent: escapeHtml(via) })}">${escapeHtml(p)}</span>`
  ).join('');

  const inheritedSection = inheritedChips ? `
    <div class="flex items-center gap-2 px-3 py-1.5 border-t border-border-subtle">
      <span class="text-2xs uppercase tracking-wide font-semibold text-text-faint">${t('accounts.role.inherited_label')}</span>
    </div>
    <div class="px-3 pb-3 flex flex-wrap gap-1.5">${inheritedChips}</div>
  ` : '';

  const usersChips = usersWithRole.map(u => `
    <button type="button" class="badge badge-muted gap-1.5 hover:bg-surface-3 transition-colors js-user-badge"
            data-user-id="${u.id}">
      <span class="avatar" style="width:16px;height:16px;font-size:8px" aria-hidden="true">${escapeHtml((u.username ?? '?')[0].toUpperCase())}</span>
      <span class="font-mono text-xs">${escapeHtml(u.username)}</span>
    </button>
  `).join('');

  el.innerHTML = `
    <div class="p-6 flex flex-col gap-5 min-h-0">
      <!-- Role header -->
      <div class="flex flex-col gap-3 md:flex-row md:items-start md:gap-4">
        <div class="flex flex-col gap-1 min-w-0">
          <div class="flex items-center gap-2 flex-wrap">
            <h2 class="text-lg font-semibold text-text font-mono">${escapeHtml(role.slug)}</h2>
            ${isProtected ? `<span class="badge badge-muted text-2xs">${t('accounts.role.system_label')}</span>` : ''}
          </div>
          <p class="meta">${t('accounts.role.perm_count', { count: role.permissions?.length ?? 0 })}${role.parent ? ` · ${t('accounts.role.inherits', { parent: escapeHtml(role.parent) })}` : ''}</p>
          ${role.description ? `<p class="text-sm text-text-muted mt-0.5">${escapeHtml(role.description)}</p>` : ''}
        </div>
        <div class="flex items-center gap-1 flex-wrap md:ml-auto md:shrink-0">
          <button type="button" class="btn-ghost btn-sm js-edit-role">${t('accounts.action.edit')}</button>
          ${!isProtected ? `<button type="button" class="btn-danger btn-sm js-delete-role">${t('common.delete')}</button>` : ''}
        </div>
      </div>

      <!-- Permissions card -->
      <div class="detail-card">
        <div class="detail-card-head">${t('accounts.role.perms', { count: directPerms.size + inheritedPerms.size })}</div>
        <div class="p-3 flex flex-wrap gap-1.5">
          ${directChips || `<span class="meta p-1">${t('accounts.role.no_perms')}</span>`}
        </div>
        ${inheritedSection}
      </div>

      <!-- Users with this role -->
      <div class="detail-card">
        <div class="detail-card-head">${t('accounts.role.users', { count: usersWithRole.length })}</div>
        <div class="p-3 flex flex-wrap gap-1.5">
          ${usersChips || `<p class="meta p-1">${t('accounts.role.no_users')}</p>`}
        </div>
      </div>
    </div>
  `;

  el.querySelector('.js-edit-role')?.addEventListener('click', () => {
    _showRoleModal(role, async () => {
      await _reload();
      const refreshed = _roles.find(r => r.slug === role.slug);
      _selected = refreshed ?? null;
      _rerenderList();
      if (_detailEl && _selected) _renderRoleDetail(_detailEl, _selected);
    });
  });

  const _delRoleBtn = /** @type {HTMLButtonElement|null} */ (el.querySelector('.js-delete-role'));
  _delRoleBtn?.addEventListener('click', async () => {
    if (!(await showConfirm(t('accounts.role.delete.message', { slug: role.slug }), { title: t('accounts.role.delete.title'), danger: true }))) return;
    if (_delRoleBtn) _delRoleBtn.disabled = true;
    try {
      await api.adminDeleteRole(role.slug);
      showToast(t('accounts.role.deleted', { slug: role.slug }));
      await _reload();
      _selected = null;
      _rerenderList();
      if (_detailEl) _renderDetail(_detailEl);
    } catch (e) {
      showToast(e?.message ?? t('accounts.role.delete.failed'), { type: 'error' });
      if (_delRoleBtn) _delRoleBtn.disabled = false;
    }
  });

  for (const btn of el.querySelectorAll('.js-user-badge')) {
    btn.addEventListener('click', () => {
      const userId = Number(/** @type {HTMLElement} */ (btn).dataset.userId);
      const user = _users.find(u => u.id === userId);
      if (!user) return;
      _activeTab = 'users';
      _selected = user;
      _updateHeaderActions();
      if (_listEl) _renderList(_listEl);
      if (_detailEl) _renderUserDetail(_detailEl, user);
    });
  }
}


/**
 * @param {any | null} user
 * @param {() => void} onSaved
 */
function _showUserModal(user, onSaved) {
  const isEdit = user != null;

  function UserModal({ onClose }) {
    const [username, setUsername] = useState(user?.username ?? '');
    const [email, setEmail]       = useState(user?.email ?? '');
    const [password, setPassword] = useState('');
    const [isActive, setIsActive] = useState(user?.is_active ?? true);
    const [roles, setRoles]       = useState(/** @type {string[]} */ (user?.roles ?? []));
    const [error, setError]       = useState('');
    const [saving, setSaving]     = useState(false);

    const toggleRole = (slug) =>
      setRoles(prev => prev.includes(slug) ? prev.filter(r => r !== slug) : [...prev, slug]);

    const save = async () => {
      if (!username.trim()) { setError(t('accounts.modal.user.error.username')); return; }
      if (!email.trim())    { setError(t('accounts.modal.user.error.email')); return; }
      if (!isEdit && !password) { setError(t('accounts.modal.user.error.password')); return; }
      if (!isEdit && password.length < 8) { setError(t('accounts.modal.user.error.password_short')); return; }
      setSaving(true); setError('');
      try {
        if (isEdit) {
          const patch = /** @type {Record<string, any>} */ ({});
          if (username.trim() !== user.username) patch.username = username.trim();
          if (email.trim() !== user.email)       patch.email    = email.trim();
          patch.is_active = isActive;
          if (Object.keys(patch).length) await api.adminUpdateUser(user.id, patch);
          const cur = user.roles ?? [];
          for (const r of roles) if (!cur.includes(r)) await api.adminGrantRole(user.id, r);
          for (const r of cur)   if (!roles.includes(r)) await api.adminRevokeRole(user.id, r);
          showToast(t('accounts.modal.user.updated'));
        } else {
          const created = await api.adminCreateUser({ username: username.trim(), email: email.trim(), password, roles });
          showToast(t('accounts.modal.user.created', { username: created.username }));
        }
        onClose();
        await onSaved();
      } catch (e) {
        showToast(e?.message ?? t('accounts.save.failed'), { type: 'error' });
        setSaving(false);
      }
    };

    return html`
      <${Modal} open=${true} onClose=${onClose} title=${isEdit ? t('accounts.modal.user.edit', { username: user.username }) : t('accounts.modal.user.add')}
        footer=${html`
          <button class="btn-ghost btn-sm" onClick=${onClose}>${t('common.cancel')}</button>
          <button class="btn-primary btn-sm" onClick=${save} disabled=${saving}>
            ${saving ? t('common.saving') : isEdit ? t('accounts.modal.save') : t('accounts.modal.user.create')}
          </button>
        `}
      >
        <div class="flex flex-col gap-4">
          <div class="flex flex-col gap-1.5">
            <label class="text-sm font-medium text-text" for="modal-username">${t('accounts.user.username')}</label>
            <input id="modal-username" type="text" class="input" value=${username}
              onInput=${(e) => setUsername(e.target.value)} autocomplete="off" />
          </div>
          <div class="flex flex-col gap-1.5">
            <label class="text-sm font-medium text-text" for="modal-email">${t('accounts.user.email')}</label>
            <input id="modal-email" type="email" class="input" value=${email}
              onInput=${(e) => setEmail(e.target.value)} />
          </div>
          ${!isEdit && html`
            <div class="flex flex-col gap-1.5">
              <label class="text-sm font-medium text-text" for="modal-password">${t('accounts.modal.user.password')}</label>
              <input id="modal-password" type="password" class="input" value=${password}
                onInput=${(e) => setPassword(e.target.value)} autocomplete="new-password" placeholder=${t('accounts.modal.user.password.placeholder')} />
            </div>
          `}
          ${isEdit && html`
            <div class="flex flex-col gap-1.5">
              <label class="text-sm font-medium text-text">${t('accounts.user.status')}</label>
              <label class="flex items-center gap-2 text-sm text-text cursor-pointer">
                <input type="checkbox" checked=${isActive} onChange=${(e) => setIsActive(e.target.checked)} />
                ${t('accounts.modal.user.active')}
              </label>
            </div>
          `}
          <div class="flex flex-col gap-1.5">
            <span class="text-sm font-medium text-text">${t('accounts.user.roles')}</span>
            <div class="flex flex-col gap-2">
              ${_roles.map(r => html`
                <label key=${r.slug} class="flex items-center gap-2 text-sm text-text cursor-pointer">
                  <input type="checkbox" checked=${roles.includes(r.slug)}
                    onChange=${() => toggleRole(r.slug)} />
                  ${r.slug}
                  ${r.description && html`<span class="text-xs text-text-muted">— ${r.description}</span>`}
                </label>
              `)}
            </div>
          </div>
          ${error && html`<p class="text-sm text-danger">${error}</p>`}
        </div>
      </${Modal}>
    `;
  }

  const unmount = mountIntoModalRoot(html`<${UserModal} onClose=${() => unmount()} />`);
}


/**
 * @param {any | null} role
 * @param {() => void} onSaved
 */
function _showRoleModal(role, onSaved) {
  const isEdit = role != null;

  function RoleModal({ onClose }) {
    const [slug, setSlug]         = useState(role?.slug ?? '');
    const [description, setDesc]  = useState(role?.description ?? '');
    const [perms, setPerms]       = useState(/** @type {string[]} */ (role?.permissions ?? []));
    const [error, setError]       = useState('');
    const [saving, setSaving]     = useState(false);

    const togglePerm = (perm) =>
      setPerms(prev => prev.includes(perm) ? prev.filter(p => p !== perm) : [...prev, perm]);

    const save = async () => {
      setSaving(true); setError('');
      try {
        const desc = description.trim() || null;
        if (isEdit) {
          await api.adminUpdateRole(role.slug, { description: desc ?? undefined, permissions: perms });
          showToast(t('accounts.modal.role.updated', { slug: role.slug }));
        } else {
          const s = slug.trim();
          if (!s) { setError(t('accounts.modal.role.error.slug')); setSaving(false); return; }
          await api.adminCreateRole({ slug: s, description: desc ?? undefined, permissions: perms });
          showToast(t('accounts.modal.role.created', { slug: s }));
        }
        onClose();
        await onSaved();
      } catch (e) {
        showToast(e?.message ?? t('accounts.save.failed'), { type: 'error' });
        setSaving(false);
      }
    };

    return html`
      <${Modal} open=${true} onClose=${onClose}
        title=${isEdit ? t('accounts.modal.role.edit', { slug: role.slug }) : t('accounts.modal.role.add')}
        footer=${html`
          <button class="btn-ghost btn-sm" onClick=${onClose}>${t('common.cancel')}</button>
          <button class="btn-primary btn-sm" onClick=${save} disabled=${saving}>
            ${saving ? t('common.saving') : isEdit ? t('accounts.modal.save') : t('accounts.modal.role.create')}
          </button>
        `}
      >
        <div class="flex flex-col gap-4">
          ${!isEdit && html`
            <div class="flex flex-col gap-1.5">
              <label class="text-sm font-medium text-text" for="modal-role-slug">${t('accounts.modal.role.slug')}</label>
              <input id="modal-role-slug" type="text" class="input font-mono"
                value=${slug} onInput=${(e) => setSlug(e.target.value)}
                placeholder=${t('accounts.modal.role.slug.placeholder')} autocomplete="off" />
            </div>
          `}
          <div class="flex flex-col gap-1.5">
            <label class="text-sm font-medium text-text" for="modal-role-desc">${t('accounts.modal.role.desc')}</label>
            <input id="modal-role-desc" type="text" class="input" value=${description}
              onInput=${(e) => setDesc(e.target.value)} placeholder=${t('accounts.modal.role.desc.placeholder')} />
          </div>
          <div class="flex flex-col gap-1.5">
            <span class="text-sm font-medium text-text">${t('accounts.modal.role.perms')}</span>
            <div class="flex flex-col gap-1.5">
              ${ALL_PERMISSIONS.map(perm => html`
                <label key=${perm} class="flex items-center gap-2 text-sm text-text cursor-pointer">
                  <input type="checkbox" checked=${perms.includes(perm)}
                    onChange=${() => togglePerm(perm)} />
                  <span class="font-mono text-xs">${perm}</span>
                </label>
              `)}
            </div>
          </div>
          ${error && html`<p class="text-sm text-danger">${error}</p>`}
        </div>
      </${Modal}>
    `;
  }

  const unmount = mountIntoModalRoot(html`<${RoleModal} onClose=${() => unmount()} />`);
}
