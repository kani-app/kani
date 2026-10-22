// @ts-check
import { h, render } from 'preact';
import { useState, useEffect, useRef, useCallback } from 'preact/hooks';
import { t } from '../i18n.js';
import * as api from '../api.js';
import { navigate } from '../router.js';
import { showCheatsheet } from '../shortcuts.js';
import { modKeyLabel } from '../utils.js';
import { SECTION_SEARCH_PREFIXES, buildSettingsSearchIndex } from '../settings-search-index.js';
import { isPhoneLayout, PAGINATION_SETTINGS_PREFIX } from '../pagination-mode.js';

const CATEGORY_LABELS = {
  nav: () => t('palette.category.nav'),
  settings: () => t('palette.category.settings'),
  library: () => t('palette.category.library'),
};

const NAV_ITEMS = [
  { id: 'nav-library',        get label() { return t('palette.nav.library.label'); },        get description() { return t('palette.nav.library.desc'); },        category: 'nav', path: '/' },
  { id: 'nav-updates',        get label() { return t('palette.nav.updates.label'); },        get description() { return t('palette.nav.updates.desc'); },        category: 'nav', path: '/updates' },
  { id: 'nav-sources',        get label() { return t('palette.nav.sources.label'); },        get description() { return t('palette.nav.sources.desc'); },        category: 'nav', path: '/sources' },
  { id: 'nav-downloads',      get label() { return t('palette.nav.downloads.label'); },      get description() { return t('palette.nav.downloads.desc'); },      category: 'nav', path: '/downloads' },
  { id: 'nav-jobs',           get label() { return t('palette.nav.jobs.label'); },           get description() { return t('palette.nav.jobs.desc'); },           category: 'nav', path: '/jobs' },
  { id: 'nav-settings',       get label() { return t('palette.nav.settings.label'); },       get description() { return t('palette.nav.settings.desc'); },       category: 'nav', path: '/settings' },
];

const SETTINGS_ITEMS = [
  { id: 's-general',          get label() { return t('palette.settings.general.label'); },          get description() { return t('palette.settings.general.desc'); },          category: 'settings', path: '/settings?section=general' },
  { id: 's-library',          get label() { return t('palette.settings.library.label'); },          get description() { return t('palette.settings.library.desc'); },          category: 'settings', path: '/settings?section=library' },
  { id: 's-collections',      get label() { return t('palette.settings.collections.label'); },      get description() { return t('palette.settings.collections.desc'); },      category: 'settings', path: '/settings?section=collections' },
  { id: 's-manga-management', get label() { return t('palette.settings.manga_management.label'); }, get description() { return t('palette.settings.manga_management.desc'); }, category: 'settings', path: '/settings?section=manga-management' },
  { id: 's-trash',            get label() { return t('palette.settings.trash.label'); },            get description() { return t('palette.settings.trash.desc'); },            category: 'settings', path: '/settings?section=trash' },
  { id: 's-downloads',        get label() { return t('palette.settings.downloads.label'); },        get description() { return t('palette.settings.downloads.desc'); },        category: 'settings', path: '/settings?section=downloads' },
  { id: 's-offline',          get label() { return t('palette.settings.offline.label'); },          get description() { return t('palette.settings.offline.desc'); },          category: 'settings', path: '/settings?section=offline' },
  { id: 's-scan',             get label() { return t('palette.settings.scan.label'); },             get description() { return t('palette.settings.scan.desc'); },             category: 'settings', path: '/settings?section=scan' },
  { id: 's-trackers',         get label() { return t('palette.settings.trackers.label'); },         get description() { return t('palette.settings.trackers.desc'); },         category: 'settings', path: '/settings?section=trackers' },
  { id: 's-email',            get label() { return t('palette.settings.email.label'); },            get description() { return t('palette.settings.email.desc'); },            category: 'settings', path: '/settings?section=email' },
  { id: 's-webhooks',         get label() { return t('palette.settings.webhooks.label'); },         get description() { return t('palette.settings.webhooks.desc'); },         category: 'settings', path: '/settings?section=webhooks' },
  { id: 's-advanced',         get label() { return t('palette.settings.advanced.label'); },         get description() { return t('palette.settings.advanced.desc'); },         category: 'settings', path: '/settings?section=advanced' },
  { id: 's-storage',          get label() { return t('palette.settings.storage.label'); },          get description() { return t('palette.settings.storage.desc'); },          category: 'settings', path: '/settings?section=storage' },
  { id: 's-server',           get label() { return t('palette.settings.server.label'); },           get description() { return t('palette.settings.server.desc'); },           category: 'settings', path: '/settings?section=server' },
  { id: 's-account',          get label() { return t('palette.settings.account.label'); },          get description() { return t('palette.settings.account.desc'); },          category: 'settings', path: '/settings?section=account' },
  { id: 's-security',         get label() { return t('palette.settings.security.label'); },         get description() { return t('palette.settings.security.desc'); },         category: 'settings', path: '/settings?section=security' },
];

/**
 * @param {string} text
 * @param {string} query
 */
function _matches(text, query) {
  return text.toLowerCase().includes(query.toLowerCase());
}

/** @type {Map<string, string>} section id → its own palette label */
const _SECTION_LABELS = new Map(
  SETTINGS_ITEMS.map(item => [item.id.replace(/^s-/, ''), item.label])
);

/** @type {Map<boolean, ReturnType<typeof buildSettingsSearchIndex>>} */
const _settingsSearchIndexes = new Map();

function _settingsSearchIndex() {
  const phone = isPhoneLayout();
  let idx = _settingsSearchIndexes.get(phone);
  if (!idx) {
    idx = buildSettingsSearchIndex(
      Object.keys(SECTION_SEARCH_PREFIXES).map(id => ({ id })),
      { exclude: phone ? [PAGINATION_SETTINGS_PREFIX] : [] },
    );
    _settingsSearchIndexes.set(phone, idx);
  }
  return idx;
}

const _MAX_SETTING_HITS = 8;

/**
 * @param {string} query
 * @returns {Array<{ id: string, label: string, description: string, category: string, path: string }>}
 */
function _matchIndividualSettings(query) {
  /** @type {Array<{ id: string, label: string, description: string, category: string, path: string }>} */
  const hits = [];
  for (const [sectionId, items] of _settingsSearchIndex()) {
    const sectionLabel = _SECTION_LABELS.get(sectionId);
    if (!sectionLabel) continue;
    for (const { label: text } of items) {
      if (hits.length >= _MAX_SETTING_HITS) return hits;
      if (!_matches(text, query)) continue;
      hits.push({
        id: `is-${sectionId}-${text}`,
        label: text,
        description: sectionLabel,
        category: 'settings',
        path: `/settings?section=${sectionId}&q=${encodeURIComponent(text)}`,
      });
    }
  }
  return hits;
}

/**
 * @param {{ onClose: () => void }} props
 */
function CommandPalette({ onClose }) {
  const [query, setQuery] = useState('');
  const [activeIdx, setActiveIdx] = useState(0);
  const [libraryItems, setLibraryItems] = useState(/** @type {Array<{id:string,label:string,category:string,path:string}>} */ ([]));
  const inputRef = useRef(/** @type {HTMLInputElement|null} */ (null));
  const listRef  = useRef(/** @type {HTMLUListElement|null} */ (null));
  const abortRef = useRef(/** @type {AbortController|null} */ (null));

  const trimmed = query.trim();

  const filteredNav = trimmed
    ? NAV_ITEMS.filter(i => _matches(i.label + ' ' + i.description, trimmed))
    : NAV_ITEMS;

  const filteredSettings = trimmed
    ? [...SETTINGS_ITEMS.filter(i => _matches(i.label + ' ' + i.description, trimmed)), ..._matchIndividualSettings(trimmed)]
    : [];

  const items = [...filteredNav, ...filteredSettings, ...libraryItems];

  useEffect(() => {
    if (abortRef.current) abortRef.current.abort();
    if (trimmed.length < 2) { setLibraryItems([]); return; }
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    const timer = setTimeout(async () => {
      try {
        const result = await api.getLibrary({ search: trimmed, page: 1, page_size: 20 }, ctrl.signal);
        if (ctrl.signal.aborted) return;
        const hits = /** @type {any[]} */ (result?.items ?? []);
        setLibraryItems(hits.map(m => ({
          id: `manga-${m.id}`,
          label: m.title ?? `Manga #${m.id}`,
          category: 'library',
          path: `/manga/${m.id}`,
        })));
      } catch {
        if (!ctrl.signal.aborted) setLibraryItems([]);
      }
    }, 180);
    return () => { clearTimeout(timer); ctrl.abort(); };
  }, [trimmed]);

  useEffect(() => { setActiveIdx(0); }, [items.length]);

  useEffect(() => {
    requestAnimationFrame(() => inputRef.current?.focus());
  }, []);

  useEffect(() => {
    if (!listRef.current) return;
    const el = listRef.current.querySelector('[data-active]');
    el?.scrollIntoView({ block: 'nearest' });
  }, [activeIdx]);

  const _activate = useCallback((/** @type {number} */ idx) => {
    const item = items[idx];
    if (!item) return;
    onClose();
    navigate(item.path);
  }, [items, onClose]);

  const onKeyDown = useCallback((/** @type {KeyboardEvent} */ e) => {
    if (e.key === 'Escape') { e.preventDefault(); onClose(); return; }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActiveIdx(i => Math.min(i + 1, items.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActiveIdx(i => Math.max(i - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      _activate(activeIdx);
    }
  }, [items.length, activeIdx, _activate, onClose]);

  const rows = [];
  let lastCat = '';
  let flatIdx = 0;
  for (const item of items) {
    if (item.category !== lastCat) {
      rows.push(h('li', {
        key: `hdr-${item.category}`,
        role: 'presentation',
        class: 'px-4 pt-3 pb-1 text-xs font-medium text-text-faint uppercase tracking-wider select-none',
      }, CATEGORY_LABELS[item.category]?.() ?? item.category));
      lastCat = item.category;
    }
    const idx = flatIdx++;
    const isActive = idx === activeIdx;
    rows.push(h('li', {
      key: item.id,
      role: 'option',
      'aria-selected': isActive,
      ...(isActive ? { 'data-active': '' } : {}),
      class: `flex items-center gap-3 px-4 py-2 cursor-pointer text-sm transition-colors ${isActive ? 'bg-surface-2' : 'hover:bg-surface-2'}`,
      onClick: () => _activate(idx),
      onMouseEnter: () => setActiveIdx(idx),
    },
      h('div', { class: 'flex-1 min-w-0' },
        h('div', { class: 'truncate text-text font-medium' }, item.label),
        item.description && h('div', { class: 'truncate text-xs text-text-muted' }, item.description),
      ),
    ));
  }

  return h('div', {
    class: 'fixed inset-0 z-modal-stack bg-scrim flex items-start justify-center pt-[12vh] px-4 pb-4',
    onClick: (/** @type {MouseEvent} */ e) => { if (e.target === e.currentTarget) onClose(); },
  },
    h('div', {
      class: 'bg-surface border border-border shadow-xl rounded-xl w-full max-w-lg flex flex-col overflow-hidden',
      role: 'dialog',
      'aria-label': t('palette.shortcut'),
      onKeyDown,
    },
      h('div', { class: 'flex items-center gap-2 px-4 border-b border-border-subtle' },
        h('span', { class: 'text-text-faint text-xs font-medium shrink-0', 'aria-hidden': 'true' }, modKeyLabel()),
        h('input', {
          ref: inputRef,
          type: 'text',
          class: 'flex-1 py-3.5 bg-transparent text-sm text-text outline-none placeholder:text-text-faint',
          placeholder: t('palette.placeholder'),
          value: query,
          onInput: (/** @type {InputEvent} */ e) => setQuery(/** @type {HTMLInputElement} */ (e.target).value),
          'aria-label': t('palette.shortcut'),
          role: 'combobox',
          'aria-expanded': 'true',
          'aria-haspopup': 'listbox',
          autocomplete: 'off',
        }),
        h('kbd', { class: 'shrink-0 text-xs text-text-faint border border-border-subtle rounded px-1.5 py-0.5' }, 'esc'),
      ),
      items.length > 0
        ? h('ul', {
            ref: listRef,
            class: 'overflow-y-auto max-h-[min(60vh,400px)] py-1',
            role: 'listbox',
          }, ...rows)
        : trimmed
          ? h('div', { class: 'px-4 py-8 text-center text-sm text-text-muted' }, t('palette.no_results'))
          : null,
      h('button', {
        type: 'button',
        class: 'flex items-center gap-1.5 px-4 py-2 text-xs text-text-faint hover:text-text-muted border-t border-border-subtle transition-colors',
        onClick: () => { onClose(); showCheatsheet(); },
      },
        h('kbd', { class: 'text-2xs border border-border-subtle rounded px-1 py-0.5' }, 'F1'),
        t('palette.shortcuts_hint'),
      ),
    ),
  );
}

/** @type {HTMLElement | null} */
let _container = null;

export function openCommandPalette() {
  if (_container) return;
  _container = document.createElement('div');
  document.body.appendChild(_container);

  const close = () => {
    if (!_container) return;
    render(null, _container);
    _container.remove();
    _container = null;
  };

  render(h(CommandPalette, { onClose: close }), _container);
}

