// @ts-check

import { renderTabs } from './tabs.js';
import { t } from '../i18n.js';

/**
 * The Extensions/Repositories switcher.
 *
 * It lives in the page body rather than the header because it is navigation —
 * which of two views you are looking at — and header actions are the first
 * thing a narrow viewport sacrifices. As a header action it was demoted into an
 * unlabelled kebab on every phone, so nothing indicated the Repositories view
 * existed. The underline tab bar also shows which view is active, which two
 * equally-weighted buttons and an `aria-pressed` did not.
 *
 * @param {HTMLElement} container
 * @param {{ onTab: (tab: 'extensions' | 'repos') => void }} opts
 * @returns {{ update: (tab: 'extensions' | 'repos') => void, destroy: () => void }}
 */
export function mountSourcesViewTabs(container, { onTab }) {
  return renderTabs(container, {
    tabs: [
      { id: /** @type {const} */ ('extensions'), name: t('sources.tab.extensions') },
      { id: /** @type {const} */ ('repos'), name: t('repo.tab') },
    ],
    activeId: 'extensions',
    onSelect: onTab,
  });
}

/**
 * @param {{ canInstall: boolean }} opts
 */
export function createSourcesHeaderActions({ canInstall }) {
  /** @type {HTMLButtonElement | null} */
  let addSourceBtn = null;
  if (canInstall) {
    addSourceBtn = document.createElement('button');
    addSourceBtn.type = 'button';
    addSourceBtn.className = 'btn-primary btn-sm';
    addSourceBtn.textContent = t('source.add.title');
  }

  /** @param {'extensions' | 'repos'} tab */
  const setActive = (tab) => {
    addSourceBtn?.classList.toggle('hidden', tab === 'repos');
  };

  const actions = /** @type {HTMLElement[]} */ ([]);
  if (addSourceBtn) actions.push(addSourceBtn);

  return { actions, addSourceBtn, setActive };
}
