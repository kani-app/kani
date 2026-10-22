// @ts-check
import { h, render, Fragment } from 'preact';
import htm from 'htm';
import { t } from '../../i18n.js';
import { iconChevronLeft, iconChevronRight, iconMenu, iconSettings, iconX } from '../../icons.js';

const html = htm.bind(h);

function IconButton({ icon, label, onClick, disabled = false, extraClass = '' }) {
  return html`
    <button class="btn-icon shrink-0 ${extraClass}" aria-label=${label}
            disabled=${disabled} onClick=${onClick}
            dangerouslySetInnerHTML=${{ __html: icon }}></button>`;
}

function mergingIsland(container, renderFn) {
  let _props = {};
  return {
    update(/** @type {object} */ next = {}) {
      _props = { ..._props, ...next };
      render(renderFn(_props), container);
    },
  };
}

/**
 * @param {HTMLElement} container
 * @param {{ onBack: () => void, onMenu: () => void }} deps
 */
export function createTopBar(container, { onBack, onMenu }) {
  return mergingIsland(container, ({ title }) => html`
    <${Fragment}>
      <${IconButton} icon=${iconChevronLeft} label=${t('reader.aria.back')} onClick=${onBack} />
      <span class="flex-1 text-sm font-medium text-text truncate">${title ?? ''}</span>
      <${IconButton} icon=${iconMenu} label=${t('reader.aria.open_menu')} onClick=${onMenu} />
    <//>`);
}

/**
 * @param {HTMLElement} container
 * @param {{ onBack: () => void, onSettings: () => void, onClose: () => void }} deps
 */
export function createSidePanelHeader(container, { onBack, onSettings, onClose }) {
  return mergingIsland(container, ({ title, meta }) => {
    const titleBlock = title
      ? html`
        <div class="flex-1 min-w-0">
          <div class="flex items-center gap-1.5 min-w-0">
            <span class="text-sm font-medium text-text truncate">${title}</span>
          </div>
          ${meta ? html`<span class="block text-xs text-muted truncate">${meta}</span>` : null}
        </div>`
      : html`<span class="flex-1 text-sm font-medium text-muted truncate">—</span>`;
    return html`
      <${Fragment}>
        <${IconButton} icon=${iconChevronLeft} label=${t('reader.aria.back_to_manga')} onClick=${onBack} />
        ${titleBlock}
        <${IconButton} icon=${iconSettings} label=${t('reader.aria.settings')} onClick=${onSettings} />
        <${IconButton} icon=${iconX} label=${t('reader.aria.close_menu')} onClick=${onClose} />
      <//>`;
  });
}

/**
 * @param {HTMLElement} container
 * @param {{ onBack: () => void, onPrev: () => void, onNext: () => void, onSelect: (id: number) => void }} deps
 */
export function createChapterNav(container, { onBack, onPrev, onNext, onSelect }) {
  return mergingIsland(container, ({ chapters = [], currentId, hasPrev = false, hasNext = false }) => {
    const options = chapters.length
      ? chapters.map(ch => html`<option value=${ch.id} selected=${ch.id === currentId}>${ch.title}</option>`)
      : html`<option>—</option>`;
    return html`
      <${Fragment}>
        <div class="px-3 py-3 flex gap-1.5 border-b border-border shrink-0 items-center">
          <button class="btn-ghost touch-min flex items-center justify-center gap-0.5 shrink-0 px-2"
                  disabled=${!hasPrev} onClick=${onPrev}
                  dangerouslySetInnerHTML=${{ __html: iconChevronLeft }}></button>
          <select class="input text-sm flex-1 min-w-0 text-center h-9 py-0"
                  disabled=${chapters.length === 0}
                  onChange=${(/** @type {any} */ e) => onSelect(Number(e.currentTarget.value))}>
            ${options}
          </select>
          <button class="btn-ghost touch-min flex items-center justify-center gap-0.5 shrink-0 px-2"
                  disabled=${!hasNext} onClick=${onNext}
                  dangerouslySetInnerHTML=${{ __html: iconChevronRight }}></button>
        </div>
      <//>`;
  });
}
