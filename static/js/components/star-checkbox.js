// @ts-check

import { iconStarFilled, iconStarOutline } from '../icons.js';

/**
 * @param {{
 *   checked: boolean,
 *   onChange: (checked: boolean) => void,
 *   label?: string,
 * }} props
 * @returns {{ el: HTMLElement, update: (checked: boolean) => void }}
 */
export function createStarCheckbox({ checked, onChange, label = 'Favourite' }) {
  let _checked = checked;

  const wrap = document.createElement('label');
  wrap.className = 'kani-toggle inline-flex items-center cursor-pointer';
  wrap.title = label;

  const input = document.createElement('input');
  input.type = 'checkbox';
  input.className = 'sr-only peer';
  input.checked = _checked;
  input.setAttribute('aria-label', label);

  const icon = document.createElement('span');
  icon.className = 'icon-md transition-colors duration-150 cursor-pointer peer-checked:text-warn text-text-muted';
  icon.setAttribute('aria-hidden', 'true');
  icon.innerHTML = _checked ? iconStarFilled : iconStarOutline;

  input.addEventListener('change', () => {
    _checked = input.checked;
    icon.innerHTML = _checked ? iconStarFilled : iconStarOutline;
    onChange(_checked);
  });

  wrap.appendChild(input);
  wrap.appendChild(icon);

  return {
    el: wrap,
    update(newChecked) {
      _checked = newChecked;
      input.checked = _checked;
      icon.innerHTML = _checked ? iconStarFilled : iconStarOutline;
    },
  };
}
