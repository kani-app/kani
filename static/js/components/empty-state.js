// @ts-check
import { h, render } from 'preact';
import htm from 'htm';

const html = htm.bind(h);

/**
 * `compact` trades the icon and the tall padding for a single quiet line — use it
 * inside a card sub-list or table body, where the full treatment would dwarf the
 * panel it sits in.
 */
export function EmptyState({ icon, title, subtitle, action, compact = false }) {
  if (compact) {
    return html`
      <div class="flex flex-col items-center gap-1 px-4 py-6 text-center">
        <p class="text-sm text-text-muted">${title}</p>
        ${subtitle && html`<p class="text-xs text-text-faint">${subtitle}</p>`}
        ${action && ('href' in action
          ? html`<a href=${action.href} class="btn-secondary btn-sm mt-1">${action.label}</a>`
          : html`<button type="button" class="btn-secondary btn-sm mt-1" onClick=${action.onClick}>${action.label}</button>`
        )}
      </div>
    `;
  }
  return html`
    <div class="flex flex-col items-center justify-center gap-4 px-4 py-12 sm:px-6 sm:py-16 text-center">
      ${icon && html`<span class="text-text-muted icon-3xl" aria-hidden="true" dangerouslySetInnerHTML=${{ __html: icon }} />`}
      <p class="text-base font-medium text-text">${title}</p>
      ${subtitle && html`<p class="text-sm text-text-muted">${subtitle}</p>`}
      ${action && ('href' in action
        ? html`<a href=${action.href} class="btn-primary">${action.label}</a>`
        : html`<button type="button" class="btn-primary" onClick=${action.onClick}>${action.label}</button>`
      )}
    </div>
  `;
}

export function createEmptyState(opts) {
  const el = document.createElement('div');
  render(html`<${EmptyState} ...${opts} />`, el);
  return el;
}
