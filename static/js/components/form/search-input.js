
import { h } from 'preact';
import htm from 'htm';
import { iconSearch, iconX } from '../../icons.js';
import { t } from '../../i18n.js';

const html = htm.bind(h);

/**
 * @param {{
 *   value: string,
 *   onInput: (value: string) => void,
 *   placeholder?: string,
 *   ariaLabel?: string,
 *   size?: 'sm' | 'md',
 *   class?: string,
 * }} props
 */
export function SearchInput({ value, onInput, placeholder, ariaLabel, size = 'md', class: klass = '' }) {
  return html`
    <div class=${'search-input ' + (size === 'sm' ? 'search-input--sm ' : '') + klass}>
      <span class="search-input__icon icon-sm" aria-hidden="true" dangerouslySetInnerHTML=${{ __html: iconSearch }} />
      <input
        type="search"
        class=${'input w-full' + (size === 'sm' ? ' input-sm' : '')}
        placeholder=${placeholder ?? ''}
        aria-label=${ariaLabel ?? placeholder ?? ''}
        value=${value}
        onInput=${(/** @type {any} */ e) => onInput(e.target.value)}
      />
      <button type="button" class="search-input__clear" aria-label=${t('search.clear')}
        onMouseDown=${(/** @type {MouseEvent} */ e) => e.preventDefault()}
        onClick=${() => onInput('')}
        dangerouslySetInnerHTML=${{ __html: iconX }} />
    </div>
  `;
}

/**
 * Vanilla factory for the same control. The returned `input` fires normal
 * `input` events (including when cleared via the ✕), so existing listeners
 * work unchanged.
 *
 * @param {{
 *   value?: string,
 *   placeholder?: string,
 *   ariaLabel?: string,
 *   size?: 'sm' | 'md',
 *   inputClass?: string,
 *   id?: string,
 * }} opts
 * @returns {{ el: HTMLElement, input: HTMLInputElement }}
 */
export function createSearchInput({ value = '', placeholder, ariaLabel, size = 'md', inputClass = '', id } = {}) {
  const el = document.createElement('div');
  el.className = 'search-input' + (size === 'sm' ? ' search-input--sm' : '');

  const icon = document.createElement('span');
  icon.className = 'search-input__icon icon-sm';
  icon.setAttribute('aria-hidden', 'true');
  icon.innerHTML = iconSearch;
  el.appendChild(icon);

  const input = document.createElement('input');
  input.type = 'search';
  input.className = ('input w-full ' + (size === 'sm' ? 'input-sm ' : '') + inputClass).trim();
  // The clear affordance relies on :placeholder-shown, so a placeholder must exist.
  input.placeholder = placeholder ?? ' ';
  if (ariaLabel ?? placeholder) input.setAttribute('aria-label', /** @type {string} */ (ariaLabel ?? placeholder));
  if (id) input.id = id;
  input.value = value;
  el.appendChild(input);

  const clear = document.createElement('button');
  clear.type = 'button';
  clear.className = 'search-input__clear';
  clear.setAttribute('aria-label', t('search.clear'));
  clear.innerHTML = iconX;
  clear.addEventListener('mousedown', e => e.preventDefault());
  clear.addEventListener('click', () => {
    input.value = '';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.focus();
  });
  el.appendChild(clear);

  return { el, input };
}
