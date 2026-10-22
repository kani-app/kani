// @ts-check

/**
 * @typedef {{ id: string | number, label: string }} ChipItem
 */

/**
 * @param {HTMLElement} container
 * @param {{
 *   items: ChipItem[],
 *   selected: Set<string | number>,
 *   onToggle: (id: string | number, selected: boolean) => void,
 *   multi?: boolean,
 * }} props
 * @returns {{ selected: () => Set<string | number>, update: (selected: Set<string | number>) => void, destroy: () => void }}
 */
export function renderChipGroup(container, { items, selected, onToggle, multi = true }) {
  let _selected = new Set(selected);
  let _focusIdx = 0;

  const wrap = document.createElement('div');
  wrap.className = 'flex flex-wrap gap-2';
  wrap.setAttribute('role', 'group');
  container.appendChild(wrap);

  function _chips() {
    return /** @type {HTMLButtonElement[]} */ ([...wrap.querySelectorAll('button')]);
  }

  function _moveFocus(delta) {
    const chips = _chips();
    if (!chips.length) return;
    _focusIdx = (_focusIdx + delta + chips.length) % chips.length;
    chips[_focusIdx]?.focus();
  }

  function _render() {
    const prevFocusIdx = _focusIdx;
    wrap.innerHTML = '';
    items.forEach((item, i) => {
      const isActive = _selected.has(item.id);
      const chip = document.createElement('button');
      chip.type = 'button';
      chip.className = isActive ? 'chip chip-active' : 'chip';
      chip.textContent = item.label;
      chip.setAttribute('aria-pressed', String(isActive));
      chip.tabIndex = i === prevFocusIdx ? 0 : -1;

      chip.addEventListener('click', () => {
        _focusIdx = i;
        if (multi) {
          const nowActive = !_selected.has(item.id);
          if (nowActive) {
            _selected.add(item.id);
          } else {
            _selected.delete(item.id);
          }
          onToggle(item.id, nowActive);
        } else {
          const alreadyActive = _selected.has(item.id);
          _selected = new Set(alreadyActive ? [] : [item.id]);
          onToggle(item.id, !alreadyActive);
        }
        _render();
        _chips()[_focusIdx]?.focus();
      });

      chip.addEventListener('keydown', (e) => {
        if (e.key === 'ArrowRight' || e.key === 'ArrowDown') { e.preventDefault(); _moveFocus(1); }
        else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') { e.preventDefault(); _moveFocus(-1); }
        else if (e.key === 'Home') { e.preventDefault(); _focusIdx = 0; _chips()[0]?.focus(); }
        else if (e.key === 'End')  { e.preventDefault(); _focusIdx = items.length - 1; _chips()[items.length - 1]?.focus(); }
      });

      wrap.appendChild(chip);
    });
  }

  _render();

  return {
    selected() {
      return new Set(_selected);
    },
    update(newSelected) {
      _selected = new Set(newSelected);
      _render();
    },
    destroy() {
      wrap.remove();
    },
  };
}
