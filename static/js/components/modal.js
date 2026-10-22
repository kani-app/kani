// @ts-check
// Generic modal component — focus trap, escape-to-close, backdrop click.

import { h, render, Fragment } from 'preact';
import { createPortal } from 'preact/compat';
import { signal, effect } from '@preact/signals';
import { useEffect, useRef, useState } from 'preact/hooks';
import htm from 'htm';
import { iconX } from '../icons.js';
import { Icon } from './icon.js';
import { t } from '../i18n.js';
const html = htm.bind(h);

const _SKIP_PREFIX = 'kani-confirm-skip-';

/**
 * Modal component. Renders a centred overlay with a card.
 * @param {{
 *   open: boolean,
 *   onClose: () => void,
 *   title?: string,
 *   wide?: boolean,
 *   sheet?: boolean,
 *   footer?: any,
 *   children?: any,
 * }} props
 */
/** Stack of currently-open Modals (tokens); the last entry owns Escape. */
const _openStack = /** @type {object[]} */ ([]);

export function Modal({ open, onClose, title, wide = false, sheet = false, footer, focusContainer = false, children }) {
  const dialogRef = useRef(/** @type {HTMLDivElement | null} */ (null));
  const triggerRef = useRef(/** @type {Element | null} */ (null));
  const titleId = 'modal-title';

  // Store trigger element and restore focus on close
  useEffect(() => {
    if (open) {
      triggerRef.current = document.activeElement;
    } else {
      if (triggerRef.current instanceof HTMLElement) triggerRef.current.focus();
      triggerRef.current = null;
    }
  }, [open]);

  // Escape key to close — only the topmost open dialog reacts, so a nested
  // confirm doesn't drag its parent dialog down with it.
  useEffect(() => {
    if (!open) return;
    const token = {};
    _openStack.push(token);
    /** @param {KeyboardEvent} e */
    const onKey = (e) => {
      if (e.key === 'Escape' && _openStack[_openStack.length - 1] === token) onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => {
      const i = _openStack.indexOf(token);
      if (i !== -1) _openStack.splice(i, 1);
      document.removeEventListener('keydown', onKey);
    };
  }, [open, onClose]);

  // Focus trap: keep focus inside the modal
  useEffect(() => {
    if (!open || !dialogRef.current) return;
    const el = dialogRef.current;
    const focusable = /** @type {NodeListOf<HTMLElement>} */ (
      el.querySelectorAll('a,button:not(:disabled),input,select,textarea,[tabindex]:not([tabindex="-1"])')
    );
    // focusContainer: land focus on the dialog itself rather than the first
    // control (e.g. the close ✕), so opening doesn't flash an accent ring on it.
    if (focusContainer || !focusable.length) el.focus();
    else focusable[0].focus();

    /** @param {KeyboardEvent} e */
    const trapTab = (e) => {
      if (e.key !== 'Tab' || !focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey ? document.activeElement === first : document.activeElement === last) {
        e.preventDefault();
        (e.shiftKey ? last : first).focus();
      }
    };
    document.addEventListener('keydown', trapTab);
    return () => document.removeEventListener('keydown', trapTab);
  }, [open]);

  if (!open) return null;

  return html`
    <div
      class=${'fixed inset-0 z-modal flex justify-center bg-scrim '
        + (sheet ? 'items-end p-0' : 'items-end sm:items-center p-0 sm:p-4')}
      onClick=${(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby=${title ? titleId : undefined}
        tabindex="-1"
        class=${'relative bg-surface shadow-lg w-full flex flex-col overflow-hidden outline-none max-h-[85dvh] '
          + (sheet
              ? 'rounded-t-2xl pb-safe'
              : 'rounded-t-2xl sm:rounded-xl sm:max-h-modal ' + (wide ? 'modal-wide' : 'modal-narrow'))}
        ref=${dialogRef}
      >
        ${title && html`
          <div class="flex items-center justify-between gap-3 px-5 py-4 border-b border-border-subtle shrink-0">
            <h2 id=${titleId} class="text-lg font-semibold text-text">${title}</h2>
            <button class="btn-icon" aria-label=${t('common.close')} onClick=${onClose}><${Icon} svg=${iconX} /></button>
          </div>
        `}
        <div class="flex-1 overflow-y-auto p-5">${children}</div>
        ${footer && html`<div class="flex items-center justify-end gap-3 px-5 py-4 border-t border-border-subtle shrink-0">${footer}</div>`}
      </div>
    </div>
  `;
}


/** @type {import('@preact/signals').Signal<{ id: number, vnode: any }[]>} */
const _modalStack = signal([]);
let _nextModalId = 1;
let _hostMounted = false;

/** @param {{ stack: { id: number, vnode: any }[] }} props */
function ModalHost({ stack }) {
  const root = document.getElementById('modal-root');
  if (!root) return null;
  return createPortal(
    stack.map((entry) => h(Fragment, { key: entry.id }, entry.vnode)),
    root,
  );
}

function ensureModalHost() {
  if (_hostMounted) return;
  _hostMounted = true;
  const host = document.createElement('div');
  document.body.appendChild(host);
  effect(() => {
    render(h(ModalHost, { stack: _modalStack.value }), host);
  });
}

/**
 * Imperatively open a Preact tree as a modal. Returns a cleanup function that
 * closes it. Dialogs stack; passing null closes the entire stack.
 * @param {any} vnode
 * @returns {() => void}
 */
export function mountIntoModalRoot(vnode) {
  ensureModalHost();
  if (vnode == null) {
    if (_modalStack.value.length) _modalStack.value = [];
    return () => {};
  }
  const id = _nextModalId++;
  _modalStack.value = [..._modalStack.value, { id, vnode }];
  let done = false;
  return () => {
    if (done) return;
    done = true;
    _modalStack.value = _modalStack.value.filter((e) => e.id !== id);
  };
}


/**
 * @param {{
 *   title: string,
 *   message: string,
 *   confirmLabel: string,
 *   cancelLabel: string,
 *   danger: boolean,
 *   rememberKey?: string,
 *   onResolve: (result: boolean) => void,
 * }} props
 */
function ConfirmModal({ title, message, confirmLabel, cancelLabel, danger, rememberKey, onResolve }) {
  const [remember, setRemember] = useState(false);
  const confirmBtnRef = useRef(/** @type {HTMLButtonElement | null} */ (null));

  useEffect(() => {
    const id = setTimeout(() => confirmBtnRef.current?.focus(), 15);
    return () => clearTimeout(id);
  }, []);

  function confirm() {
    if (rememberKey && remember) {
      localStorage.setItem(_SKIP_PREFIX + rememberKey, '1');
    }
    onResolve(true);
  }

  return html`
    <${Modal}
      open=${true}
      title=${title}
      onClose=${() => onResolve(false)}
      footer=${html`
        <div class="flex items-center w-full gap-2">
          ${rememberKey && html`
            <label class="flex items-center gap-2 text-xs text-text-muted cursor-pointer select-none mr-auto">
              <input
                type="checkbox"
                class="accent-accent"
                checked=${remember}
                onChange=${(/** @type {any} */ e) => setRemember(e.target.checked)}
              />
              ${t('common.dont_ask_again')}
            </label>
          `}
          <div class=${'flex items-center gap-2' + (rememberKey ? '' : ' ml-auto')}>
            <button type="button" class="btn-ghost btn-sm" onClick=${() => onResolve(false)}>${cancelLabel}</button>
            <button
              type="button"
              class=${(danger ? 'btn-danger' : 'btn-primary') + ' btn-sm'}
              ref=${confirmBtnRef}
              onClick=${confirm}
            >${confirmLabel}</button>
          </div>
        </div>
      `}
    >
      <p class="text-sm text-text-muted">${message}</p>
    </${Modal}>
  `;
}

/**
 * Shows a confirm dialog. Returns a Promise resolving to true (confirmed) or false (cancelled).
 * Pass `rememberKey` to enable a "Don't ask again" checkbox backed by localStorage.
 * @param {string} message
 * @param {{
 *   title?: string,
 *   confirmLabel?: string,
 *   cancelLabel?: string,
 *   danger?: boolean,
 *   rememberKey?: string,
 * }} [opts]
 * @returns {Promise<boolean>}
 */
export function showConfirm(message, opts = {}) {
  const title = opts.title ?? t('confirm.title');
  const confirmLabel = opts.confirmLabel ?? t('confirm.confirm');
  const cancelLabel = opts.cancelLabel ?? t('common.cancel');
  const danger = opts.danger ?? false;
  const { rememberKey } = opts;

  if (rememberKey && localStorage.getItem(_SKIP_PREFIX + rememberKey) === '1') {
    return Promise.resolve(true);
  }

  return new Promise(resolve => {
    let cleanup = () => {};
    cleanup = mountIntoModalRoot(html`
      <${ConfirmModal}
        title=${title}
        message=${message}
        confirmLabel=${confirmLabel}
        cancelLabel=${cancelLabel}
        danger=${danger}
        rememberKey=${rememberKey}
        onResolve=${(/** @type {boolean} */ result) => { cleanup(); resolve(result); }}
      />
    `);
  });
}


/**
 * @param {{ message: string, title?: string, closeLabel?: string, onClose: () => void }} props
 */
function AlertModal({ message, title, closeLabel, onClose }) {
  title = title ?? t('common.notice');
  closeLabel = closeLabel ?? t('common.ok');
  return html`
    <${Modal}
      open=${true}
      title=${title}
      onClose=${onClose}
      footer=${html`
        <button type="button" class="btn-primary btn-sm" onClick=${onClose}>${closeLabel}</button>
      `}
    >
      <p class="text-sm text-text">${message}</p>
    </${Modal}>
  `;
}

/**
 * Shows an alert dialog using the app modal. Returns a Promise that resolves
 * when the user dismisses it.
 * @param {string} message
 * @param {{ title?: string }} [opts]
 * @returns {Promise<void>}
 */
export function showAlert(message, opts = {}) {
  return new Promise((resolve) => {
    let cleanup = () => {};
    cleanup = mountIntoModalRoot(html`
      <${AlertModal}
        message=${message}
        title=${opts.title ?? t('common.notice')}
        onClose=${() => { cleanup(); resolve(); }}
      />
    `);
  });
}
