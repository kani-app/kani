// @ts-check
import { sanitizeCss } from './sanitize-css.js';
// audit-ignore-file: built-in theme palette source.

/** @type {ReadonlyArray<{color: string, label: string}>} */
export const ACCENT_SWATCHES = [
  { color: '#e0523f', label: 'Kani vermilion' },
  { color: '#3d8ef5', label: 'Ocean'     },
  { color: '#3a9e67', label: 'Forest'    },
  { color: '#9b6ec8', label: 'Amethyst'  },
  { color: '#d4870f', label: 'Amber'     },
  { color: '#e05585', label: 'Rose'      },
];

const _DEFAULT_ACCENT = '#e0523f';

/**
 * The 13 manually-editable colour tokens that define a custom theme.
 * accent-hover and accent-dim are auto-derived from accent and not listed here.
 * @type {ReadonlyArray<string>}
 */
export const CORE_TOKENS = [
  '--color-bg',
  '--color-surface',
  '--color-surface-2',
  '--color-surface-3',
  '--color-border',
  '--color-border-subtle',
  '--color-accent',
  '--color-text',
  '--color-text-muted',
  '--color-text-faint',
  '--color-success',
  '--color-warn',
  '--color-danger',
];

/**
 * All 15 stored tokens (CORE_TOKENS + the 2 derived accent tokens).
 * Used when clearing a custom theme from the document.
 * @type {ReadonlyArray<string>}
 */
const _ALL_THEME_TOKENS = [...CORE_TOKENS, '--color-accent-hover', '--color-accent-dim'];

/**
 * `id` is the server id; there is deliberately no second local identity space.
 * @typedef {{
 *   id: string,
 *   name: string,
 *   tokens: Record<string, string>,
 *   customCss?: string,
 *   instanceWide?: boolean,
 * }} CustomTheme
 */

/** @returns {CustomTheme[]} */
export function getCustomThemes() {
  try {
    const raw = localStorage.getItem('kani-custom-themes');
    return raw ? JSON.parse(raw) : [];
  } catch { return []; }
}

/** @param {CustomTheme[]} themes */
function _saveCustomThemeList(themes) {
  localStorage.setItem('kani-custom-themes', JSON.stringify(themes));
}

/** @param {CustomTheme} theme */
export function saveCustomTheme(theme) {
  const all = getCustomThemes();
  const idx = all.findIndex(t => t.id === theme.id);
  if (idx >= 0) all[idx] = theme;
  else all.push(theme);
  _saveCustomThemeList(all);
}

/** @param {string} id */
export function deleteCustomTheme(id) {
  _saveCustomThemeList(getCustomThemes().filter(t => t.id !== id));
  if (localStorage.getItem('kani-theme') === `custom:${id}`) {
    localStorage.removeItem('kani-theme');
  }
}

/**
 * Reads the current computed token values from <html> to seed a new custom theme.
 * Returns all 13 CORE_TOKENS as hex strings.
 * @returns {Record<string, string>}
 */
export function snapshotCurrentTokens() {
  const style = getComputedStyle(document.documentElement);
  /** @type {Record<string, string>} */
  const tokens = {};
  for (const k of CORE_TOKENS) {
    tokens[k] = style.getPropertyValue(k).trim();
  }
  return tokens;
}

/** @returns {{ theme: string, density: string, accent: string }} */
export function getCurrentTheme() {
  return {
    theme:   localStorage.getItem('kani-theme')   || 'system',
    density: localStorage.getItem('kani-density') || 'comfortable',
    accent:  localStorage.getItem('kani-accent')  || _DEFAULT_ACCENT,
  };
}

/**
 * Derive the hover and translucent accent variants from a hex colour.
 * @param {string} hex
 * @returns {{ color: string, hover: string, dim: string }}
 */
export function accentFromHex(hex) {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  const d = /** @param {number} n */ n => Math.max(0, Math.round(n * 0.84)).toString(16).padStart(2, '0');
  return { color: hex, hover: `#${d(r)}${d(g)}${d(b)}`, dim: `rgba(${r},${g},${b},0.15)` };
}

/** @param {string} theme @param {string} density @param {string} accent */
function _applyRaw(theme, density, accent) {
  const h = document.documentElement;

  for (const t of _ALL_THEME_TOKENS) h.style.removeProperty(t);
  applyCustomCss(undefined);

  if (theme && theme.startsWith('custom:')) {
    const id = theme.slice(7);
    const custom = getCustomThemes().find(c => c.id === id);
    if (custom) {
      h.removeAttribute('data-theme');
      if (density === 'compact') h.setAttribute('data-density', 'compact');
      else h.removeAttribute('data-density');
      for (const [k, v] of Object.entries(custom.tokens)) {
        h.style.setProperty(k, v);
      }
      applyCustomCss(custom.customCss);
      const bg = custom.tokens['--color-bg'];
      if (bg) {
        for (const meta of document.querySelectorAll('meta[name="theme-color"]')) {
          meta.setAttribute('content', bg);
        }
      }
      return;
    }
  }

  if (theme === 'system') {
    const light = window.matchMedia('(prefers-color-scheme: light)').matches;
    if (light) h.setAttribute('data-theme', 'light');
    else h.removeAttribute('data-theme');
  } else if (theme === 'dark') {
    h.removeAttribute('data-theme');
  } else {
    h.setAttribute('data-theme', theme);
  }

  if (density === 'compact') h.setAttribute('data-density', 'compact');
  else h.removeAttribute('data-density');

  if (accent && accent !== _DEFAULT_ACCENT) {
    const { color, hover, dim } = accentFromHex(accent);
    h.style.setProperty('--color-accent', color);
    h.style.setProperty('--color-accent-hover', hover);
    h.style.setProperty('--color-accent-dim', dim);
  }

  const effectiveTheme = theme === 'system'
    ? (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
    : theme;
  for (const meta of document.querySelectorAll('meta[name="theme-color"]')) {
    meta.setAttribute('content', effectiveTheme === 'light' ? '#f5f4f0' : '#111113');
  }
}

/**
 * Apply theme/density/accent, optionally crossfading via View Transitions.
 * @param {string} theme
 * @param {string} density
 * @param {string} accent
 */
export function applyTheme(theme, density, accent) {
  const fn = () => _applyRaw(theme, density, accent);
  if ('startViewTransition' in document && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    /** @type {any} */ (document).startViewTransition(fn);
  } else {
    fn();
  }
}

/** Persist to localStorage and apply. */
export function saveAndApplyTheme(theme, density, accent) {
  localStorage.setItem('kani-theme', theme);
  localStorage.setItem('kani-density', density);
  localStorage.setItem('kani-accent', accent);
  applyTheme(theme, density, accent);
}

/**
 * Persist and apply a custom theme by id without touching density or accent prefs.
 * @param {string} id
 */
export function applyCustomTheme(id) {
  const { density, accent } = getCurrentTheme();
  localStorage.setItem('kani-theme', `custom:${id}`);
  applyTheme(`custom:${id}`, density, accent);
}

/** @type {MediaQueryList | null} */
let _sysMq = null;

/** Call once at app startup — wires the system-preference listener. */
export function initTheme() {
  _sysMq = window.matchMedia('(prefers-color-scheme: light)');
  _sysMq.addEventListener('change', () => {
    const cur = getCurrentTheme();
    if (cur.theme === 'system') _applyRaw('system', cur.density, cur.accent);
  });
  const { theme, density, accent } = getCurrentTheme();
  _applyRaw(theme, density, accent);
}


/**
 * Install (or remove) a theme's custom CSS.
 *
 * Pass `raw: true` ONLY for text the user is still typing. Stored CSS arrives
 * already sanitised *and scoped* by the server, and scoping is not idempotent —
 * re-sanitising it yields `[data-kani-theme] [data-kani-theme] .btn`, a
 * descendant selector that matches nothing, silently disabling every rule.
 * @param {string | undefined} css
 * @param {{ raw?: boolean }} [opts]
 */
export function applyCustomCss(css, opts = {}) {
  const h = document.documentElement;
  const existing = document.getElementById('kani-theme-css');
  if (!css || !css.trim()) {
    existing?.remove();
    h.removeAttribute('data-kani-theme');
    return;
  }
  const safe = opts.raw ? sanitizeCss(css).css : css;
  if (!safe.trim()) {
    existing?.remove();
    h.removeAttribute('data-kani-theme');
    return;
  }
  const el = existing ?? document.createElement('style');
  el.id = 'kani-theme-css';
  el.textContent = safe;
  if (!existing) document.head.appendChild(el);
  h.setAttribute('data-kani-theme', '');
}

/** @param {any} t */
function _fromServer(t) {
  return {
    id: t.id,
    name: t.name,
    tokens: t.tokens ?? {},
    customCss: t.custom_css ?? undefined,
    instanceWide: !!t.instance_wide,
  };
}

/**
 * Reconcile the local cache with the server, which owns the themes.
 * localStorage is only a cache so boot can apply a theme without a round-trip;
 * local-only themes are uploaded on first sync and the cache is then replaced
 * by the server's list, so the two cannot diverge.
 */
export async function syncServerThemes() {
  const api = await import('./api.js');

  let remote;
  try {
    remote = await api.getUiThemes();
  } catch {
    return;
  }

  const serverThemes = (remote?.themes ?? []).map(_fromServer);
  const serverIds = new Set(serverThemes.map((t) => t.id));

  const orphans = getCustomThemes().filter((t) => !serverIds.has(t.id));
  for (const local of orphans) {
    try {
      const saved = await api.saveUiTheme({
        name: local.name,
        tokens: local.tokens,
        custom_css: local.customCss ?? null,
      });
      serverThemes.push(_fromServer(saved));
      if (localStorage.getItem('kani-theme') === `custom:${local.id}`) {
        localStorage.setItem('kani-theme', `custom:${saved.id}`);
      }
    } catch {
      serverThemes.push(local);
    }
  }

  _saveCustomThemeList(serverThemes);

  if (remote?.active_id) {
    const selection = `custom:${remote.active_id}`;
    if (localStorage.getItem('kani-theme') !== selection) {
      const { density, accent } = getCurrentTheme();
      localStorage.setItem('kani-theme', selection);
      applyTheme(selection, density, accent);
      return;
    }
  }

  const { theme, density, accent } = getCurrentTheme();
  if (theme.startsWith('custom:')) applyTheme(theme, density, accent);
}
