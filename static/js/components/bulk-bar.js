
import { h } from 'preact';
import { useRef, useState } from 'preact/hooks';
import htm from 'htm';
import { t } from '../i18n.js';
import { ContextMenu } from './menu.js';
import { Icon } from './icon.js';
import { iconChevronDown } from '../icons.js';

const html = htm.bind(h);

/** Above this many, helpers move into the menu instead of sitting in the bar. */
const INLINE_HELPER_LIMIT = 2;

/**
 * Actions kept in the bar on a phone. Nine wrapped to three rows, a quarter of
 * the screen, pushing the content being selected out of view.
 */
const INLINE_ACTION_LIMIT = 2;

/**
 * @typedef {{ label: string, onClick: () => void, kind?: 'secondary'|'danger', disabled?: boolean, title?: string }} BulkAction
 * @typedef {{ label: string, onClick: () => void, title?: string, disabled?: boolean }} BulkHelper
 */

/**
 * @param {{
 *   countLabel: string,
 *   statLine?: string | null,
 *   helpers?: BulkHelper[],
 *   actions: BulkAction[],
 *   onCancel: () => void,
 *   busy?: boolean,
 * }} props
 */
export function BulkBar({ countLabel, statLine = null, helpers = [], actions, onCancel, busy = false }) {
  const menuBtn = useRef(/** @type {HTMLButtonElement|null} */ (null));
  const moreBtn = useRef(/** @type {HTMLButtonElement|null} */ (null));
  const [menuOpen, setMenuOpen] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const collapsed = helpers.length > INLINE_HELPER_LIMIT;

  // Destructive actions stay in the bar whatever else overflows: hiding one
  // behind a menu makes it undiscoverable, and it already reads differently
  // from the rest.
  const danger = actions.filter((a) => a.kind === 'danger');
  const plain = actions.filter((a) => a.kind !== 'danger');
  // Never hide a single action behind a menu — a "More (1)" costs a tap and saves nothing.
  const overflowing = plain.length > INLINE_ACTION_LIMIT + 1;
  const inlineActions = overflowing ? plain.slice(0, INLINE_ACTION_LIMIT) : plain;
  const overflowActions = overflowing ? plain.slice(INLINE_ACTION_LIMIT) : [];

  const helperGroup = helpers.length === 0 ? null : collapsed
    ? html`
        <button
          ref=${menuBtn}
          type="button"
          class="btn-ghost btn-sm inline-flex items-center gap-1 whitespace-nowrap"
          aria-haspopup="menu"
          aria-expanded=${menuOpen}
          onClick=${() => setMenuOpen((o) => !o)}
        >
          ${t('bulk.select')}
          <${Icon} svg=${iconChevronDown} class="icon-xs" />
        </button>
        ${menuOpen && html`<${ContextMenu}
          items=${helpers.map((hp) => ({ label: hp.label, action: hp.onClick, disabled: hp.disabled }))}
          trigger=${menuBtn}
          onClose=${() => setMenuOpen(false)}
        />`}
      `
    : helpers.map((hp) => html`
        <button key=${hp.label} type="button" class="btn-ghost btn-sm whitespace-nowrap"
          title=${hp.title} disabled=${hp.disabled} onClick=${hp.onClick}>
          ${hp.label}
        </button>
      `);

  return html`
    <div class="bulk-bar-dock z-40 bg-surface border border-border-subtle rounded-none md:rounded-2xl shadow-xl flex items-center gap-x-3 gap-y-2 px-4 py-2.5 flex-wrap pb-safe md:pb-2.5">
      <div class="flex flex-col min-w-0 mr-auto">
        <span class="text-sm font-medium text-text-muted whitespace-nowrap">${countLabel}</span>
        ${statLine && html`<span class="text-xs text-text-faint whitespace-nowrap">${statLine}</span>`}
      </div>

      ${helperGroup && html`
        <div class="flex items-center gap-1 relative">
          ${helperGroup}
        </div>
        <span class="hidden md:block w-px self-stretch bg-border-subtle" aria-hidden="true"></span>
      `}

      <div class="flex items-center gap-1.5 flex-wrap relative">
        ${[...inlineActions, ...danger].map(a => html`
          <button
            key=${a.label}
            type="button"
            class=${(a.kind === 'danger' ? 'btn-danger' : 'btn-secondary') + ' btn-sm js-bulk-action whitespace-nowrap'}
            disabled=${busy || a.disabled}
            title=${a.title}
            onClick=${a.onClick}
          >${a.label}</button>
        `)}
        ${overflowActions.length > 0 && html`
          <button
            ref=${moreBtn}
            type="button"
            class="btn-ghost btn-sm inline-flex items-center gap-1 whitespace-nowrap"
            aria-haspopup="menu"
            aria-expanded=${moreOpen}
            disabled=${busy}
            onClick=${() => setMoreOpen((o) => !o)}
          >
            ${t('bulk.more', { count: overflowActions.length })}
            <${Icon} svg=${iconChevronDown} class="icon-xs" />
          </button>
          ${moreOpen && html`<${ContextMenu}
            items=${overflowActions.map((a) => ({ label: a.label, action: a.onClick, disabled: busy || a.disabled }))}
            trigger=${moreBtn}
            onClose=${() => setMoreOpen(false)}
          />`}
        `}
        <button type="button" class="btn-ghost btn-sm whitespace-nowrap" onClick=${onCancel}>${t('common.cancel')}</button>
      </div>
    </div>
  `;
}
