// @ts-check
// Global tooltip controller. Renders a small popover for any element with a
// [data-tooltip] attribute on hover or keyboard focus. Call initTooltip() once
// from app.js to activate.

/** @type {HTMLElement|null} */
let _tip = null;

/**
 * Initialise the tooltip controller. Idempotent — safe to call multiple times.
 */
export function initTooltip() {
  if (_tip) return;

  _tip = document.createElement('div');
  _tip.id = 'kani-tooltip';
  _tip.className =
    'fixed z-50 px-2.5 py-1.5 rounded-lg text-xs text-text bg-surface-3 shadow-popover ' +
    'border border-border pointer-events-none opacity-0 transition-opacity duration-100 max-w-xs';
  _tip.setAttribute('role', 'tooltip');
  document.body.appendChild(_tip);

  document.addEventListener('mouseover', _onEnter);
  document.addEventListener('focusin', _onEnter);
  document.addEventListener('mouseout', _onLeave);
  document.addEventListener('focusout', _onLeave);
}

/** @param {Event} e */
function _onEnter(e) {
  const target = /** @type {Element} */ (e.target);
  const el = target.closest('[data-tooltip]');
  if (!el || !_tip) return;

  const text = el.getAttribute('data-tooltip') ?? '';
  if (!text) return;

  _tip.textContent = text;
  _tip.style.opacity = '1';
  _position(/** @type {HTMLElement} */ (el));
}

function _onLeave() {
  if (_tip) _tip.style.opacity = '0';
}

/** @param {HTMLElement} anchor */
function _position(anchor) {
  if (!_tip) return;
  const r = anchor.getBoundingClientRect();
  const tipW = _tip.offsetWidth || 200;
  let left = r.left + r.width / 2 - tipW / 2;
  left = Math.max(8, Math.min(left, window.innerWidth - tipW - 8));
  const top = r.bottom + 6;
  _tip.style.left = `${left}px`;
  _tip.style.top = `${top}px`;
}
