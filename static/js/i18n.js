// @ts-check
import catalog from '../locales/en.js';

/**
 * Returns the localised string for key, falling back to the key itself.
 * @param {string} key
 * @param {Record<string, string | number>} [vars] — placeholder substitutions
 * @returns {string}
 */
export function t(key, vars = {}) {
  let str = /** @type {Record<string,string>} */ (catalog)[key] ?? key;
  for (const [k, v] of Object.entries(vars)) {
    str = str.replace(`{${k}}`, String(v));
  }
  return str;
}
