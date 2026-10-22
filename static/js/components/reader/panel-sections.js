// @ts-check
import { h } from 'preact';
import { useState } from 'preact/hooks';
import htm from 'htm';
import { t } from '../../i18n.js';
import { SelectRow, ActionBtn } from './settings-controls.js';

const html = htm.bind(h);

/** @param {{ title: string, defaultOpen?: boolean, children?: any }} props */
export function PanelAccordion({ title, defaultOpen = false, children }) {
  const key = `kani_reader_acc_${title.toLowerCase().replace(/\s+/g, '_')}`;
  const stored = localStorage.getItem(key);
  const [open, setOpen] = useState(stored !== null ? stored === 'true' : defaultOpen);
  const toggle = () => setOpen((o) => { const n = !o; localStorage.setItem(key, String(n)); return n; });
  return html`
    <div class="border-b border-border">
      <button type="button" aria-expanded=${open}
              class="w-full flex items-center justify-between px-3 py-3 text-xs font-medium text-muted uppercase tracking-wide hover:text-text transition-colors"
              onClick=${toggle}>
        <span>${title}</span>
        <span class="transition-transform duration-150 text-base leading-none ${open ? 'rotate-180' : ''}">▾</span>
      </button>
      <div class="flex flex-col gap-3 px-3 pb-4" style=${open ? '' : 'display:none'}>${children}</div>
    </div>`;
}

/** @param {{ value: string, onInput: (v: string) => void }} props */
export function NoteBody({ value, onInput }) {
  return html`
    <textarea rows="3" placeholder=${t('reader.note.placeholder')} value=${value}
      class="w-full text-sm bg-surface-2 border border-border rounded-md px-2 py-1.5 resize-none outline-none focus:border-accent"
      onInput=${(/** @type {any} */ e) => onInput(e.currentTarget.value)}
      onKeyDown=${(/** @type {any} */ e) => e.stopPropagation()}></textarea>`;
}

/** @param {{ addLabel: string, onToggle: () => void, bookmarks: number[], onJump: (pg: number) => void }} props */
export function BookmarksBody({ addLabel, onToggle, bookmarks, onJump }) {
  return html`
    <${ActionBtn} label=${addLabel} onClick=${onToggle} />
    <div class="flex flex-col gap-1">
      ${bookmarks.length === 0
        ? html`<p class="text-xs text-muted">${t('reader.bookmarks.empty')}</p>`
        : bookmarks.map(pg => html`
            <button class="text-xs text-left text-text hover:text-accent py-0.5" onClick=${() => onJump(pg)}>
              ${t('reader.bookmark.page', { n: pg + 1 })}
            </button>`)}
    </div>`;
}

/** @param {{ options: {value: string, label: string}[], selected: string, disabled: boolean, onChange: (v: string) => void }} props */
export function ScanlatorBody({ options, selected, disabled, onChange }) {
  return html`<${SelectRow} options=${options} selected=${selected} disabled=${disabled} onChange=${onChange} />`;
}
