// @ts-check
// URL query-parameter helpers shared across pages.

/**
 * @param {string} key
 * @param {string|null} [defaultValue]
 * @returns {string|null}
 */
export function getParam(key, defaultValue = null) {
  return new URLSearchParams(location.search).get(key) ?? defaultValue;
}

/**
 * Build a URL string for the current pathname with the given params.
 * Keys with null/undefined/'' values are omitted.
 * @param {Record<string, string|number|boolean|string[]|null|undefined>} params
 * @returns {string}
 */
function buildUrl(params) {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (Array.isArray(v)) {
      v.forEach(item => p.append(k, String(item)));
    } else if (v !== null && v !== undefined && v !== '') {
      p.set(k, String(v));
    }
  }
  const qs = p.toString();
  return qs ? `${location.pathname}?${qs}` : location.pathname;
}

/**
 * Push a new history entry with the given params.
 * @param {Record<string, string|number|boolean|string[]|null|undefined>} params
 */
export function pushState(params) {
  history.pushState(null, '', buildUrl(params));
}

/**
 * Replace the current history entry with the given params.
 * @param {Record<string, string|number|boolean|string[]|null|undefined>} params
 */
export function replaceState(params) {
  history.replaceState(null, '', buildUrl(params));
}
