// @ts-check

import { navigate } from '../router.js';
import { iconChevronLeft } from '../icons.js';
import { t } from '../i18n.js';

/** @typedef {{ label: string, href?: string }} Crumb */
/** @typedef {{ crumbs?: Crumb[], actions?: HTMLElement | HTMLElement[] | null }} HeaderState */

/** @type {HTMLElement | null} */
let _headerEl = null;
/** @type {HTMLElement | null} */
let _breadcrumbSlot = null;
/** @type {HTMLElement | null} */
let _actionsSlot = null;
/** @type {HTMLElement | null} */
let _actionBarEl = null;
/** @type {HeaderState} */
let _state = { crumbs: [], actions: null };

/**
 * Below this width page actions move out of the bar into their own toolbar
 * beneath it. The bar cannot hold a row of pills beside the crumb, and hiding
 * them behind a glyph obscured them instead.
 */
const ACTION_BAR_WIDTH = 768;

/** Below this width the trail collapses to one back link plus the current page. */
const CRUMB_COLLAPSE_WIDTH = 480;

/**
 * Mount the global app header into `container`.
 * Call once from app.js after rendering the sidebar.
 * Returns a reference to the #header-notifications div so app.js can mount the panel.
 *
 * @param {HTMLElement} container
 * @returns {{ notificationsMount: HTMLElement, destroy: () => void }}
 */
let _resizeBound = false;

export function mountAppHeader(container) {
  if (!_resizeBound) {
    let raf = 0;
    window.addEventListener('resize', () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => _applyState());
    });
    _resizeBound = true;
  }
  _headerEl = document.createElement('header');
  _headerEl.className = 'app-header';
  _headerEl.setAttribute('aria-label', t('app_header.aria'));

  _breadcrumbSlot = document.createElement('nav');
  _breadcrumbSlot.className = 'breadcrumb';
  _breadcrumbSlot.setAttribute('aria-label', t('app_header.breadcrumb_aria'));

  _actionsSlot = document.createElement('div');
  _actionsSlot.className = 'header-actions';

  const notificationsMount = document.createElement('div');
  notificationsMount.id = 'header-notifications';

  _headerEl.appendChild(_breadcrumbSlot);
  _headerEl.appendChild(_actionsSlot);
  _headerEl.appendChild(notificationsMount);

  _actionBarEl = document.createElement('div');
  _actionBarEl.className = 'page-action-bar';
  _actionBarEl.setAttribute('aria-label', t('app_header.actions_aria'));
  _actionBarEl.hidden = true;

  container.appendChild(_headerEl);
  container.appendChild(_actionBarEl);
  _applyState();

  return {
    notificationsMount,
    destroy() {
      _headerEl?.remove();
      _actionBarEl?.remove();
      _headerEl = null;
      _breadcrumbSlot = null;
      _actionsSlot = null;
      _actionBarEl = null;
      _state = { crumbs: [], actions: null };
    },
  };
}

/**
 * Called by each page's init() to set the header content.
 *
 * @param {HeaderState} state
 */
export function setPageHeader(state) {
  _state = state;
  _applyState();
}

/**
 * Called by each page's destroy() to clear page-specific content.
 */
export function clearPageHeader() {
  _state = { crumbs: [], actions: null };
  _applyState();
}

function _applyState() {
  if (!_breadcrumbSlot || !_actionsSlot || !_actionBarEl) return;
  _renderCrumbs();
  _placeActions();
}

/**
 * @param {Crumb} crumb
 * @returns {HTMLAnchorElement}
 */
function _crumbLink(crumb) {
  const a = document.createElement('a');
  a.href = /** @type {string} */ (crumb.href);
  a.textContent = crumb.label;
  a.addEventListener('click', (e) => {
    e.preventDefault();
    navigate(/** @type {string} */ (crumb.href));
  });
  return a;
}

function _renderCrumbs() {
  if (!_breadcrumbSlot) return;
  _breadcrumbSlot.innerHTML = '';
  const crumbs = _state.crumbs ?? [];
  if (crumbs.length === 0) return;

  const current = crumbs[crumbs.length - 1];
  const parent = [...crumbs.slice(0, -1)].reverse().find((c) => c.href);

  // A phone has room for one ancestor at most, and dropping every ancestor left
  // the app with no way back at all. Keep the nearest as an explicit control.
  if (window.innerWidth < CRUMB_COLLAPSE_WIDTH && parent) {
    const back = _crumbLink(parent);
    back.className = 'breadcrumb__back';
    back.textContent = '';
    back.setAttribute('aria-label', t('app_header.back', { label: parent.label }));

    const chevron = document.createElement('span');
    chevron.className = 'icon-sm';
    chevron.setAttribute('aria-hidden', 'true');
    chevron.innerHTML = iconChevronLeft;

    const label = document.createElement('span');
    label.className = 'breadcrumb__back-label';
    label.textContent = parent.label;

    back.append(chevron, label);
    _breadcrumbSlot.appendChild(back);
    _breadcrumbSlot.appendChild(_currentCrumb(current));
    return;
  }

  crumbs.slice(0, -1).forEach((crumb) => {
    if (crumb.href) {
      _breadcrumbSlot?.appendChild(_crumbLink(crumb));
    } else {
      const span = document.createElement('span');
      span.textContent = crumb.label;
      _breadcrumbSlot?.appendChild(span);
    }
    _breadcrumbSlot?.appendChild(_separator());
  });

  _breadcrumbSlot.appendChild(_currentCrumb(current));
}

/** @returns {HTMLSpanElement} */
function _separator() {
  const sep = document.createElement('span');
  sep.className = 'sep';
  sep.setAttribute('aria-hidden', 'true');
  sep.textContent = '/';
  return sep;
}

/**
 * @param {Crumb} crumb
 * @returns {HTMLSpanElement}
 */
function _currentCrumb(crumb) {
  const span = document.createElement('span');
  span.className = 'cur';
  span.setAttribute('aria-current', 'page');
  span.textContent = crumb.label;
  return span;
}

/**
 * Page actions sit in the bar on a desktop and in their own toolbar below it on
 * a phone. The nodes are moved rather than cloned, so their listeners survive.
 */
function _placeActions() {
  if (!_actionsSlot || !_actionBarEl) return;

  _actionsSlot.innerHTML = '';
  _actionBarEl.innerHTML = '';

  const actions = _state.actions;
  const actionEls = actions
    ? (Array.isArray(actions) ? actions : [actions])
    : [];

  const inBar = window.innerWidth < ACTION_BAR_WIDTH;
  const host = inBar ? _actionBarEl : _actionsSlot;
  for (const el of actionEls) host.appendChild(el);

  _actionBarEl.hidden = !inBar || actionEls.length === 0;
}
