// @ts-check
import { h, render } from 'preact';
import htm from 'htm';

const html = htm.bind(h);

/**
 * @template T
 * @typedef {{ id: T, name: string, count?: number, disabled?: boolean }} Tab
 */

/**
 * @template T
 * @param {{
 *   tabs: Tab<T>[],
 *   activeId: T,
 *   onSelect: (id: T) => void,
 *   variant?: 'underline' | 'pill',
 *   stretch?: boolean,
 * }} props
 */
export function Tabs({ tabs, activeId, onSelect, variant = 'underline', stretch = false }) {
  const barClass = variant === 'pill'
    ? 'flex gap-1 p-1 rounded-lg bg-surface-2 border border-border'
    : 'flex gap-1 overflow-x-auto [scrollbar-width:none] border-b border-border';

  return html`
    <div class=${barClass} role="tablist">
      ${tabs.map(tab => {
        const isActive = tab.id === activeId;
        let cls = 'tab-btn flex items-center gap-1.5 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent';
        if (variant === 'pill') {
          cls += ' px-3 py-1.5 rounded-md flex-1 justify-center'
            + (isActive ? ' bg-surface text-text shadow-sm' : ' text-text-muted hover:text-text');
        } else {
          cls += ' px-4 py-2 rounded-t-md'
            + (stretch ? ' flex-1 justify-center' : ' shrink-0 whitespace-nowrap')
            + (isActive ? ' text-accent border-b-2 border-accent' : ' text-text-muted');
        }
        if (tab.disabled) cls += ' opacity-40 cursor-not-allowed';
        return html`
          <button
            key=${tab.id}
            type="button"
            role="tab"
            aria-selected=${String(isActive)}
            aria-disabled=${String(!!tab.disabled)}
            disabled=${!!tab.disabled}
            class=${cls}
            onClick=${() => { if (!tab.disabled) onSelect(tab.id); }}
          >
            ${tab.name}
            ${tab.count != null && html`<span class="nav-badge">${tab.count}</span>`}
          </button>
        `;
      })}
    </div>
  `;
}

/**
 * @template T
 * @param {HTMLElement} container
 * @param {{
 *   tabs: Tab<T>[],
 *   activeId: T,
 *   onSelect: (id: T) => void,
 *   variant?: 'underline' | 'pill',
 *   stretch?: boolean,
 * }} props
 * @returns {{ update: (activeId: T, tabs?: Tab<T>[]) => void, destroy: () => void }}
 */
export function renderTabs(container, { tabs, activeId, onSelect, variant = 'underline', stretch = false }) {
  let _props = { tabs, activeId, onSelect, variant, stretch };
  const _mount = document.createElement('div');
  _mount.style.display = 'contents';
  container.appendChild(_mount);

  function _render() {
    render(html`<${Tabs} ...${_props} />`, _mount);
  }

  _render();

  return {
    update(newActiveId, newTabs) {
      _props = { ..._props, activeId: newActiveId, ...(newTabs ? { tabs: newTabs } : {}) };
      _render();
    },
    destroy() {
      render(null, _mount);
      _mount.remove();
    },
  };
}

/**
 * @param {HTMLElement} container
 * @param {{
 *   tabs: { id: number | null, name: string }[],
 *   activeId: number | null,
 *   onSelect: (id: number | null) => void,
 * }} props
 * @returns {{ update: (activeId: number | null) => void, destroy: () => void }}
 */
export function renderCategoryTabs(container, { tabs, activeId, onSelect }) {
  return renderTabs(container, { tabs, activeId, onSelect });
}
