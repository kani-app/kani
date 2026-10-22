// @ts-check
// History API (pushState) SPA router.
// Pages are lazy-imported modules that export init(container, params) and destroy(container).

import { announce } from './utils.js';
import { t } from './i18n.js';

/**
 * @typedef {{ init: (container: HTMLElement, params: Record<string, string>) => void,
 *             destroy?: (container: HTMLElement) => void }} PageModule
 */

/** @type {Array<{ re: RegExp, keys: string[], load: () => Promise<PageModule> }>} */
const _routes = [
  { path: '/login',                     load: () => import('./pages/login.js') },
  { path: '/register',                  load: () => import('./pages/register.js') },
  { path: '/setup',                     load: () => import('./pages/setup.js') },
  { path: '/forgot-password',           load: () => import('./pages/forgot-password.js') },
  { path: '/reset-password',            load: () => import('./pages/reset-password.js') },
  { path: '/verify-email',              load: () => import('./pages/verify-email.js') },
  { path: '/sources',                   load: () => import('./pages/sources.js') },
  { path: '/source/:id/manga/:manga_id',load: () => import('./pages/manga-details.js') },
  { path: '/source/:id',                load: () => import('./pages/source-details.js') },
  { path: '/manga/:db_id',              load: () => import('./pages/manga-details.js') },
  { path: '/reader/:id',                load: () => import('./pages/reader.js') },
  { path: '/search',                    load: () => import('./pages/global-search.js') },
  { path: '/downloads',                 load: () => import('./pages/downloads.js') },
  { path: '/settings',                  load: () => import('./pages/settings/index.js') },
  { path: '/accounts',                  load: () => import('./pages/accounts.js') },
  { path: '/updates',                   load: () => import('./pages/recent-updates.js') },
  { path: '/upgrades',                  load: () => import('./pages/upgrades.js') },
  { path: '/stats',                     load: () => import('./pages/stats.js') },
  { path: '/admin/logs',                load: () => import('./pages/admin/logs.js') },
  { path: '/jobs',                      load: () => import('./pages/admin/jobs.js') },
  { path: '/onboarding',               load: () => import('./pages/onboarding.js') },
  { path: '/',                          load: () => import('./pages/library.js') },
  // Component gallery: development only. The bundle defines __KANI_DEV__ as
  // false, so neither the route nor its chunk reaches a release build.
  ...(__KANI_DEV__
    ? [{ path: '/admin/ui-showcase', load: () => import('./pages/admin/ui-showcase.js') }]
    : []),
].map(({ path, load }) => ({ re: _pathToRegex(path), keys: _extractKeys(path), load }));

/** @type {HTMLElement | null} */
let _container = null;
/** @type {PageModule | null} */
let _activePage = null;
/** @type {Set<Function>} */
const _navCallbacks = new Set();
/** @type {Record<string, string>} */
let _currentParams = {};
/** @type {(() => Promise<boolean>) | null} — resolve false to cancel navigation */
let _beforeNavigate = null;
let _isInitialRoute = true;
/**
 * How long a page gets to finish loading before the router shows its skeleton.
 * Matches `deferredSkeleton`'s default so a cached page never flashes one.
 */
const SKELETON_GRACE_MS = 150;
/** @param {number} ms */
const _sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
/**
 * Bumped by every navigation. A route that started earlier compares its own
 * value after each await and bails if a newer navigation has superseded it —
 * otherwise a slow page's `await import()` resolves *after* a redirect and
 * renders itself over the page the redirect chose, leaving the URL and the
 * content disagreeing.
 */
let _navGeneration = 0;

/**
 * Registers a guard called before every programmatic navigation.
 * Return (or resolve) false to abort. Only one guard is active at a time.
 * @param {() => boolean | Promise<boolean>} fn
 */
export function setBeforeNavigate(fn) { _beforeNavigate = fn; }
export function clearBeforeNavigate() { _beforeNavigate = null; }

/**
 * Initialises the router, performs the initial route match, and sets up
 * popstate + link-interception listeners.
 * @param {HTMLElement} container
 */
export function initRouter(container) {
  _container = container;
  window.addEventListener('popstate', () => _route(location.pathname + location.search, true));
  document.addEventListener('click', _interceptLink);
  _route(location.pathname);
}

/**
 * Navigates to `path`, updating the browser URL.
 * @param {string} path
 * @param {{ replace?: boolean }} [opts]
 * @returns {Promise<void>}
 */
export async function navigate(path, opts = {}) {
  if (_beforeNavigate && !(await _beforeNavigate())) return;
  sessionStorage.setItem(`scroll:${location.pathname + location.search}`, String(_container?.scrollTop ?? 0));
  if (opts.replace) {
    history.replaceState(null, '', path);
  } else {
    history.pushState(null, '', path);
  }
  _route(path);
}

const INTENDED_KEY = 'kani-intended-destination';

/**
 * Park the destination a redirect is about to discard, so the flow that
 * interrupted the user can send them back to it.
 * @param {string} path pathname + search
 */
export function rememberIntendedDestination(path) {
  try {
    if (path && path !== '/') sessionStorage.setItem(INTENDED_KEY, path);
  } catch {
    /* storage unavailable — the redirect still works, it just forgets */
  }
}

/**
 * Take the parked destination, clearing it so a later unrelated visit to the
 * interrupting page does not bounce somewhere unexpected.
 * @returns {string | null}
 */
export function consumeIntendedDestination() {
  try {
    const v = sessionStorage.getItem(INTENDED_KEY);
    sessionStorage.removeItem(INTENDED_KEY);
    return v;
  } catch {
    return null;
  }
}

/** Scrolls the page content container back to the top (instant). */
export function scrollPageTop() {
  _container?.scrollTo({ top: 0, behavior: 'instant' });
}

/**
 * Registers a callback that fires on every navigation (used for nav highlight).
 * Returns an unregister function.
 * @param {(path: string) => void} callback
 * @returns {() => void}
 */
export function onNavigate(callback) {
  _navCallbacks.add(callback);
  return () => _navCallbacks.delete(callback);
}


/**
 * @param {string} path
 * @param {boolean} [fromPopstate]
 */
async function _route(path, fromPopstate = false) {
  if (!_container) return;
  const generation = ++_navGeneration;

  // Strip query string before matching — pages read location.search directly
  const pathname = path.split('?')[0];

  // Find matching route
  let matched = null;
  let params = {};
  for (const route of _routes) {
    const m = pathname.match(route.re);
    if (m) {
      for (let i = 0; i < route.keys.length; i++) {
        params[route.keys[i]] = decodeURIComponent(m[i + 1] ?? '');
      }
      matched = route;
      break;
    }
  }

  // Tear down current page — keep DOM visible while the new module loads to
  // avoid a blank-content flash during the async import.
  if (_activePage?.destroy) {
    try { _activePage.destroy(_container); } catch (e) { console.error('destroy error:', e); }
  }
  _activePage = null;
  _currentParams = params;

  // A page opts into the fixed-viewport layout by adding `page-fixed` to the shared container;
  // the router takes it back off on the way out. Left on, it applies `overflow: hidden` to the
  // next page too — statistics lost 615 px with no way to scroll to it after any visit to a manga.
  _container.classList.remove('page-fixed');

  if (!matched) {
    _container.innerHTML = `<div style="padding:2rem;text-align:center;color:var(--color-text-muted)">${t('router.not_found')}</div>`;
    document.title = t('router.not_found.title');
  } else {
    try {
      const mod = await matched.load();
      if (generation !== _navGeneration) return;

      // A view transition suppresses painting until its callback settles, so
      // awaiting all of `init()` holds the old page on screen for the whole
      // fetch. Start it, wait a grace period, then let the skeleton show.
      /** @type {Promise<void> | null} */
      let pending = null;
      const swap = async () => {
        if (generation !== _navGeneration) return;
        // Pages paint their own skeleton synchronously before their first
        // await, so clearing here is enough — the skeleton is in the DOM by the
        // time the grace period is up.
        /** @type {HTMLElement} */ (_container).innerHTML = '';
        _activePage = mod;
        pending = Promise.resolve(mod.init(_container, params));
        await Promise.race([pending, _sleep(SKELETON_GRACE_MS)]);
      };
      const vt = /** @type {any} */ (document).startViewTransition;
      if (typeof vt === 'function'
          && !_isInitialRoute
          && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
        await vt.call(document, swap).updateCallbackDone;
      } else {
        await swap();
      }
      // Scroll restoration below needs real content, not the skeleton, so the
      // navigation still resolves only once the page has loaded.
      if (pending) await pending;
    } catch (e) {
      console.error('Page init error:', e);
      _container.innerHTML = `<div style="padding:2rem;color:var(--color-danger)">${t('router.load_failed')}</div>`;
    }
  }

  if (fromPopstate) {
    const saved = sessionStorage.getItem(`scroll:${location.pathname + location.search}`);
    _container.scrollTo({ top: saved ? parseInt(saved, 10) : 0, behavior: 'instant' });
  } else {
    _container.scrollTo({ top: 0, behavior: 'instant' });
  }

  if (!_isInitialRoute) {
    announce(document.title);
    _moveFocusToMain();
  }
  _isInitialRoute = false;

  for (const cb of _navCallbacks) {
    try { cb(pathname); } catch {}
  }
}

/** Moves keyboard/screen-reader focus to the new page's heading (or the
 * container itself if it has none) after a client-side navigation. */
function _moveFocusToMain() {
  if (!_container) return;
  const target = /** @type {HTMLElement} */ (_container.querySelector('h1') ?? _container);
  if (!target.hasAttribute('tabindex')) target.setAttribute('tabindex', '-1');
  target.focus({ preventScroll: true });
}

/** @param {MouseEvent} e */
function _interceptLink(e) {
  if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || e.button !== 0) return;
  if (e.defaultPrevented) return;

  const anchor = /** @type {HTMLElement} */ (e.target)?.closest('a');
  if (!anchor) return;

  const href = anchor.getAttribute('href');
  if (!href) return;

  // Only intercept relative paths that aren't API or external URLs
  if (href.startsWith('/rest/') || href.startsWith('http') || href.startsWith('//')) return;
  if (!href.startsWith('/')) return;

  e.preventDefault();
  navigate(href);
}


/** Converts '/source/:id/manga/:manga_id' to a RegExp. */
function _pathToRegex(path) {
  const escaped = path
    .replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
    .replace(/:([a-zA-Z_][a-zA-Z0-9_]*)/g, '([^/]+)');
  return new RegExp(`^${escaped}/?$`);
}

/** Extracts param names from '/source/:id/manga/:manga_id' → ['id', 'manga_id']. */
function _extractKeys(path) {
  return [...path.matchAll(/:([a-zA-Z_][a-zA-Z0-9_]*)/g)].map(m => m[1]);
}
