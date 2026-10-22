// @ts-check
// Global keyboard shortcut manager.
// Scoped registrations (e.g. 'reader') are unregistered when the page destroys.
// F1 opens a cheatsheet of currently active shortcuts (or triggers a registered override).

import { h } from 'preact';
import htm from 'htm';
import { t } from './i18n.js';
import { Modal, mountIntoModalRoot } from './components/modal.js';

const html = htm.bind(h);

/**
 * @typedef {{ key: string, description: string }} ShortcutEntry
 * @typedef {{ key: string | string[], description: string, handler: () => void }} ShortcutDef
 */

/** @type {Map<string, ShortcutEntry[]>} — scope → display entries */
const _registry = new Map();
/** @type {Map<string, (e: KeyboardEvent) => void>} — scope → handler fn */
const _handlers = new Map();
/** Optional override for the F1 help shortcut (e.g. opens a page-specific settings modal). */
let _f1Override = /** @type {(() => void)|null} */ (null);

let _globalListenerAttached = false;

function _onKeyDown(/** @type {KeyboardEvent} */ e) {
  const tag = /** @type {HTMLElement} */ (e.target)?.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
  if (/** @type {HTMLElement} */ (e.target)?.isContentEditable) return;
  if (e.metaKey || e.ctrlKey || e.altKey) return;

  if (e.key === 'F1') {
    e.preventDefault();
    if (_f1Override) { _f1Override(); return; }
    _showCheatsheet();
    return;
  }

  for (const handler of _handlers.values()) {
    handler(e);
  }
}

function _attachGlobal() {
  if (!_globalListenerAttached) {
    document.addEventListener('keydown', _onKeyDown);
    _globalListenerAttached = true;
  }
}

/**
 * Registers a set of shortcuts under a named scope.
 * Returns an unregister function — call it when the page/component destroys.
 *
 * @param {string} scope
 * @param {ShortcutDef[]} shortcuts
 * @returns {() => void} unregister
 */
export function registerShortcuts(scope, shortcuts) {
  _attachGlobal();

  const entries = /** @type {ShortcutEntry[]} */ ([]);
  for (const s of shortcuts) {
    const keys = Array.isArray(s.key) ? s.key : [s.key];
    const primary = keys[0];
    entries.push({ key: primary, description: s.description });
  }
  _registry.set(scope, entries);

  _handlers.set(scope, (e) => {
    for (const s of shortcuts) {
      const keys = Array.isArray(s.key) ? s.key : [s.key];
      if (keys.includes(e.key)) {
        e.preventDefault();
        s.handler();
        return;
      }
    }
  });

  return () => {
    _registry.delete(scope);
    _handlers.delete(scope);
  };
}

/**
 * Returns the registered shortcut entries for a scope (or all if omitted).
 * @param {string} [scope]
 * @returns {ShortcutEntry[]}
 */
/**
 * Register a function to call instead of the default F1 cheatsheet.
 * Pass null to restore default behaviour.
 * @param {(() => void)|null} fn
 */
export function setF1Override(fn) { _f1Override = fn; }

export function getShortcuts(scope) {
  if (scope) return _registry.get(scope) ?? [];
  const all = [];
  for (const entries of _registry.values()) all.push(...entries);
  return all;
}

export function showCheatsheet() { _showCheatsheet(); }

let _cheatsheetOpen = false;

function _showCheatsheet() {
  if (_cheatsheetOpen) return;
  _cheatsheetOpen = true;

  const sections = [..._registry].filter(([, entries]) => entries.length > 0);

  let cleanup = () => {};
  const onClose = () => { _cheatsheetOpen = false; cleanup(); };

  cleanup = mountIntoModalRoot(html`
    <${Modal} open=${true} title=${t('shortcuts.cheatsheet.title')} onClose=${onClose}>
      <div class="flex flex-col gap-3">
        ${sections.length === 0 && html`<p class="text-sm text-text-muted">${t('shortcuts.cheatsheet.empty')}</p>`}
        ${sections.map(([scope, entries]) => html`
          <div key=${scope} class="flex flex-col gap-1">
            <p class="eyebrow mb-1">${scope}</p>
            ${entries.map(entry => html`
              <div class="flex items-center justify-between gap-4">
                <span class="text-sm text-text">${entry.description}</span>
                <kbd class="text-xs bg-surface-2 border border-border rounded px-1.5 py-0.5 font-mono shrink-0">${entry.key}</kbd>
              </div>
            `)}
          </div>
        `)}
      </div>
    </${Modal}>
  `);
}
