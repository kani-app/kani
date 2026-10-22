// @ts-check
// Shared DOM builder helpers for manga-details manage panels.

/** @param {string} title @param {string} subtitle */
export function mkSectionHeader(title, subtitle) {
  const el = document.createElement('div');
  el.className = 'flex flex-col gap-0.5 pb-2 border-b border-border-subtle';
  const h = document.createElement('h2');
  h.className = 'text-sm font-semibold text-text';
  h.textContent = title;
  el.appendChild(h);
  if (subtitle) {
    const s = document.createElement('p');
    s.className = 'text-xs text-text-muted';
    s.textContent = subtitle;
    el.appendChild(s);
  }
  return el;
}

export function mkCard() {
  const card = document.createElement('div');
  card.className = 'bg-surface border border-border rounded-xl px-4 md:px-6 py-1';
  return card;
}

/** @param {string} title @param {string} subtitle */
export function mkTitledCard(title, subtitle) {
  const card = document.createElement('div');
  card.className = 'bg-surface border border-border rounded-xl p-4 md:p-6';
  const h = document.createElement('h3');
  h.className = 'text-sm font-semibold text-text';
  h.textContent = title;
  card.appendChild(h);
  const s = document.createElement('p');
  s.className = 'text-xs text-text-muted mt-0.5';
  s.textContent = subtitle;
  card.appendChild(s);
  const sep = document.createElement('div');
  sep.className = 'border-t border-border-subtle mt-3 mb-4';
  card.appendChild(sep);
  return card;
}

/**
 * @param {string} label
 * @param {string|null} sublabel
 * @param {HTMLElement} control
 */
export function mkRow(label, sublabel, control) {
  const row = document.createElement('div');
  row.className = 'flex items-center justify-between gap-4';
  const text = document.createElement('div');
  const lEl = document.createElement('p');
  lEl.className = 'text-sm font-medium text-text';
  lEl.textContent = label;
  text.appendChild(lEl);
  if (sublabel) {
    const sEl = document.createElement('p');
    sEl.className = 'text-xs text-text-muted mt-0.5';
    sEl.textContent = sublabel;
    text.appendChild(sEl);
  }
  row.appendChild(text);
  control.classList.add('shrink-0');
  row.appendChild(control);
  return row;
}

/** @param {HTMLElement} rowEl */
export function mkItem(rowEl) {
  const item = document.createElement('div');
  item.className = 'py-4 first:pt-3 last:pb-3 border-b border-border-subtle last:border-b-0';
  item.appendChild(rowEl);
  return item;
}
