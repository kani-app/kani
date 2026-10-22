// @ts-check
const CHANNEL_NAME = 'kani-state';

/** @type {BroadcastChannel | null} */
let _channel = null;

function _getChannel() {
  if (!_channel && typeof BroadcastChannel !== 'undefined') {
    _channel = new BroadcastChannel(CHANNEL_NAME);
  }
  return _channel;
}

/**
 * Broadcast a state atom change to other tabs.
 * @param {string} key
 * @param {any} value
 */
export function broadcastStateChange(key, value) {
  try { _getChannel()?.postMessage({ key, value }); } catch { }
}

/**
 * Listen for state changes broadcast from other tabs.
 * Returns an unsubscribe function.
 * @param {(key: string, value: any) => void} onMessage
 * @returns {() => void}
 */
export function listenForStateChanges(onMessage) {
  const ch = _getChannel();
  if (!ch) return () => {};
  const handler = (/** @type {MessageEvent} */ e) => onMessage(e.data.key, e.data.value);
  ch.addEventListener('message', handler);
  return () => ch.removeEventListener('message', handler);
}
