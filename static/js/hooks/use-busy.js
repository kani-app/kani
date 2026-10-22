// @ts-check
import { useState, useCallback, useRef } from 'preact/hooks';

/**
 * Tracks in-flight state for an async handler so a control can disable itself
 * while the operation runs — the Preact counterpart to `withBusy` in utils.js.
 * `run` wraps an async function, toggling `busy` around it and guarding against
 * re-entry while one call is already in flight.
 * @returns {{ busy: boolean, run: <T>(fn: () => Promise<T>) => Promise<T | undefined> }}
 */
export function useBusy() {
  const [busy, setBusy] = useState(false);
  const inflight = useRef(false);

  const run = useCallback(async (/** @type {() => Promise<any>} */ fn) => {
    if (inflight.current) return undefined;
    inflight.current = true;
    setBusy(true);
    try {
      return await fn();
    } finally {
      inflight.current = false;
      setBusy(false);
    }
  }, []);

  return { busy, run };
}
