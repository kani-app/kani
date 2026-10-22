// @ts-check
// Shared building blocks for the auth pages (login, register, forgot/reset
// password, verify email): centred card shell, alert boxes, labelled field.

import { h } from 'preact';
import htm from 'htm';
import { iconX } from '../icons.js';
import { Icon } from './icon.js';
const html = htm.bind(h);

/**
 * Centred full-viewport card shell.
 * @param {{ title: any, subtitle?: any, center?: boolean, children?: any }} props
 */
export function AuthCard({ title, subtitle, center = false, children }) {
  return html`
    <div class="min-h-screen flex items-center justify-center p-4 bg-bg">
      <div class=${'w-full max-w-sm auth-card p-8 flex flex-col gap-6' + (center ? ' text-center' : '')}>
        <div class="flex flex-col items-center gap-3">
          <span class="auth-mark" aria-hidden="true">K</span>
          <div class="text-center flex flex-col gap-1">
            <h1 class="text-2xl text-text">${title}</h1>
            ${subtitle && html`<p class="text-sm text-text-muted">${subtitle}</p>`}
          </div>
        </div>
        ${children}
      </div>
    </div>
  `;
}

/**
 * Inline error alert. Renders nothing when message is falsy.
 * @param {{ message?: string, id?: string }} props
 */
export function AuthError({ message, id }) {
  if (!message) return null;
  return html`
    <div
      id=${id}
      class="flex items-center gap-2 px-3 py-2.5 rounded-lg bg-danger/10 border border-danger/30 text-sm text-danger"
      role="alert"
      aria-live="assertive"
    >
      <span aria-hidden="true" class="shrink-0 icon-sm"><${Icon} svg=${iconX} /></span>
      <span>${message}</span>
    </div>
  `;
}

/**
 * Inline success note.
 * @param {{ children?: any }} props
 */
export function AuthSuccess({ children }) {
  return html`
    <div class="px-3 py-2.5 rounded-lg bg-success/10 border border-success/30 text-sm text-success" role="status">
      ${children}
    </div>
  `;
}

/**
 * Labelled form field.
 * @param {{
 *   id: string,
 *   label: any,
 *   type?: string,
 *   value: string,
 *   onInput: (value: string) => void,
 *   autocomplete?: string,
 *   required?: boolean,
 *   autofocus?: boolean,
 *   inputMode?: string,
 *   describedBy?: string,
 * }} props
 */
export function AuthField({ id, label, type = 'text', value, onInput, autocomplete, required = false, autofocus = false, inputMode, describedBy }) {
  return html`
    <div class="flex flex-col gap-1.5">
      <label class="text-sm font-medium text-text" for=${id}>${label}</label>
      <input
        id=${id}
        class="input"
        type=${type}
        value=${value}
        autocomplete=${autocomplete}
        required=${required}
        autofocus=${autofocus}
        inputMode=${inputMode}
        aria-describedby=${describedBy}
        onInput=${(/** @type {any} */ e) => onInput(e.target.value)}
      />
    </div>
  `;
}
