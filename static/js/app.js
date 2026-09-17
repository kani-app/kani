// @ts-check
// App entry point. Bootstraps permissions, SSE, nav, and the SPA router.

import { initPermissions, getState, setState, subscribe, hasPermission } from './session.js';
import { initTheme, syncServerThemes } from './theme.js';
import { initTileSize } from './tile-size.js';
import { connectSSE } from './sse.js';
import { initRouter, navigate, onNavigate, rememberIntendedDestination } from './router.js';
import { getBootId, logout, getFeatures, getSystemInfo, getChangelog, getCurrentUser } from './api.js';
import { iconSettings, iconLogout, iconWarning, iconBell, iconLibrary, iconSources, iconSearch, iconUpdates, iconDownloads, iconAccounts, iconBookOpen, iconCube, iconStats, iconLogs, iconRefresh, iconEllipsisHorizontal, iconArrowUp, iconChevronLeft } from './icons.js';
import { mountNotificationsPanel } from './components/notifications-panel.js';
import { mountAppHeader } from './components/app-header.js';
import { maybeShowUpdateBanner } from './components/update-banner.js';
import { maybeShowDegradedBanner } from './components/degraded-banner.js';
import { initTooltip } from './components/tooltip.js';
import { showAlert } from './components/modal.js';
import { showWhatsNew } from './components/whats-new.js';
import { showMoreSheet } from './components/more-sheet.js';
import { getLocal, setLocal, modKeyCombo } from './utils.js';
import { t } from './i18n.js';
import { listenForStateChanges } from './sync.js';
import { registerShortcuts, showCheatsheet } from './shortcuts.js';
import { openCommandPalette } from './components/command-palette.js';


(async () => {
  initTheme();
  initTileSize();

  if (['/login', '/register', '/setup'].includes(location.pathname)) {
    const appEl = document.getElementById('app');
    if (appEl) initRouter(appEl);
    _hideChrome();
    return;
  }


  await initPermissions();
  syncServerThemes().catch(() => { });
  getCurrentUser().then(user => setState('user', user)).catch(() => { });
  initTooltip();
  connectSSE();
  _mountConnectionBanner();
  listenForStateChanges((key, value) => setState(key, value, { broadcast: false }));
  _registerGlobalShortcuts();

  try {
    const { boot_id } = await getBootId();
    if (boot_id) setState('bootId', boot_id);
  } catch { }

  const navEl    = document.getElementById('nav');
  const tabNavEl = document.getElementById('bottom-nav');

  if (navEl)    _renderDesktopNav(navEl);
  if (tabNavEl) _renderBottomNav(tabNavEl);

  const appEl = document.getElementById('app');
  if (appEl) {
    const { notificationsMount } = mountAppHeader(appEl);
    // Async: show security banner if admin and TOTP not enabled on public instance.
    _maybeShowSecurityBanner(appEl);
    maybeShowUpdateBanner(appEl);
  maybeShowDegradedBanner(appEl);
    mountNotificationsPanel(notificationsMount);

    const pageContent = document.createElement('div');
    pageContent.id = 'page-content';
    pageContent.className = 'flex flex-col flex-1';
    appEl.appendChild(pageContent);

    initRouter(pageContent);
    _maybeRedirectFirstRun();
    _maybeShowWhatsNew(appEl);
  }

  window.addEventListener('kani:server-restart', () => {
    // Re-hydrate permissions immediately — the SSE reconnect means the server
    // is up, so this should succeed without a full reload.
    initPermissions();
    getCurrentUser().then(user => setState('user', user)).catch(() => { });
    _handleServerRestart();
  });

  _registerServiceWorker();
})();


async function _maybeRedirectFirstRun() {
  if (!hasPermission('admin:manage')) return;
  try {
    const info = await getSystemInfo();
    if (info?.first_run && location.pathname !== '/onboarding') {
      // Remember where they were going. Discarding it silently sends every first-run deep link to
      // the library instead — and a link carrying a query string (`/settings?section=diagnostics`)
      // lands somewhere that looks plausible, so the drop is invisible rather than merely annoying.
      rememberIntendedDestination(location.pathname + location.search);
      navigate('/onboarding');
    }
  } catch { }
}

/** @param {HTMLElement} appEl */
async function _maybeShowWhatsNew(appEl) {
  try {
    const info = await getSystemInfo();
    if (!info?.version || info?.first_run) return;
    const lastSeen = getLocal('kani_last_seen_version');
    if (lastSeen === info.version) return;
    setLocal('kani_last_seen_version', info.version);
    const changelog = await getChangelog().catch(() => null);
    if (!changelog?.html?.trim()) return;
    showWhatsNew(changelog.version ?? info.version, changelog.html);
  } catch { }
}

const SIDEBAR_KEY = 'kani_sidebar_collapsed';

/**
 * Whether the rail is collapsed, and if the reader has never said, a default
 * taken from the viewport.
 *
 * Below lg the sidebar costs a fifth of the screen — 200 of 1024, and more of a
 * 768 px tablet — on a layout that is already down to one column. Starting
 * collapsed there hands that width back. A stored preference always wins, at
 * every size: this only decides for someone who has not chosen yet.
 */
function _sidebarCollapsed() {
  const stored = getLocal(SIDEBAR_KEY);
  if (stored === '1') return true;
  if (stored === '0') return false;
  return window.innerWidth < 1024;
}

/**
 * Reflects the stored state onto the document and the sidebar's own controls.
 *
 * The width itself is one custom property on <html>; everything that offsets
 * against the sidebar reads it, so nothing here has to know about banners or
 * the bulk bar. What this does own is the part CSS cannot: the toggle's
 * accessible name and state, and the per-item tooltips that stand in for the
 * labels the rail drops.
 *
 * @param {HTMLElement} [el]
 */
function _applySidebarCollapsed(el) {
  const collapsed = _sidebarCollapsed();
  document.documentElement.dataset.sidebar = collapsed ? 'collapsed' : 'expanded';

  const sidebar = el ?? document.querySelector('.sidebar');
  const toggle = sidebar?.querySelector('#sidebar-toggle');
  if (toggle) {
    const label = collapsed ? t('nav.sidebar.expand') : t('nav.sidebar.collapse');
    toggle.setAttribute('aria-label', label);
    toggle.setAttribute('title', label);
    toggle.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
  }
  for (const a of sidebar?.querySelectorAll('.nav-item[data-label]') ?? []) {
    if (collapsed) a.setAttribute('title', a.getAttribute('data-label') ?? '');
    else a.removeAttribute('title');
  }
}

/** @param {boolean} collapsed */
function _setSidebarCollapsed(collapsed) {
  setLocal(SIDEBAR_KEY, collapsed ? '1' : '0');
  _applySidebarCollapsed();
}

/** @param {HTMLElement} el */
function _renderDesktopNav(el) {
  el.className = 'sidebar';

  el.innerHTML = `
    <div class="sidebar-brand">
      <span class="sidebar-mark" aria-hidden="true">K</span>
      <span class="sidebar-title">Kani</span>
      <button
        type="button"
        class="btn-icon sidebar-toggle"
        id="sidebar-toggle"
        aria-controls="sidebar-nav-links"
      ><span class="icon-sm">${iconChevronLeft}</span></button>
    </div>
    <nav class="sidebar-nav" id="sidebar-nav-links" aria-label="${t('nav.main.aria')}"></nav>
    <div class="sidebar-footer" id="sidebar-footer">
      <span class="avatar" id="sidebar-avatar" aria-hidden="true">U</span>
      <span class="sidebar-user-text">
        <span class="text-sm font-medium text-text truncate" id="sidebar-username">${t('nav.user_fallback')}</span>
        <span class="meta truncate" id="sidebar-role"></span>
      </span>
      <button class="btn-icon shrink-0" id="nav-logout" aria-label="${t('nav.sign_out')}" title="${t('nav.sign_out')}">${iconLogout}</button>
    </div>
  `;

  const toggleEl = /** @type {HTMLButtonElement|null} */ (el.querySelector('#sidebar-toggle'));
  toggleEl?.addEventListener('click', () => _setSidebarCollapsed(!_sidebarCollapsed()));
  _applySidebarCollapsed(el);

  // Crossing lg re-decides the default, but only while it is still a default —
  // once the reader has chosen, dragging a window must not undo the choice.
  let resizeTimer = 0;
  window.addEventListener('resize', () => {
    // getLocal returns '' for an absent key, never null.
    const chosen = getLocal(SIDEBAR_KEY);
    if (chosen === '1' || chosen === '0') return;
    clearTimeout(resizeTimer);
    resizeTimer = window.setTimeout(() => _applySidebarCollapsed(el), 120);
  });

  el.querySelector('#nav-logout')?.addEventListener('click', async () => {
    try { await logout(); } catch { }
    navigate('/login');
  });

  _rebuildSidebarLinks(el);
  _updateSidebarUser(el);
  _updateDesktopActive(el, location.pathname);

  onNavigate(path => {
    if (['/login', '/register', '/setup', '/onboarding'].includes(path)) { _hideChrome(); return; }
    _showChrome();
    _updateDesktopActive(el, path);
  });

  subscribe('permissions', () => {
    _rebuildSidebarLinks(el);
    _updateDesktopActive(el, location.pathname);
  });

  subscribe('user', () => _updateSidebarUser(el));
}

/** @param {HTMLElement} sidebar */
function _rebuildSidebarLinks(sidebar) {
  const nav = sidebar.querySelector('#sidebar-nav-links');
  if (!nav) return;
  nav.innerHTML = _buildNavLinks();
  _applySidebarCollapsed(sidebar);
}

/** @param {HTMLElement} sidebar */
function _updateSidebarUser(sidebar) {
  const user = getState('user');
  if (!user) return;
  const avatarEl = sidebar.querySelector('#sidebar-avatar');
  const nameEl   = sidebar.querySelector('#sidebar-username');
  const roleEl   = sidebar.querySelector('#sidebar-role');
  if (avatarEl) avatarEl.textContent = (user.username ?? 'U')[0].toUpperCase();
  if (nameEl)   nameEl.textContent   = user.username ?? t('nav.user_fallback');
  if (roleEl)   roleEl.textContent   = user.roles?.join(', ') ?? '';
}

/** @returns {string} */
function _buildNavLinks() {
  /** @type {{ href: string, label: string, icon: string, perm?: string, section?: string, matchPrefix?: string, matchPaths?: string[] }[]} */
  const defs = [
    { href: '/',          label: t('nav.library'),   icon: iconLibrary,   perm: 'library:view',  matchPaths: ['/', '/manga'] },
    { href: '/sources',   label: t('nav.sources'),   icon: iconSources,   perm: 'source:browse', matchPrefix: '/source' },
    { href: '/search',    label: t('nav.search'),    icon: iconSearch,    perm: 'source:browse' },
    { href: '/updates',   label: t('nav.updates'),   icon: iconUpdates,   perm: 'library:view' },
    { href: '/upgrades',  label: t('nav.upgrades'),  icon: iconArrowUp,   perm: 'library:view' },
    { href: '/downloads', label: t('nav.downloads'), icon: iconDownloads, perm: 'chapter:download' },
    { href: '/stats',     label: t('nav.statistics'), icon: iconStats,     perm: 'library:view' },
    { href: '/settings',  label: t('nav.settings'),  icon: iconSettings,  perm: 'settings:view',  section: 'Admin' },
    { href: '/accounts',  label: t('nav.accounts'),  icon: iconAccounts,  perm: 'user:manage' },
    { href: '/admin/logs', label: t('nav.logs'),     icon: iconLogs,      perm: 'admin:view_logs', matchPrefix: '/admin/logs' },
    { href: '/jobs',       label: t('nav.jobs'), icon: iconRefresh,   perm: 'admin:jobs',      matchPrefix: '/jobs',  section: 'Admin' },
  ];
  if (__KANI_DEV__) {
    defs.push({ href: '/admin/ui-showcase', label: 'UI Showcase', icon: iconCube, perm: 'admin:manage', matchPrefix: '/admin/ui-showcase' });
  }
  const visible = defs.filter(d => !d.perm || hasPermission(d.perm));
  let html = '';
  let lastSection = '';
  for (const d of visible) {
    if (d.section && d.section !== lastSection) {
      html += `<div class="nav-section">${d.section}</div>`;
      lastSection = d.section;
    }
    const prefix = d.matchPrefix ? ` data-match-prefix="${d.matchPrefix}"` : '';
    const paths = d.matchPaths ? ` data-match-paths="${d.matchPaths.join(',')}"` : '';
    html += `<a href="${d.href}" class="nav-item" data-href="${d.href}"${prefix}${paths} data-label="${d.label}">${d.icon}<span>${d.label}</span></a>`;
  }
  return html;
}

/**
 * @param {HTMLElement} el
 * @param {string} path
 */
function _updateDesktopActive(el, path) {
  for (const a of /** @type {NodeListOf<HTMLAnchorElement>} */ (el.querySelectorAll('.nav-item[data-href]'))) {
    const href = a.dataset.href ?? '';
    const matchPrefix = a.dataset.matchPrefix;
    const matchPaths = a.dataset.matchPaths?.split(',');
    let isActive;
    if (matchPaths) {
      isActive = matchPaths.some(p => p === '/' ? path === '/' : path.startsWith(p));
    } else if (matchPrefix) {
      isActive = path.startsWith(matchPrefix);
    } else {
      isActive = href === '/' ? path === '/' : path.startsWith(href);
    }
    a.classList.toggle('active', isActive);
    a.setAttribute('aria-current', isActive ? 'page' : 'false');
  }
}


/** @param {HTMLElement} el */
function _renderBottomNav(el) {
  el.className = 'md:hidden fixed bottom-0 inset-x-0 z-30 tab-bar bg-surface border-t border-border';

  // Four permanent slots; everything else lives behind "More". Without the sheet,
  // Downloads / Statistics / Accounts / Logs / Jobs have no route on a phone at all.
  const tabs = [
    { href: '/',         icon: iconBookOpen, label: t('nav.library'), perm: 'library:view' },
    { href: '/sources',  icon: iconCube,     label: t('nav.sources'), perm: 'source:browse', matchPrefix: '/source' },
    { href: '/search',   icon: iconSearch,   label: t('nav.search'),  perm: 'source:browse' },
    { href: '/updates',  icon: iconBell,     label: t('nav.updates'), perm: 'library:view' },
  ].filter(tab => !tab.perm || hasPermission(tab.perm));

  const moreItems = [
    { href: '/settings',   icon: iconSettings,  label: t('nav.settings'),   perm: 'settings:view' },
    { href: '/downloads',  icon: iconDownloads, label: t('nav.downloads'),  perm: 'chapter:download' },
    { href: '/stats',      icon: iconStats,     label: t('nav.statistics'), perm: 'library:view' },
    { href: '/upgrades',   icon: iconArrowUp,   label: t('nav.upgrades'),   perm: 'library:view' },
    { href: '/accounts',   icon: iconAccounts,  label: t('nav.accounts'),   perm: 'user:manage' },
    { href: '/admin/logs', icon: iconLogs,      label: t('nav.logs'),       perm: 'admin:view_logs' },
    { href: '/jobs',       icon: iconRefresh,   label: t('nav.jobs'),       perm: 'admin:jobs' },
  ];

  const inner = document.createElement('div');
  inner.className = 'flex h-full';
  inner.id = 'tab-bar-inner';

  const _tabClass = 'tab-link flex flex-col items-center justify-center flex-1 gap-0.5 text-xs text-text-muted transition-colors hover:text-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent';

  for (const tab of tabs) {
    const a = document.createElement('a');
    a.href = tab.href;
    a.className = _tabClass;
    a.setAttribute('aria-label', tab.label);
    if (tab.matchPrefix) a.dataset.matchPrefix = tab.matchPrefix;
    a.innerHTML = `<span class="icon-md">${tab.icon}</span><span>${tab.label}</span>`;
    inner.appendChild(a);
  }

  if (moreItems.some(i => !i.perm || hasPermission(i.perm))) {
    const moreBtn = document.createElement('button');
    moreBtn.type = 'button';
    moreBtn.className = _tabClass;
    moreBtn.setAttribute('aria-label', t('nav.more'));
    moreBtn.setAttribute('aria-haspopup', 'dialog');
    moreBtn.innerHTML = `<span class="icon-md">${iconEllipsisHorizontal}</span><span>${t('nav.more')}</span>`;
    // Highlight "More" only when the current route lives behind it.
    moreBtn.dataset.morePaths = moreItems.map(i => i.href).join(',');
    moreBtn.addEventListener('click', () => showMoreSheet(moreItems));
    inner.appendChild(moreBtn);
  }

  el.appendChild(inner);
  _updateTabActive(el, location.pathname);

  onNavigate(path => {
    if (['/login', '/register', '/setup', '/onboarding'].includes(path)) { el.style.display = 'none'; return; }
    el.style.display = '';
    _updateTabActive(el, path);
  });
}

/**
 * @param {HTMLElement} el
 * @param {string} path
 */
function _updateTabActive(el, path) {
  for (const a of /** @type {NodeListOf<HTMLElement>} */ (el.querySelectorAll('.tab-link'))) {
    let isActive;
    if (a.dataset.morePaths != null) {
      isActive = a.dataset.morePaths.split(',').some(p => p && (p === '/' ? path === '/' : path.startsWith(p)));
    } else {
      const href = a.getAttribute('href') ?? '';
      const matchPrefix = a.dataset.matchPrefix ?? href;
      isActive = !matchPrefix ? false : (href === '/' ? path === '/' : path.startsWith(matchPrefix));
    }
    a.classList.toggle('!text-accent', isActive);
    a.setAttribute('aria-current', isActive ? 'page' : 'false');
  }
}


function _hideChrome() {
  const nav    = document.getElementById('nav');
  const tabNav = document.getElementById('bottom-nav');
  const hdr    = document.querySelector('.app-header');
  const bar    = document.querySelector('.page-action-bar');
  if (nav)    nav.style.display = 'none';
  if (tabNav) tabNav.style.display = 'none';
  if (hdr)    /** @type {HTMLElement} */ (hdr).style.display = 'none';
  if (bar)    /** @type {HTMLElement} */ (bar).style.display = 'none';
  const app = document.getElementById('app');
  if (app) app.classList.remove('shell-main');
}

function _showChrome() {
  const nav    = document.getElementById('nav');
  const tabNav = document.getElementById('bottom-nav');
  const hdr    = document.querySelector('.app-header');
  const bar    = document.querySelector('.page-action-bar');
  if (nav)    nav.style.display = '';
  if (tabNav) tabNav.style.display = '';
  if (hdr)    /** @type {HTMLElement} */ (hdr).style.display = '';
  // Cleared, not shown: the bar's own `hidden` decides, since a page with no
  // actions must not leave an empty strip under the header.
  if (bar)    /** @type {HTMLElement} */ (bar).style.display = '';
  const app = document.getElementById('app');
  if (app) app.classList.add('shell-main');
}


function _handleServerRestart() {
  if (document.getElementById('restart-banner')) return;

  const banner = document.createElement('div');
  banner.id = 'restart-banner';
  banner.className = 'fixed top-0 right-0 md:left-sidebar z-20 flex items-center gap-3 px-6 py-3 bg-warn/10 border-b border-warn/30 text-sm text-warn';
  banner.innerHTML = `
    <span aria-hidden="true" class="shrink-0 icon-sm">${iconWarning}</span>
    <span class="flex-1">${t('restart.banner.message')}</span>
    <button id="restart-reload-btn" class="btn-primary btn-sm ml-auto">${t('restart.banner.reload')}</button>
  `;
  document.body.prepend(banner);

  document.getElementById('restart-reload-btn')?.addEventListener('click', async function () {
    this.disabled = true;
    this.textContent = t('restart.waiting');
    // Poll /ready until the server confirms it is fully initialised, then
    // reload. The SSE reconnect already proves the server is up, but the DB
    // and other subsystems may still be settling.
    for (let i = 0; i < 20; i++) {
      try {
        const r = await fetch('/ready');
        if (r.ok) { location.reload(); return; }
      } catch { }
      await new Promise(res => setTimeout(res, 500));
    }
    // Fell through — reload anyway after 10 s.
    location.reload();
  });
}


function _mountConnectionBanner() {
  /** @type {HTMLElement | null} */
  let _banner = null;
  /** @type {ReturnType<typeof setTimeout> | null} */
  let _graceTimer = null;
  const GRACE_MS = 3000;

  window.addEventListener('kani:sse-disconnected', () => {
    if (_graceTimer || _banner) return;
    _graceTimer = setTimeout(() => {
      _graceTimer = null;
      _banner = document.createElement('div');
      _banner.id = 'sse-disconnect-banner';
      _banner.className = 'fixed top-0 right-0 md:left-sidebar z-20 flex items-center gap-3 px-6 py-2.5 bg-surface-3 border-b border-border text-sm text-text-muted';
      _banner.innerHTML = `<span class="flex-1">${t('sse.disconnected')}</span>`;
      document.body.prepend(_banner);
    }, GRACE_MS);
  });

  window.addEventListener('kani:sse-connected', () => {
    if (_graceTimer) { clearTimeout(_graceTimer); _graceTimer = null; }
    if (_banner) { _banner.remove(); _banner = null; }
  });
}


function _registerServiceWorker() {
  if (!('serviceWorker' in navigator)) return;

  navigator.serviceWorker.register('/sw.js', { scope: '/' })
    .then(reg => {
      reg.addEventListener('updatefound', () => {
        const incoming = reg.installing;
        if (!incoming) return;
        incoming.addEventListener('statechange', () => {
          if (incoming.state === 'installed' && navigator.serviceWorker.controller) {
            _showSwUpdateBanner(incoming);
          }
        });
      });
    })
    .catch(err => console.warn('SW registration failed:', err));
}

/**
 * If public_instance mode is active and the current user is admin but hasn't enabled TOTP,
 * show a persistent security notice banner below the app header.
 * @param {HTMLElement} appEl
 */
async function _maybeShowSecurityBanner(appEl) {
  try {
    const features = await getFeatures();
    if (!features?.public_instance) return;
    if (features?.totp_enabled) return;
    if (!hasPermission('admin:manage')) return;

    if (document.getElementById('security-banner')) return;
    const banner = document.createElement('div');
    banner.id = 'security-banner';
    banner.className = 'flex items-center gap-3 px-4 py-2 border-b border-warn/30 bg-warn/10 text-sm';
    banner.innerHTML = `
      <span class="text-warn font-semibold shrink-0">${t('security_banner.label')}</span>
      <span class="text-text flex-1">${t('security_banner.message')}</span>
      <a href="/settings?section=security" class="text-accent underline shrink-0">${t('security_banner.action')}</a>
    `;
    // Insert after the app-header element
    const header = appEl.querySelector('header');
    if (header?.nextSibling) {
      appEl.insertBefore(banner, header.nextSibling);
    } else {
      appEl.appendChild(banner);
    }
  } catch { }
}

/** @param {ServiceWorker} incoming */
function _registerGlobalShortcuts() {
  registerShortcuts('global', [
    {
      key: '/',
      description: 'Focus search',
      handler: () => {
        const search = /** @type {HTMLInputElement|null} */ (document.querySelector('.js-search'));
        if (search) { search.focus(); search.select(); }
        else navigate('/search');
      },
    },
    {
      key: ['h', 'H'],
      description: 'Go to Library',
      handler: () => navigate('/'),
    },
    {
      key: ['u', 'U'],
      description: 'Go to Updates',
      handler: () => navigate('/updates'),
    },
    {
      key: ['s', 'S'],
      description: 'Go to Sources',
      handler: () => navigate('/sources'),
    },
    {
      key: ['[', ']'],
      description: 'Collapse or expand the sidebar',
      handler: () => _setSidebarCollapsed(!_sidebarCollapsed()),
    },
    {
      key: '?',
      description: 'Show keyboard shortcuts',
      handler: () => showCheatsheet(),
    },
    {
      key: modKeyCombo('K'),
      description: 'Command palette',
      handler: () => {},
    },
  ]);

  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      const tag = /** @type {HTMLElement} */ (e.target)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
      if (/** @type {HTMLElement} */ (e.target)?.isContentEditable) return;
      e.preventDefault();
      openCommandPalette();
    }
  });
}

function _showSwUpdateBanner(incoming) {
  if (document.getElementById('sw-update-banner')) return;

  const banner = document.createElement('div');
  banner.id = 'sw-update-banner';
  banner.className = 'fixed top-0 right-0 md:left-sidebar z-20 flex items-center gap-3 px-6 py-3 bg-surface-3 border-b border-border text-sm text-text';
  banner.innerHTML = `
    <span class="flex-1">${t('sw_update.message')}</span>
    <button class="btn-primary btn-sm ml-auto" id="sw-update-reload">${t('sw_update.action')}</button>
  `;
  document.body.prepend(banner);

  document.getElementById('sw-update-reload')?.addEventListener('click', () => {
    incoming.postMessage({ type: 'SKIP_WAITING' });
    navigator.serviceWorker.addEventListener('controllerchange', () => location.reload(), { once: true });
  });
}
