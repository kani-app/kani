// @ts-check
import { useEffect } from 'preact/hooks';

/**
 * Subscribe to SSE events of a specific type. Auto-unsubscribes on unmount.
 * The `onEvent` callback is intentionally excluded from the deps array — callers
 * should pass a stable function (useCallback or module-level) to avoid resubscribing.
 * @param {string} type - SSE event type (matches `data.type` from the server)
 * @param {(data: any) => void} onEvent
 */
export function useSSE(type, onEvent) {
  useEffect(() => {
    function handler(/** @type {CustomEvent} */ e) {
      if (/** @type {any} */ (e).detail?.type !== type) return;
      onEvent(/** @type {any} */ (e).detail);
    }
    window.addEventListener('kani:sse', /** @type {EventListener} */ (handler));
    return () => window.removeEventListener('kani:sse', /** @type {EventListener} */ (handler));
  }, [type]);
}
