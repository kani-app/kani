
import { signal, effect, untracked } from '@preact/signals';

/**
 * @param {Record<string, any>} [initial] Seed values for declared keys; keys
 *   absent here are created lazily with an `undefined` value on first access.
 */
export function createSignalStore(initial = {}) {
  /** @type {Map<string, import('@preact/signals').Signal>} */
  const _sigs = new Map();

  /**
   * @param {string} key
   * @returns {import('@preact/signals').Signal}
   */
  function sigFor(key) {
    let s = _sigs.get(key);
    if (!s) {
      s = signal(Object.prototype.hasOwnProperty.call(initial, key) ? initial[key] : undefined);
      _sigs.set(key, s);
    }
    return s;
  }

  for (const key of Object.keys(initial)) sigFor(key);

  /**
   * @param {string} key
   * @returns {any}
   */
  function getState(key) {
    return sigFor(key).value;
  }

  /**
   * @param {string} key
   * @param {any} value
   */
  function setState(key, value) {
    sigFor(key).value = value;
  }

  /**
   * @param {string} key
   * @param {(current: any) => any} fn
   */
  function updateState(key, fn) {
    const s = sigFor(key);
    s.value = fn(s.value);
  }

  /**
   * Fire `listener` on every future change to `key` (not immediately, matching
   * the old pub/sub contract). Returns an unsubscribe function.
   * @param {string} key
   * @param {(value: any) => void} listener
   * @returns {() => void}
   */
  function subscribe(key, listener) {
    const s = sigFor(key);
    let primed = false;
    return effect(() => {
      const value = s.value;
      if (primed) {
        untracked(() => {
          try { listener(value); } catch (e) { console.error('Store listener error:', e); }
        });
      } else {
        primed = true;
      }
    });
  }

  return { sigFor, getState, setState, updateState, subscribe };
}
