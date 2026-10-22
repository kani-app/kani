// @ts-check

/**
 * The `data:` URL for a source's base64 icon, typed by its bytes, or null when it is not a
 * PNG or WebP image.
 * @param {string | null | undefined} icon
 * @returns {string | null}
 */
export function sourceIconSrc(icon) {
  if (!icon) return null;
  let head;
  try {
    head = atob(icon.slice(0, 16));
  } catch {
    return null;
  }
  if (head.startsWith('\x89PNG\r\n\x1a\n')) return `data:image/png;base64,${icon}`;
  if (head.startsWith('RIFF') && head.slice(8, 12) === 'WEBP') return `data:image/webp;base64,${icon}`;
  return null;
}
